//! DocsQA-Repo adapter (`PowderXu/docsqa-data`, schema v3): real community questions over
//! four documentation repositories pinned to commits, with sparse page-level relevance
//! labels (`qrel_ids`). We index the repository source at the pinned commit, so a label's
//! `doc_id` is mapped to the page's `repository_source_path` through `corpus.jsonl` and
//! looked up in our store by relative path. Plan §2 calls this the "source-repository
//! adaptation" and asks for the coverage report before any number.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Metrics, Split, pages_of, score_pages, split_ids};
use crate::embed::Embedder;
use crate::search::{self, SearchOptions};
use crate::store::Store;
use crate::{Error, Result};

/// One question with its labels, as this adapter needs it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    /// Stable id (`question_id`).
    pub id: String,
    /// The agent input, image transcriptions included.
    pub query: String,
    /// Relevant pages as repository-relative paths (mapped from `qrel_ids`, deduplicated).
    pub relevant: Vec<String>,
    /// The dataset's resolved evidence anchors: `(page path, canonical heading)`; a heading
    /// of `None` means the whole page. Used by the evidence-presence check (plan §2 F1 v).
    pub anchors: Vec<(String, Option<String>)>,
    /// Labels that could not be mapped to a page of the corpus file.
    pub unmapped_qrels: Vec<String>,
    /// The reference answer needed evidence from image-derived text (excluded, plan §2 iv).
    pub image_evidence: bool,
    /// The reference answer needs multimodal judgment to grade (axis B caveat).
    pub multimodal_judgment: bool,
}

/// The questions of one project, plus the page map.
#[derive(Debug, Clone)]
pub struct Dataset {
    /// Project id as the dataset names it (`github-docs`, `prisma`, `supabase`,
    /// `tailwind-css`).
    pub project: String,
    /// Every question of the project, in file order.
    pub questions: Vec<Question>,
    /// `doc_id` → repository-relative source path, for the whole project corpus.
    pub pages: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct QuestionRow {
    question_id: String,
    project: String,
    query: String,
}

/// A flag the dataset writes either as a boolean or as the list of evidence items behind it
/// (`image_text_evidence_used` is a list in schema v3; a non-empty list means "used").
#[derive(Deserialize)]
#[serde(untagged)]
enum Flag {
    Bool(bool),
    List(Vec<serde_json::Value>),
}

impl Flag {
    fn is_set(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::List(v) => !v.is_empty(),
        }
    }
}

impl Default for Flag {
    fn default() -> Self {
        Self::Bool(false)
    }
}

#[derive(Deserialize)]
struct AnchorRow {
    doc_id: String,
    #[serde(default)]
    canonical_anchor: Option<String>,
    #[serde(default)]
    canonical_heading: Option<String>,
}

#[derive(Deserialize)]
struct AnswerRow {
    question_id: String,
    #[serde(default)]
    qrel_ids: Vec<String>,
    #[serde(default)]
    anchor_resolution: Vec<AnchorRow>,
    #[serde(default)]
    image_text_evidence_used: Flag,
    #[serde(default)]
    requires_multimodal_judgment: Flag,
}

#[derive(Deserialize)]
struct CorpusRow {
    doc_id: String,
    project: String,
    repository_source_path: String,
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            serde_json::from_str(l).map_err(|e| Error::parse(path, format!("line {}: {e}", n + 1)))
        })
        .collect()
}

