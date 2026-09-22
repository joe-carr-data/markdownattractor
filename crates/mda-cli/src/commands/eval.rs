//! `mda eval --golden <dir>` — retrieval metrics on a golden set, offline; and
//! `mda eval --dataset docsqa --data <dir> --project <p> --root <checkout>` — the DocsQA-Repo
//! adapter of the benchmark plan (page-level metrics, coverage report, seeded split), over a
//! root that was indexed beforehand (`mda index --no-summarize <checkout>`).
//!
//! The golden directory holds `docs/` (a markdown corpus) and `queries.jsonl`, one query
//! per line: `{"q": "...", "expect": [{"path": "a.md", "heading": "Rollback"}], "temporal":
//! false}`. A hit counts when its path matches and, if `heading` is given, that text occurs
//! in the hit's heading path. Optional `cards.json` (`{"<section_hash>": <SectionSummary>}`)
//! turns on the hybrid run: cards are attached without any model call and, when the
//! embedding model is on disk, vectors are built too.
//!
//! Metrics: a query's `expect` entries are *alternatives* (any of them answers it), so the
//! headline number is **success@k** (an expected section is in the top k), and **MRR@k** is
//! the mean reciprocal rank of the first expected section within the top k (0 beyond k).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use mda_core::card::{Provenance, SCHEMA_VERSION, SectionSummary};
use mda_core::config::Config;
use mda_core::pipeline::Engine;
use mda_core::search::{self, SearchOptions};
use mda_core::store::{Store, Usage};
use serde::{Deserialize, Serialize};

use crate::output::{self, Style};

/// Arguments for `mda eval`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory with `docs/`, `queries.jsonl` and optionally `cards.json` (default
    /// `evals/golden` when no `--dataset` is given).
    #[arg(long, conflicts_with = "dataset")]
    pub golden: Option<PathBuf>,
    /// Cutoff for recall@k (golden set).
    #[arg(short, long, default_value_t = 5)]
    pub k: usize,
    /// Produce `cards.json` by summarizing the corpus with the configured backend (spends
    /// money on the `api` backend), then evaluate (golden set).
    #[arg(long)]
    pub record: bool,
    /// A public dataset adapter: `docsqa` (DocsQA-Repo, `PowderXu/docsqa-data`).
    #[arg(long, value_parser = ["docsqa"], requires = "data", requires = "project", requires = "root")]
    pub dataset: Option<String>,
    /// The dataset checkout (for `docsqa`: the directory holding `data/questions.jsonl`,
    /// `data/answers.jsonl` and the decompressed `data/corpus.jsonl`).
    #[arg(long)]
    pub data: Option<PathBuf>,
    /// The project inside the dataset (`github-docs`, `prisma`, `supabase`, `tailwind-css`).
    #[arg(long)]
    pub project: Option<String>,
    /// The indexed checkout of that project's repository at the pinned commit.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Which split to score: `dev`, `test`, `holdout`, or `all` (plan rule 0.2). The sealed
    /// holdout is scored only with `--open-holdout`.
    #[arg(long, default_value = "dev")]
    pub split: String,
    /// Score the sealed holdout too (plan rule 0.2: once, at 1.0).
    #[arg(long)]
    pub open_holdout: bool,
    /// Seed of the dev/test/holdout split.
    #[arg(long, default_value_t = 20_260_922)]
    pub seed: u64,
    /// Recorded cards (`{"<section_hash>": <SectionSummary>}`) to attach before scoring, so
    /// the hybrid runs need no model.
    #[arg(long)]
    pub cards: Option<PathBuf>,
    /// Sections fetched per query before page deduplication.
    #[arg(long, default_value_t = 30)]
    pub fetch: usize,
    /// Write `coverage.json`, `split.json` and `results.json` here.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct Expect {
    path: String,
    #[serde(default)]
    heading: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Query {
    q: String,
    expect: Vec<Expect>,
    #[serde(default)]
    temporal: bool,
    #[serde(default)]
    since: Option<String>,
}