impl Dataset {
    /// Load one project from a checkout of `docsqa-data` (`data/questions.jsonl`,
    /// `data/answers.jsonl`, `data/corpus.jsonl`; the corpus ships gzipped and must be
    /// decompressed first).
    pub fn load(dir: &Path, project: &str) -> Result<Self> {
        let data = dir.join("data");
        let corpus_path = data.join("corpus.jsonl");
        if !corpus_path.is_file() {
            return Err(Error::NotFound(format!(
                "{}: decompress the corpus first: gunzip -k {}",
                corpus_path.display(),
                data.join("corpus.jsonl.gz").display()
            )));
        }
        let pages: BTreeMap<String, String> = read_jsonl::<CorpusRow>(&corpus_path)?
            .into_iter()
            .filter(|r| r.project == project)
            .map(|r| (r.doc_id, r.repository_source_path))
            .collect();
        let answers: HashMap<String, AnswerRow> =
            read_jsonl::<AnswerRow>(&data.join("answers.jsonl"))?
                .into_iter()
                .map(|a| (a.question_id.clone(), a))
                .collect();
        let mut questions = Vec::new();
        for q in read_jsonl::<QuestionRow>(&data.join("questions.jsonl"))? {
            if q.project != project {
                continue;
            }
            let a = answers.get(&q.question_id).ok_or_else(|| {
                Error::NotFound(format!("answers.jsonl has no record for {}", q.question_id))
            })?;
            let mut relevant: Vec<String> = Vec::new();
            let mut unmapped = Vec::new();
            for id in &a.qrel_ids {
                match pages.get(id) {
                    Some(p) if !relevant.contains(p) => relevant.push(p.clone()),
                    Some(_) => {}
                    None => unmapped.push(id.clone()),
                }
            }
            let anchors = a
                .anchor_resolution
                .iter()
                .filter_map(|r| {
                    let page = pages.get(&r.doc_id)?.clone();
                    let heading = match r.canonical_anchor.as_deref() {
                        None | Some("document" | "") => None,
                        Some(_) => r.canonical_heading.clone().filter(|h| !h.trim().is_empty()),
                    };
                    Some((page, heading))
                })
                .collect();
            questions.push(Question {
                id: q.question_id,
                query: q.query,
                relevant,
                anchors,
                unmapped_qrels: unmapped,
                image_evidence: a.image_text_evidence_used.is_set(),
                multimodal_judgment: a.requires_multimodal_judgment.is_set(),
            });
        }
        if questions.is_empty() {
            let known: Vec<String> = read_jsonl::<QuestionRow>(&data.join("questions.jsonl"))?
                .into_iter()
                .map(|q| q.project)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            return Err(Error::NotFound(format!(
                "no questions for project {project:?}; projects in the dataset: {known:?}"
            )));
        }
        Ok(Self { project: project.to_owned(), questions, pages })
    }

    /// The split of every question (seeded, stratified within the project).
    #[must_use]
    pub fn split(&self, seed: u64) -> HashMap<String, Split> {
        let ids: Vec<String> = self.questions.iter().map(|q| q.id.clone()).collect();
        split_ids(seed, &ids)
    }
}

/// The ingestion gate (plan §2 F1): how much of the dataset our index can see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    /// Project id.
    pub project: String,
    /// Pages in the dataset corpus for this project.
    pub corpus_pages: usize,
    /// Of those, pages whose source path is a document in our store.
    pub corpus_pages_indexed: usize,
    /// Relevance labels over all questions.
    pub qrels: usize,
    /// Labels whose page is in our store.
    pub qrels_indexed: usize,
    /// Labels that no corpus row maps (dataset-side gap).
    pub qrels_unmapped: usize,
    /// Questions in the project.
    pub questions: usize,
    /// Questions excluded because their evidence is image-derived text.
    pub excluded_image_evidence: usize,
    /// Questions excluded because a label is unmapped or not indexed.
    pub excluded_missing_page: usize,
    /// Questions left for scoring.
    pub eligible: usize,
    /// Questions (among the eligible) whose grading needs multimodal judgment (axis B).
    pub multimodal_judgment: usize,
    /// Fraction `qrels_indexed / qrels` (the plan's ≥ 0.95 gate).
    pub qrel_coverage: f64,
    /// Evidence anchors over the eligible questions (plan §2 F1 v): a section-level anchor
    /// counts as found when a section of that page carries the canonical heading (compared
    /// after [`normalize_heading`]: case, backticks, Liquid tags and whitespace folded; the
    /// page title counts as a heading too); a page-level anchor counts when the page is
    /// indexed.
    pub anchors: usize,
    /// Anchors whose heading (or page) was found in the indexed text.
    pub anchors_found: usize,
    /// Eligible questions with at least one anchor not found; they stay eligible (the label
    /// is a page) and are listed so the number is visible, never silent.
    pub questions_with_missing_anchor: usize,
    /// Their ids.
    pub missing_anchor_ids: Vec<String>,
}

/// Compute the coverage of `dataset` against an indexed store.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn coverage(dataset: &Dataset, store: &Store) -> Result<Coverage> {
    let docs = store.documents()?;
    let indexed: HashSet<String> = docs.iter().map(|d| d.rel_path.clone()).collect();
    // Lower-cased heading paths per indexed page, loaded once for the evidence check.
    let mut headings: HashMap<String, Vec<String>> = HashMap::new();
    let wanted: HashSet<&str> =
        dataset.questions.iter().flat_map(|q| q.anchors.iter().map(|(p, _)| p.as_str())).collect();
    for d in &docs {
        if wanted.contains(d.rel_path.as_str()) {
            let mut paths: Vec<String> = store
                .sections_of(&d.doc_id)?
                .into_iter()
                .map(|s| normalize_heading(&s.heading_path.join(" > ")))
                .collect();
            if let Some(t) = &d.title {
                paths.push(normalize_heading(t));
            }
            headings.insert(d.rel_path.clone(), paths);
        }
    }
    let mut anchors = 0;
    let mut anchors_found = 0;
    let mut missing_anchor_ids = Vec::new();
    let corpus_pages_indexed = dataset.pages.values().filter(|p| indexed.contains(*p)).count();
    let mut qrels = 0;
    let mut qrels_indexed = 0;
    let mut qrels_unmapped = 0;
    let mut excluded_image = 0;
    let mut excluded_missing = 0;
    let mut eligible = 0;
    let mut multimodal = 0;
    for q in &dataset.questions {
        qrels += q.relevant.len() + q.unmapped_qrels.len();
        qrels_unmapped += q.unmapped_qrels.len();
        let present = q.relevant.iter().filter(|p| indexed.contains(*p)).count();
        qrels_indexed += present;
        if q.image_evidence {
            excluded_image += 1;
        } else if !q.unmapped_qrels.is_empty()
            || present < q.relevant.len()
            || q.relevant.is_empty()
        {
            excluded_missing += 1;
        } else {
            eligible += 1;
            if q.multimodal_judgment {
                multimodal += 1;
            }
            let mut missing = false;
            for (page, heading) in &q.anchors {
                anchors += 1;
                let found = match heading {
                    None => indexed.contains(page),
                    Some(h) => {
                        let needle = normalize_heading(h);
                        !needle.is_empty()
                            && headings
                                .get(page)
                                .is_some_and(|hp| hp.iter().any(|p| p.contains(&needle)))
                    }
                };
                if found {
                    anchors_found += 1;
                } else {
                    missing = true;
                }
            }
            if missing {
                missing_anchor_ids.push(q.id.clone());
            }
        }
    }
    let questions_with_missing_anchor = missing_anchor_ids.len();
    Ok(Coverage {
        project: dataset.project.clone(),
        corpus_pages: dataset.pages.len(),
        corpus_pages_indexed,
        qrels,
        qrels_indexed,
        qrels_unmapped,
        questions: dataset.questions.len(),
        excluded_image_evidence: excluded_image,
        excluded_missing_page: excluded_missing,
        eligible,
        multimodal_judgment: multimodal,
        qrel_coverage: if qrels == 0 { 0.0 } else { qrels_indexed as f64 / qrels as f64 },
        anchors,
        anchors_found,
        questions_with_missing_anchor,
        missing_anchor_ids,
    })
}