/// One run's numbers.
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    /// Which configuration.
    pub run: String,
    /// Queries evaluated.
    pub queries: usize,
    /// Fraction of queries with an expected section in the top k (success@k).
    pub success_at_k: f64,
    /// Mean reciprocal rank of the first expected section within the top k (MRR@k).
    pub mrr_at_k: f64,
    /// Queries that missed, with the top hit for each.
    pub misses: Vec<Miss>,
    /// Mean query latency in milliseconds.
    pub mean_ms: f64,
}

/// A query whose expected section was not in the top k.
#[derive(Debug, Clone, Serialize)]
pub struct Miss {
    /// The query.
    pub q: String,
    /// What was expected.
    pub expected: Vec<String>,
    /// The top hit, if any.
    pub got: Option<String>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    if args.dataset.as_deref() == Some("docsqa") {
        return run_docsqa(args, json);
    }
    let golden_arg = args.golden.clone().unwrap_or_else(|| PathBuf::from("evals/golden"));
    let golden = golden_arg
        .canonicalize()
        .with_context(|| format!("golden set {}", golden_arg.display()))?;
    let queries = load_queries(&golden.join("queries.jsonl"))?;
    let cards: Option<HashMap<String, SectionSummary>> =
        match std::fs::read_to_string(golden.join("cards.json")) {
            Ok(text) => Some(serde_json::from_str(&text).context("cards.json")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };

    // A private root: the corpus is copied so nothing under evals/ is ever written.
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    copy_tree(&golden.join("docs"), &root)?;
    let cfg = Config::load(&golden).unwrap_or_default();
    cfg.save(&root)?;
    let mut engine = Engine::open(&root)?;
    let report = engine.index_root()?;
    let cards = if args.record { Some(record_cards(&mut engine, &golden, json)?) } else { cards };

    let mut runs = Vec::new();
    runs.push(evaluate("lexical (raw only)", &engine, &queries, args.k, None, true)?);
    if let Some(cards) = &cards {
        let attached = attach_cards(engine.store_mut(), cards)?;
        let embedder = super::embedder_for(engine.config()).filter(|e| e.ready());
        let mut vectors = 0;
        if let Some(e) = &embedder {
            vectors = engine.embed_pending(&**e, usize::MAX)?.embedded;
        }
        runs.push(evaluate("lexical (cards + raw)", &engine, &queries, args.k, None, false)?);
        if embedder.is_some() {
            runs.push(evaluate(
                "hybrid (cards + raw + vectors)",
                &engine,
                &queries,
                args.k,
                embedder.as_deref(),
                false,
            )?);
        }
        tracing::info!(attached, vectors, "golden cards attached");
    }

    if json {
        output::json(&serde_json::json!({
            "golden": golden,
            "files": report.files,
            "sections": engine.store().counts()?.sections,
            "queries": queries.len(),
            "temporal_queries": queries.iter().filter(|q| q.temporal).count(),
            "k": args.k,
            "runs": runs,
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    println!(
        "{} {} · {} files · {} sections · {} queries ({} temporal) · k={}",
        st.bold("eval"),
        st.dim(&golden.display().to_string()),
        report.files,
        engine.store().counts()?.sections,
        queries.len(),
        queries.iter().filter(|q| q.temporal).count(),
        args.k
    );
    println!("{:<34} {:>9} {:>7} {:>8}", "run", "success@k", "MRR@k", "mean ms");
    for r in &runs {
        println!("{:<34} {:>9.3} {:>7.3} {:>8.1}", r.run, r.success_at_k, r.mrr_at_k, r.mean_ms);
    }
    if let Some(last) = runs.last()
        && !last.misses.is_empty()
    {
        println!("{} ({}):", st.warn("misses in the last run"), last.misses.len());
        for m in &last.misses {
            println!(
                "  {:?} → expected {} · got {}",
                m.q,
                m.expected.join(" | "),
                m.got.as_deref().unwrap_or("nothing")
            );
        }
    }
    if cards.is_none() {
        println!(
            "  {} no cards.json in the golden set: only the lexical run was measured",
            st.dim("note:")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The DocsQA-Repo adapter: coverage first, then one row per configuration over the split.
#[allow(clippy::too_many_lines)]
fn run_docsqa(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    use mda_core::eval::Split;
    use mda_core::eval::docsqa::{self, RunOptions};
    let (Some(data), Some(project), Some(root)) = (&args.data, &args.project, &args.root) else {
        anyhow::bail!("--dataset docsqa needs --data, --project and --root");
    };
    let split = match args.split.as_str() {
        "all" => None,
        s => Some(Split::parse(s).ok_or_else(|| {
            anyhow::anyhow!("--split must be dev, test, holdout or all, not {s:?}")
        })?),
    };
    if split == Some(Split::Holdout) && !args.open_holdout {
        anyhow::bail!(
            "the holdout is sealed until 1.0 (plan rule 0.2); pass --open-holdout to score it"
        );
    }
    if args.open_holdout {
        tracing::warn!("scoring the sealed holdout: plan rule 0.2 opens it once, at 1.0");
    }
    let root = root.canonicalize().with_context(|| format!("root {}", root.display()))?;
    let out = args.out.as_deref().map(|o| report_dir(o, &root)).transpose()?;
    anyhow::ensure!(
        root.join(mda_core::config::STATE_DIR).join("index.sqlite").is_file(),
        "{} is not indexed yet: run `mda index --no-summarize {}` first",
        root.display(),
        root.display()
    );
    let dataset = docsqa::Dataset::load(data, project)
        .with_context(|| format!("loading {project} from {}", data.display()))?;
    let mut engine = Engine::open(&root)?;
    let mut attached = 0;
    if let Some(cards) = &args.cards {
        let text = std::fs::read_to_string(cards).with_context(|| cards.display().to_string())?;
        let cards: HashMap<String, SectionSummary> =
            serde_json::from_str(&text).context("cards")?;
        attached = attach_cards(engine.store_mut(), &cards)?;
    }
    let counts = engine.store().counts()?;
    let coverage = docsqa::coverage(&dataset, engine.store())?;
    let splits = dataset.split(args.seed);
    let split_counts: HashMap<String, usize> =
        splits.values().fold(HashMap::new(), |mut acc, s| {
            *acc.entry(format!("{s:?}").to_lowercase()).or_default() += 1;
            acc
        });

    let embedder = super::embedder_for(engine.config()).filter(|e| e.ready());
    let mut vectors = 0;
    if let Some(e) = &embedder
        && counts.summarized > 0
    {
        vectors = engine.embed_pending(&**e, usize::MAX)?.embedded;
    }
    let mut runs = Vec::new();
    let opts = |name: &str, raw_only: bool| RunOptions {
        name: name.to_owned(),
        raw_only,
        fetch: args.fetch,
        include_holdout: args.open_holdout,
    };
    let raw = opts("lexical (raw only)", true);
    runs.push(docsqa::evaluate(engine.store(), None, &dataset, &splits, split, &raw)?);
    if engine.store().counts()?.summarized > 0 {
        let lex = opts("lexical (cards + raw)", false);
        runs.push(docsqa::evaluate(engine.store(), None, &dataset, &splits, split, &lex)?);
        if embedder.is_some() {
            let hyb = opts("hybrid (cards + raw + vectors)", false);
            runs.push(docsqa::evaluate(
                engine.store(),
                embedder.as_deref(),
                &dataset,
                &splits,
                split,
                &hyb,
            )?);
        }
    }
    tracing::info!(attached, vectors, "docsqa cards attached");

    let report = serde_json::json!({
        "dataset": "docsqa",
        "data": portable(data),
        "project": project,
        "root": portable(&root),
        "mda_version": mda_core::VERSION,
        "store": counts,
        "cards_attached": attached,
        "embedding_model": embedder.as_ref().map(|e| e.model().to_owned()),
        "coverage": coverage,
        "seed": args.seed,
        "split": split,
        "split_counts": split_counts,
        "fetch": args.fetch,
        "runs": runs,
    });
    if let Some(out) = &out {
        write_report(&out.join("coverage.json"), &serde_json::to_string_pretty(&coverage)?)?;
        let mut split_rows: Vec<(&String, &Split)> = splits.iter().collect();
        split_rows.sort_by(|a, b| a.0.cmp(b.0));
        write_report(
            &out.join("split.json"),
            &serde_json::to_string_pretty(&serde_json::json!({
                "seed": args.seed, "project": project,
                "questions": split_rows.iter().map(|(id, s)| serde_json::json!({"id": id, "split": s})).collect::<Vec<_>>(),
            }))?,
        )?;
        write_report(&out.join("results.json"), &serde_json::to_string_pretty(&report)?)?;
    }
    if json {
        output::json(&report);
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    println!(
        "{} docsqa/{} · {} docs · {} sections ({} carded) · split {} (seed {}) · dev/test/holdout {}/{}/{}",
        st.bold("eval"),
        project,
        counts.docs,
        counts.sections,
        counts.summarized,
        args.split,
        args.seed,
        split_counts.get("dev").copied().unwrap_or(0),
        split_counts.get("test").copied().unwrap_or(0),
        split_counts.get("holdout").copied().unwrap_or(0),
    );
    println!(
        "evidence: {} of {} anchors found in the indexed text; {} eligible question(s) with a missing anchor{}",
        coverage.anchors_found,
        coverage.anchors,
        coverage.questions_with_missing_anchor,
        if coverage.missing_anchor_ids.is_empty() {
            String::new()
        } else {
            format!(" ({})", coverage.missing_anchor_ids.join(", "))
        }
    );
    println!(
        "coverage: {:.1}% of {} qrels indexed ({} unmapped) · {} of {} corpus pages indexed · {} questions: {} eligible, {} excluded (image evidence), {} excluded (page missing); {} need multimodal grading",
        coverage.qrel_coverage * 100.0,
        coverage.qrels,
        coverage.qrels_unmapped,
        coverage.corpus_pages_indexed,
        coverage.corpus_pages,
        coverage.questions,
        coverage.eligible,
        coverage.excluded_image_evidence,
        coverage.excluded_missing_page,
        coverage.multimodal_judgment,
    );
    if coverage.qrel_coverage < 0.95 {
        println!("  {} coverage is below the plan's 95% gate", st.warn("gate:"));
    }
    println!(
        "{:<34} {:>5} {:>9} {:>7} {:>8} {:>8}",
        "run", "n", "success@5", "MRR@5", "nDCG@10", "mean ms"
    );
    for r in &runs {
        println!(
            "{:<34} {:>5} {:>9.3} {:>7.3} {:>8.3} {:>8.1}",
            r.run,
            r.metrics.questions,
            r.metrics.success_at_5,
            r.metrics.mrr_at_5,
            r.metrics.ndcg_at_10,
            r.metrics.mean_ms
        );
    }
    if let Some(out) = &out {
        println!("  {} {}", st.dim("written:"), out.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// The report directory: created, canonicalized, and never inside the checkout being
/// scored (a report must not land among the source files).
fn report_dir(out: &Path, root: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(out).with_context(|| out.display().to_string())?;
    let out = out.canonicalize().with_context(|| out.display().to_string())?;
    anyhow::ensure!(
        !out.starts_with(root),
        "--out {} is inside the checkout {}; write reports elsewhere",
        out.display(),
        root.display()
    );
    Ok(out)
}

/// Write a report file, replacing a previous plain file but never following a symlink.
fn write_report(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(meta.is_file(), "{} exists and is not a plain file", path.display());
    }
    std::fs::write(path, text).with_context(|| path.display().to_string())
}

/// A path with the home directory replaced by `~`, so a report can be committed as is.
fn portable(path: &Path) -> String {
    let s = path.display().to_string();
    match std::env::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => {
            let home = home.display().to_string();
            s.strip_prefix(&home).map_or(s.clone(), |rest| format!("~{rest}"))
        }
        _ => s,
    }
}

/// Summarize the corpus with the configured backend and write `cards.json` next to it.
fn record_cards(
    engine: &mut Engine,
    golden: &Path,
    json: bool,
) -> anyhow::Result<HashMap<String, SectionSummary>> {
    let backend = mda_core::worker::backend_for(engine.config())
        .context("preparing the backend for --record (run `mda doctor`)")?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let report = rt.block_on(engine.summarize_pending(
        backend,
        tokio_util::sync::CancellationToken::new(),
        mda_core::pipeline::SummarizeOptions::default(),
        |_| {},
    ))?;
    let mut cards = HashMap::new();
    for doc in engine.store().documents()? {
        for s in engine.store().sections_of(&doc.doc_id)? {
            if let Some(summary) = s.summary {
                cards.insert(s.section_hash, summary);
            }
        }
    }
    let path = golden.join("cards.json");
    std::fs::write(&path, serde_json::to_string_pretty(&cards)?)?;
    if !json {
        println!(
            "recorded {} card(s) to {} · {} failed · ${:.4}",
            cards.len(),
            path.display(),
            report.failed,
            report.pool.usage.cost_usd
        );
    }
    // The store already has these cards attached; the caller re-attaches harmlessly.
    Ok(cards)
}

fn load_queries(path: &Path) -> anyhow::Result<Vec<Query>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let q: Query =
            serde_json::from_str(line).with_context(|| format!("{}:{}", path.display(), n + 1))?;
        out.push(q);
    }
    anyhow::ensure!(!out.is_empty(), "no queries in {}", path.display());
    Ok(out)
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    for entry in walkdir(from)? {
        let rel = entry.strip_prefix(from)?;
        let dest = to.join(rel);
        if std::fs::symlink_metadata(&entry)?.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

/// Regular files and directories under `dir`. Symlinks and anything else are skipped, so a
/// link in the fixture can neither pull outside files into the corpus nor loop.
fn walkdir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| d.display().to_string())? {
            let p = e?.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if meta.is_dir() {
                stack.push(p.clone());
                out.push(p);
            } else if meta.is_file() {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn attach_cards(
    store: &mut Store,
    cards: &HashMap<String, SectionSummary>,
) -> anyhow::Result<usize> {
    let mut n = 0;
    let prov = Provenance {
        model: "golden".to_owned(),
        prompt_version: "recorded".to_owned(),
        schema_version: SCHEMA_VERSION,
        backend: "golden".to_owned(),
        summarized_at: jiff::Timestamp::now(),
        truncated: false,
    };
    for p in store.pending_hashes(usize::MAX)? {
        if let Some(card) = cards.get(&p.section_hash) {
            store.attach_summary(&p.section_hash, card, &prov, &Usage::default())?;
            n += 1;
        }
    }
    Ok(n)
}

fn evaluate(
    name: &str,
    engine: &Engine,
    queries: &[Query],
    k: usize,
    embedder: Option<&dyn mda_core::embed::Embedder>,
    raw_only: bool,
) -> anyhow::Result<Metrics> {
    let mut hits_at_k = 0usize;
    let mut rr_sum = 0.0f64;
    let mut ms_sum = 0.0f64;
    let mut misses = Vec::new();
    for q in queries {
        let opts = SearchOptions {
            k,
            raw_only,
            since: q.since.as_deref().map(super::parse_time).transpose()?,
            ..SearchOptions::default()
        };
        let started = std::time::Instant::now();
        let hits = search::search_with(engine.store(), &q.q, &opts, embedder)?;
        ms_sum += started.elapsed().as_secs_f64() * 1000.0;
        let rank = hits.iter().position(|h| {
            q.expect.iter().any(|e| {
                h.rel_path == e.path
                    && e.heading.as_deref().is_none_or(|needle| {
                        h.heading_path.join(" › ").to_lowercase().contains(&needle.to_lowercase())
                    })
            })
        });
        match rank {
            Some(r) => {
                hits_at_k += 1;
                #[allow(clippy::cast_precision_loss)]
                {
                    rr_sum += 1.0 / (r as f64 + 1.0);
                }
            }
            None => misses.push(Miss {
                q: q.q.clone(),
                expected: q
                    .expect
                    .iter()
                    .map(|e| match &e.heading {
                        Some(h) => format!("{} › {h}", e.path),
                        None => e.path.clone(),
                    })
                    .collect(),
                got: hits
                    .first()
                    .map(|h| format!("{} › {}", h.rel_path, h.heading_path.join(" › "))),
            }),
        }
    }
    #[allow(clippy::cast_precision_loss)] // query counts are tiny
    let (n, found) = (queries.len() as f64, hits_at_k as f64);
    Ok(Metrics {
        run: name.to_owned(),
        queries: queries.len(),
        success_at_k: found / n,
        mrr_at_k: rr_sum / n,
        misses,
        mean_ms: ms_sum / n,
    })
}