/// Heading text as the evidence check compares it: lower-cased, backticks removed, Liquid
/// tags (`{% … %}`) dropped, whitespace collapsed. The dataset's canonical headings come
/// from rendered pages; ours come from the source.
#[must_use]
pub fn normalize_heading(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{%") {
        out.push_str(&rest[..start]);
        match rest[start..].find("%}") {
            Some(end) => rest = &rest[start + end + 2..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace('`', "").to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a question is scored: not image-evidence, every label mapped and indexed.
fn eligible(q: &Question, indexed: &HashSet<String>) -> bool {
    !q.image_evidence
        && q.unmapped_qrels.is_empty()
        && !q.relevant.is_empty()
        && q.relevant.iter().all(|p| indexed.contains(p))
}

/// One question's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResult {
    /// `question_id`.
    pub id: String,
    /// Its split.
    pub split: Split,
    /// Rank (1-based) of the first relevant page, if any within the fetched list.
    pub rank: Option<usize>,
    /// nDCG@10.
    pub ndcg_at_10: f64,
    /// Sections fetched to reach ten distinct pages (or exhaust the results).
    pub fetched: usize,
    /// `true` when ten distinct pages were not reached within the fetch cap, so a relevant
    /// page beyond the fetched sections could be missing from `rank`.
    pub truncated: bool,
    /// The top five pages returned.
    pub top: Vec<String>,
    /// The relevant pages.
    pub relevant: Vec<String>,
}

/// One configuration's numbers over the chosen split, with every question's row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// Configuration name (`lexical (raw only)`, `hybrid`, …).
    pub run: String,
    /// Which split was scored (`None` for every eligible question).
    pub split: Option<Split>,
    /// The metrics.
    pub metrics: Metrics,
    /// Per question.
    pub results: Vec<QuestionResult>,
    /// Scored questions for which an external arm returned no row (plan §2.2): each is
    /// scored as a miss and counted in every denominator. Empty for the store's own rows.
    #[serde(default)]
    pub missing: Vec<String>,
    /// Rows of an external arm whose `question_id` is not in the dataset at all (a driver
    /// bug worth seeing); rows for questions outside the scored split are simply unused.
    #[serde(default)]
    pub unknown: Vec<String>,
}

/// How to run one configuration.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Name for the row.
    pub name: String,
    /// Skip cards and vectors.
    pub raw_only: bool,
    /// Sections fetched per query at first; doubled until ten distinct pages are in hand,
    /// the results run out, or [`FETCH_CAP`] is reached.
    pub fetch: usize,
    /// Score holdout questions too (plan rule 0.2: only at 1.0).
    pub include_holdout: bool,
}

/// Upper bound on sections fetched for one question.
pub const FETCH_CAP: usize = 2000;

/// Pages needed for nDCG@10.
const PAGES_NEEDED: usize = 10;

/// Score `dataset` on `store` for one configuration, over `split` (or all eligible questions).
#[allow(clippy::cast_precision_loss, clippy::implicit_hasher)] // `splits` comes from `split_ids`
pub fn evaluate(
    store: &Store,
    embedder: Option<&dyn Embedder>,
    dataset: &Dataset,
    splits: &HashMap<String, Split>,
    split: Option<Split>,
    opts: &RunOptions,
) -> Result<Run> {
    let indexed: HashSet<String> = store.documents()?.into_iter().map(|d| d.rel_path).collect();
    let search_opts = SearchOptions {
        k: opts.fetch,
        raw_only: opts.raw_only,
        recency_half_life_days: 0.0, // a frozen corpus has no "recent"
        ..SearchOptions::default()
    };
    let mut results = Vec::new();
    let (mut hits5, mut rr_sum, mut ndcg_sum, mut ms_sum) = (0usize, 0.0f64, 0.0f64, 0.0f64);
    for q in &dataset.questions {
        let q_split = *splits.get(&q.id).unwrap_or(&Split::Holdout);
        if !eligible(q, &indexed) || split.is_some_and(|s| s != q_split) {
            continue;
        }
        if q_split == Split::Holdout && !opts.include_holdout {
            continue;
        }
        let started = std::time::Instant::now();
        // Page ranks need ten distinct pages: fetch deeper while one page hogs the list.
        let mut fetch = opts.fetch.clamp(1, FETCH_CAP);
        let (ranked, fetched, truncated) = loop {
            let hits = search::search_with(
                store,
                &q.query,
                &SearchOptions { k: fetch, ..search_opts.clone() },
                embedder,
            )?;
            let ranked = pages_of(hits.iter().map(|h| h.rel_path.as_str()));
            let exhausted = hits.len() < fetch;
            if ranked.len() >= PAGES_NEEDED || exhausted {
                break (ranked, fetch, false);
            }
            if fetch >= FETCH_CAP {
                break (ranked, fetch, true);
            }
            fetch = (fetch * 2).min(FETCH_CAP);
        };
        ms_sum += started.elapsed().as_secs_f64() * 1000.0;
        let (rank, rr, ndcg) = score_pages(&ranked, &q.relevant);
        if rank.is_some_and(|r| r <= 5) {
            hits5 += 1;
        }
        rr_sum += rr;
        ndcg_sum += ndcg;
        results.push(QuestionResult {
            id: q.id.clone(),
            split: q_split,
            rank,
            ndcg_at_10: ndcg,
            fetched,
            truncated,
            top: ranked.into_iter().take(5).collect(),
            relevant: q.relevant.clone(),
        });
    }
    let n = results.len().max(1) as f64;
    Ok(Run {
        run: opts.name.clone(),
        split,
        metrics: Metrics {
            questions: results.len(),
            success_at_5: hits5 as f64 / n,
            mrr_at_5: rr_sum / n,
            ndcg_at_10: ndcg_sum / n,
            mean_ms: Some(ms_sum / n),
        },
        results,
        missing: Vec::new(),
        unknown: Vec::new(),
    })
}

/// One row of an external arm's output (`--arm-output`, plan §2.2): the ranked repository
/// paths the arm's driver returned for one question, and whether the driver hit a limit
/// before reaching ten distinct pages or exhaustion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmRow {
    /// `question_id`.
    pub question_id: String,
    /// Repository-relative paths, best first; duplicates and `./` prefixes are tolerated.
    pub paths: Vec<String>,
    /// The driver could not reach ten distinct pages or exhaustion (scored and counted).
    #[serde(default)]
    pub truncated: bool,
}

/// Read an arm's rows from a JSONL file. A `question_id` that appears twice is an error
/// (two ranked lists for one question cannot both be the arm's answer).
pub fn read_arm_output(path: &Path) -> Result<HashMap<String, ArmRow>> {
    let mut rows = HashMap::new();
    for row in read_jsonl::<ArmRow>(path)? {
        if rows.insert(row.question_id.clone(), row).is_some() {
            return Err(Error::parse(path, "a question_id appears twice"));
        }
    }
    Ok(rows)
}

/// A path as a driver may write it, folded to the store's form: backslashes to slashes,
/// leading `./` and `/` removed, repeated slashes collapsed.
#[must_use]
pub fn normalize_arm_path(path: &str) -> String {
    let mut p = path.trim().replace('\\', "/");
    while p.starts_with("./") || p.starts_with('/') {
        p = p.trim_start_matches("./").trim_start_matches('/').to_owned();
    }
    p.split('/').filter(|seg| !seg.is_empty() && *seg != ".").collect::<Vec<_>>().join("/")
}

/// Score an external arm's ranked lists with the same eligibility (the store decides which
/// labels are indexed), split, page rule and metrics as the store's own rows (plan §2.2).
/// A scored question without a row is a miss, listed in [`Run::missing`].
#[allow(clippy::cast_precision_loss, clippy::implicit_hasher)] // `splits` comes from `split_ids`
pub fn score_arm(
    store: &Store,
    dataset: &Dataset,
    splits: &HashMap<String, Split>,
    split: Option<Split>,
    rows: &HashMap<String, ArmRow>,
    name: &str,
    include_holdout: bool,
) -> Result<Run> {
    let indexed: HashSet<String> = store.documents()?.into_iter().map(|d| d.rel_path).collect();
    let known: HashSet<&str> = dataset.questions.iter().map(|q| q.id.as_str()).collect();
    let mut unknown: Vec<String> =
        rows.keys().filter(|id| !known.contains(id.as_str())).cloned().collect();
    unknown.sort();
    let mut results = Vec::new();
    let mut missing = Vec::new();
    let (mut hits5, mut rr_sum, mut ndcg_sum) = (0usize, 0.0f64, 0.0f64);
    for q in &dataset.questions {
        let q_split = *splits.get(&q.id).unwrap_or(&Split::Holdout);
        if !eligible(q, &indexed) || split.is_some_and(|s| s != q_split) {
            continue;
        }
        if q_split == Split::Holdout && !include_holdout {
            continue;
        }
        let (ranked, fetched, truncated) = if let Some(row) = rows.get(&q.id) {
            let paths: Vec<String> = row.paths.iter().map(|p| normalize_arm_path(p)).collect();
            (pages_of(paths.iter().map(String::as_str)), row.paths.len(), row.truncated)
        } else {
            missing.push(q.id.clone());
            (Vec::new(), 0, false)
        };
        let (rank, rr, ndcg) = score_pages(&ranked, &q.relevant);
        if rank.is_some_and(|r| r <= 5) {
            hits5 += 1;
        }
        rr_sum += rr;
        ndcg_sum += ndcg;
        results.push(QuestionResult {
            id: q.id.clone(),
            split: q_split,
            rank,
            ndcg_at_10: ndcg,
            fetched,
            truncated,
            top: ranked.into_iter().take(5).collect(),
            relevant: q.relevant.clone(),
        });
    }
    let n = results.len().max(1) as f64;
    Ok(Run {
        run: name.to_owned(),
        split,
        metrics: Metrics {
            questions: results.len(),
            success_at_5: hits5 as f64 / n,
            mrr_at_5: rr_sum / n,
            ndcg_at_10: ndcg_sum / n,
            mean_ms: None,
        },
        results,
        missing,
        unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_arm_path, normalize_heading, read_arm_output};

    #[test]
    fn arm_paths_fold_to_the_store_form() {
        assert_eq!(normalize_arm_path("./docs/a.md"), "docs/a.md");
        assert_eq!(normalize_arm_path("/docs//a.md"), "docs/a.md");
        assert_eq!(normalize_arm_path(".//./docs/./a.md "), "docs/a.md");
        assert_eq!(normalize_arm_path("docs\\win\\a.mdx"), "docs/win/a.mdx");
        assert_eq!(normalize_arm_path("docs/a.md"), "docs/a.md");
        assert_eq!(normalize_arm_path(""), "");
    }

    #[test]
    fn arm_output_rejects_a_duplicated_question() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("arm.jsonl");
        std::fs::write(
            &f,
            "{\"question_id\":\"q1\",\"paths\":[\"a.md\"]}\n{\"question_id\":\"q2\",\"paths\":[],\"truncated\":true}\n",
        )
        .unwrap();
        let rows = read_arm_output(&f).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows["q2"].truncated && !rows["q1"].truncated);
        std::fs::write(
            &f,
            "{\"question_id\":\"q1\",\"paths\":[]}\n{\"question_id\":\"q1\",\"paths\":[]}\n",
        )
        .unwrap();
        let err = read_arm_output(&f).unwrap_err().to_string();
        assert!(err.contains("appears twice"), "{err}");
    }

    #[test]
    fn headings_normalise_like_the_dataset_renders_them() {
        assert_eq!(
            normalize_heading("Using the `GITHUB_TOKEN` in a workflow"),
            "using the github_token in a workflow"
        );
        assert_eq!(
            normalize_heading(
                "Connecting a repository on {% data variables.product.prodname_dotcom %}  today"
            ),
            "connecting a repository on today"
        );
        assert_eq!(normalize_heading("`PrismaClient`"), "prismaclient");
        assert_eq!(normalize_heading("open {% tag"), "open");
        assert_eq!(normalize_heading("   "), "");
    }
}
