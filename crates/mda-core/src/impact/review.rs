//! A review: what changed since a revision, and which other sections that possibly leaves
//! out of date (ADR-0009). This is what `mda check` runs.
//!
//! 1. **Snapshot.** The revision is resolved once to a commit that must be an ancestor of
//!    `HEAD`; the raw index is brought up to date with the working files (no model call).
//! 2. **Changes.** Every markdown file that differs from the commit is compared section by
//!    section ([`super::changes`]). A new file, a deleted file and a section that cannot be
//!    paired are listed as not reviewed, with the reason.
//! 3. **Candidates** for every change ([`super::candidates`]), then judgments dealt one per
//!    change in turn up to the cap, one call at a time ([`super::judgment`]).
//! 4. **Spend.** The daily budget is read before each call; each pair gets one retry; every
//!    attempt is written to the usage ledger whatever its outcome.
//! 5. **Before reporting**, every file that takes part in a finding is read again: a finding
//!    whose file moved on meanwhile is withheld and counted.
//!
//! A review that could not cover everything says what it did not cover. Nothing here writes
//! to the user's files.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use jiff::Timestamp;
use serde::Serialize;

use super::candidates::{Candidate, Finder, Limits};
use super::changes::{SectionChange, compare};
use super::judgment::{self, Assessed, DocContext, JudgmentLimits, Quote, Verdict};
use crate::gittime::{self, Discovery, FileChange};
use crate::markdown::{self, Flavor};
use crate::pipeline::Engine;
use crate::store::{TimeSource, Usage};
use crate::worker::{Backend, StructuredStatus};
use crate::{Error, Result};

/// What a review is asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    /// The revision to compare the working files with.
    pub since: String,
    /// Most pairs sent to the model.
    pub max_judgments: usize,
    /// The model the judgments run on.
    pub model: String,
    /// Caps of the candidate step.
    pub candidates: Limits,
    /// Caps of one judgment.
    pub judgment: JudgmentLimits,
}

impl Options {
    /// The defaults: compare with `HEAD`, at most 40 judgments.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            since: "HEAD".to_owned(),
            max_judgments: 40,
            model: model.into(),
            candidates: Limits::default(),
            judgment: JudgmentLimits::default(),
        }
    }
}

/// Something the review left out, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unreviewed {
    /// The file.
    pub rel_path: String,
    /// The section, when the reason is about one section.
    pub heading_path: Option<Vec<String>>,
    /// `new_file`, `deleted_file`, `not_utf8`, `repeated_heading`, `renamed_or_moved`,
    /// `deleted`, `too_large`.
    pub reason: String,
}

/// A section named in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionRef {
    /// The file.
    pub rel_path: String,
    /// Its heading path.
    pub heading_path: Vec<String>,
}

/// One pair and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairOutcome {
    /// The changed section.
    pub change: SectionRef,
    /// The section it was compared with.
    pub candidate: SectionRef,
    /// The query terms the candidate carries.
    pub shared_terms: Vec<String>,
    /// `possibly_outdated`, `possible_conflict`, `related`, `unrelated`, `abstain`,
    /// `dropped_unquoted`, `failed`, `too_large`, `stale_during_review`, or `not_judged`.
    pub outcome: String,
    /// Why, for `dropped_unquoted`, `failed` and `not_judged`.
    pub detail: Option<String>,
}

/// A possible disagreement, with its evidence as it stands in the files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportFinding {
    /// `possibly_outdated` or `possible_conflict`.
    pub verdict: Verdict,
    /// The changed section.
    pub change: SectionRef,
    /// The other section.
    pub candidate: SectionRef,
    /// What the changed section said before (for `possibly_outdated`). Lines are those of
    /// the file at the revision.
    pub before: Option<Quote>,
    /// What the changed section says now.
    pub after: Quote,
    /// What the other section says.
    pub candidate_quote: Quote,
    /// When the index last saw the other section change.
    pub candidate_updated_at: Timestamp,
    /// Where that date comes from (git or the file's time).
    pub candidate_updated_source: TimeSource,
    /// The model's one-sentence reading, when it takes no side.
    pub model_reading: Option<String>,
}

/// The result of a review.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Report {
    /// The commit the working files were compared with.
    pub revision: String,
    /// Markdown files that differ from it.
    pub files_changed: usize,
    /// Sections that changed or are new, in files that existed at the revision.
    pub changes: usize,
    /// What was left out, with reasons.
    pub not_reviewed: Vec<Unreviewed>,
    /// Sections of the index that share a term with some change (summed over changes).
    pub matching: u64,
    /// Pairs kept after the caps.
    pub candidates: usize,
    /// Changes whose candidate search was cut short (scan or term-lookup limit).
    pub searches_truncated: usize,
    /// Pairs the model answered for.
    pub judged: usize,
    /// Pairs kept but not judged: no backend, the cap, the budget, or a stopped run.
    pub not_judged: usize,
    /// Verdicts given, by name.
    pub verdicts: BTreeMap<String, usize>,
    /// Alerts dropped because their quotes do not stand in the sources.
    pub dropped_unquoted: usize,
    /// Pairs whose call failed twice.
    pub failed: usize,
    /// Findings withheld because a file changed during the review.
    pub stale_during_review: usize,
    /// The backend's name; `None` when there is none and nothing was judged.
    pub backend: Option<String>,
    /// Why the run stopped early, when it did.
    pub stopped: Option<String>,
    /// Tokens and cost of every attempt of this review, as the backend reported them.
    pub usage: Usage,
    /// Attempts for which the backend reported no usage (unknown, not free).
    pub usage_unknown: usize,
    /// The findings.
    pub findings: Vec<ReportFinding>,
    /// Every pair with its outcome.
    pub pairs: Vec<PairOutcome>,
}

/// A change with the file it is in.
struct Changed {
    rel_path: String,
    after_text: String,
    change: SectionChange,
}

/// Run a review of `engine`'s root. `backend` is `None` when no summarization backend is
/// configured: candidates are then listed and nothing is judged.
pub async fn review(
    engine: &mut Engine,
    backend: Option<&dyn Backend>,
    opts: &Options,
) -> Result<Report> {
    let repo = match gittime::discover(engine.root()) {
        Discovery::Usable(repo) => repo,
        other => {
            return Err(Error::Git(format!(
                "a review compares with git history, which is not usable here ({})",
                other.name()
            )));
        }
    };
    let commit = repo.resolve_commit(&opts.since)?;
    if !repo.is_ancestor(&commit, &repo.head()?)? {
        return Err(Error::Git(format!(
            "{} is not an ancestor of HEAD: a review looks at what changed since an earlier commit of this branch",
            opts.since
        )));
    }
    // The working files as they are now, in the raw index (no model call).
    engine.index_root()?;

    let mut report = Report {
        revision: commit.clone(),
        backend: backend.map(|b| b.name().to_owned()),
        ..Report::default()
    };
    let changed = collect_changes(engine, &repo, &commit, &mut report)?;
    report.changes = changed.len();

    // Candidates, then pairs dealt one per change in turn.
    let mut per_change: Vec<Vec<Candidate>> = Vec::with_capacity(changed.len());
    {
        let finder = Finder::new(engine.store(), opts.candidates.clone())?;
        for c in &changed {
            let found = finder.for_change(&c.rel_path, &c.change)?;
            report.matching += found.matching;
            if found.scan_truncated || found.terms_unexamined > 0 {
                report.searches_truncated += 1;
            }
            per_change.push(found.kept);
        }
    }
    let deepest = per_change.iter().map(Vec::len).max().unwrap_or(0);
    let mut queue: Vec<(usize, usize)> = Vec::new();
    for rank in 0..deepest {
        queue.extend((0..changed.len()).filter(|c| rank < per_change[*c].len()).map(|c| (c, rank)));
    }
    report.candidates = queue.len();

    let mut alerted: HashSet<(usize, String)> = HashSet::new();
    for (position, (c, rank)) in queue.iter().copied().enumerate() {
        let (change, candidate) = (&changed[c], &per_change[c][rank]);
        let mut pair = PairOutcome {
            change: section_ref(&change.rel_path, &change.change.heading_path),
            candidate: section_ref(&candidate.section.rel_path, &candidate.section.heading_path),
            shared_terms: candidate.shared_terms.clone(),
            outcome: "not_judged".to_owned(),
            detail: None,
        };
        let why_not = match backend {
            None => Some("no summarization backend".to_owned()),
            Some(_) if report.stopped.is_some() => report.stopped.clone(),
            Some(_) if position >= opts.max_judgments => Some("above --max-judgments".to_owned()),
            Some(_) => budget_used_up(engine)?,
        };
        if let Some(why) = why_not {
            if backend.is_some() && report.stopped.is_none() && position < opts.max_judgments {
                report.stopped = Some(why.clone());
            }
            pair.detail = Some(why);
            report.not_judged += 1;
            report.pairs.push(pair);
            continue;
        }
        let Some(backend) = backend else { continue };
        let finding =
            judge_pair(engine, backend, opts, change, candidate, &mut pair, &mut report).await?;
        if let Some(finding) = finding {
            // One alert per (change, other document).
            if alerted.insert((c, candidate.section.doc_id.clone())) {
                report.findings.push(finding);
            }
        }
        report.pairs.push(pair);
    }
    Ok(report)
}

/// The changed and new sections of every markdown file that differs from `commit`; what is
/// not reviewed goes into the report.
fn collect_changes(
    engine: &Engine,
    repo: &gittime::Repo,
    commit: &str,
    report: &mut Report,
) -> Result<Vec<Changed>> {
    let working: Vec<String> =
        engine.store().documents()?.into_iter().map(|d| d.rel_path).collect();
    let tree = repo.tree(commit)?;
    let differing =
        repo.changed_since(commit, &working, &|p| crate::walk::is_markdown(Path::new(p)))?;
    report.files_changed = differing.len();
    let mut changed = Vec::new();
    let mut skip = |rel_path: &str, heading_path: Option<Vec<String>>, reason: &str| {
        report.not_reviewed.push(Unreviewed {
            rel_path: rel_path.to_owned(),
            heading_path,
            reason: reason.to_owned(),
        });
    };
    for (kind, rel_path) in differing {
        match kind {
            FileChange::Added => skip(&rel_path, None, "new_file"),
            FileChange::Deleted => skip(&rel_path, None, "deleted_file"),
            FileChange::Modified => {
                let Some(blob) = tree.get(&rel_path) else { continue };
                let Ok(before) = String::from_utf8(repo.blob(blob)?) else {
                    skip(&rel_path, None, "not_utf8");
                    continue;
                };
                let abs = engine.safe_join(&rel_path)?;
                let Ok(after) = std::fs::read_to_string(&abs) else {
                    skip(&rel_path, None, "not_utf8");
                    continue;
                };
                let file = compare(&before, &after, Flavor::of_path(Path::new(&rel_path)));
                for n in file.not_reviewed {
                    let reason = serde_json::to_value(n.reason)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    skip(&rel_path, Some(n.heading_path), &reason);
                }
                changed.extend(file.changes.into_iter().map(|change| Changed {
                    rel_path: rel_path.clone(),
                    after_text: after.clone(),
                    change,
                }));
            }
        }
    }
    Ok(changed)
}

/// `Some(reason)` when the daily token budget leaves nothing for another call. With a daemon
/// spending at the same moment this is a check before each call, not a strict ceiling.
fn budget_used_up(engine: &Engine) -> Result<Option<String>> {
    let Some(budget) = engine.config().daily_token_budget else { return Ok(None) };
    let spent = engine.store().usage_since(crate::pipeline::start_of_today())?;
    Ok((spent.input_tokens + spent.output_tokens >= budget)
        .then(|| "the daily token budget is used up".to_owned()))
}

/// Judge one pair: at most two attempts, each written to the ledger. Returns the finding when
/// there is one that still stands in the files.
async fn judge_pair(
    engine: &mut Engine,
    backend: &dyn Backend,
    opts: &Options,
    change: &Changed,
    candidate: &Candidate,
    pair: &mut PairOutcome,
    report: &mut Report,
) -> Result<Option<ReportFinding>> {
    let section = &candidate.section;
    let id = pair_id(&change.rel_path, &change.change.heading_path, &section.section_id);
    let request = judgment::request(
        &id,
        &doc_context(engine, &change.rel_path)?,
        &change.change,
        &doc_context(engine, &section.rel_path)?,
        section,
        &opts.judgment,
    );
    let Ok(request) = request else {
        "too_large".clone_into(&mut pair.outcome);
        report.not_reviewed.push(Unreviewed {
            rel_path: change.rel_path.clone(),
            heading_path: Some(change.change.heading_path.clone()),
            reason: "too_large".to_owned(),
        });
        return Ok(None);
    };

    let mut last_failure = String::new();
    for _attempt in 0..2 {
        let result = backend.structured(&request, &opts.model).await?;
        let kind = result.kind();
        // The ledger takes every attempt; an unknown usage is written as zero and named.
        let (spent, outcome) = if let Some(u) = &result.usage {
            let spent = Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cost_usd: u.cost_usd,
            };
            (spent, format!("impact_{kind}"))
        } else {
            report.usage_unknown += 1;
            (Usage::default(), format!("impact_{kind}_usage_unknown"))
        };
        report.usage.input_tokens += spent.input_tokens;
        report.usage.output_tokens += spent.output_tokens;
        report.usage.cost_usd += spent.cost_usd;
        engine.store_mut().record_usage(&section.section_hash, &opts.model, &spent, &outcome)?;
        let answer = match result.status {
            StructuredStatus::Ok { value } => judgment::parse_answer(&value),
            StructuredStatus::Fatal { reason } => {
                report.stopped = Some(format!("the backend stopped: {reason}"));
                "failed".clone_into(&mut pair.outcome);
                pair.detail = Some(reason);
                report.failed += 1;
                return Ok(None);
            }
            StructuredStatus::Malformed { reason, .. }
            | StructuredStatus::Retryable { reason }
            | StructuredStatus::RateLimited { reason } => Err(reason),
        };
        let answer = match answer {
            Ok(answer) => answer,
            Err(why) => {
                last_failure = why;
                continue;
            }
        };
        report.judged += 1;
        let name = verdict_name(answer.verdict);
        *report.verdicts.entry(name.clone()).or_default() += 1;
        return Ok(match judgment::assess(&answer, &change.change, section) {
            Assessed::NotAlert(_) => {
                pair.outcome = name;
                None
            }
            Assessed::DroppedUnquoted { why, .. } => {
                "dropped_unquoted".clone_into(&mut pair.outcome);
                pair.detail = Some(format!("{name}: {why}"));
                report.dropped_unquoted += 1;
                None
            }
            Assessed::Finding(finding) => {
                let fresh = still_stands(engine, change, candidate, finding)?;
                if fresh.is_some() {
                    pair.outcome = name;
                } else {
                    "stale_during_review".clone_into(&mut pair.outcome);
                    report.stale_during_review += 1;
                }
                fresh
            }
        });
    }
    "failed".clone_into(&mut pair.outcome);
    pair.detail = Some(last_failure);
    report.failed += 1;
    Ok(None)
}

/// Read both files of a finding again. The changed file must read as it did when the review
/// compared it; the other file must still hold the candidate section, whose lines are taken
/// from this last read. Otherwise the finding is withheld.
fn still_stands(
    engine: &Engine,
    change: &Changed,
    candidate: &Candidate,
    finding: judgment::Finding,
) -> Result<Option<ReportFinding>> {
    let read = |rel: &str| -> Result<Option<String>> {
        Ok(std::fs::read_to_string(engine.safe_join(rel)?).ok())
    };
    if read(&change.rel_path)?.as_deref() != Some(change.after_text.as_str()) {
        return Ok(None);
    }
    let section = &candidate.section;
    let Some(now) = read(&section.rel_path)? else { return Ok(None) };
    let parsed = markdown::parse_str_as(&now, Flavor::of_path(Path::new(&section.rel_path)));
    let same = |s: &&markdown::Section| {
        s.hash == section.section_hash && s.heading_path == section.heading_path
    };
    let Some(current) = parsed.sections.iter().find(same) else { return Ok(None) };
    // The section may have moved in its file since the index read it.
    let moved = |line: u32| line - section.line_start.min(line) + current.line_start;
    let candidate_quote = Quote {
        text: finding.candidate.text,
        line_start: moved(finding.candidate.line_start),
        line_end: moved(finding.candidate.line_end),
    };
    Ok(Some(ReportFinding {
        verdict: finding.verdict,
        change: section_ref(&change.rel_path, &change.change.heading_path),
        candidate: section_ref(&section.rel_path, &section.heading_path),
        before: finding.before,
        after: finding.after,
        candidate_quote,
        candidate_updated_at: section.updated_at,
        candidate_updated_source: section.updated_source,
        model_reading: finding.model_reading,
    }))
}

/// What the index knows about a document besides one section: its front matter and its
/// opening lines.
fn doc_context(engine: &Engine, rel_path: &str) -> Result<DocContext> {
    let store = engine.store();
    let Some(doc) = store.document_by_path(rel_path)? else {
        return Ok(DocContext { rel_path: rel_path.to_owned(), ..DocContext::default() });
    };
    let opening = store.sections_of(&doc.doc_id)?.into_iter().next().map(|s| s.text);
    Ok(DocContext {
        rel_path: rel_path.to_owned(),
        frontmatter: doc.frontmatter,
        opening: opening.unwrap_or_default(),
    })
}

/// The id of a pair's request, for logs: the changed file, its section's last heading, and
/// the other section's id.
#[must_use]
pub fn pair_id(change_path: &str, heading_path: &[String], candidate_section_id: &str) -> String {
    let heading = heading_path.last().map_or("", String::as_str);
    format!("impact:{change_path}#{heading}:{candidate_section_id}")
}

fn section_ref(rel_path: &str, heading_path: &[String]) -> SectionRef {
    SectionRef { rel_path: rel_path.to_owned(), heading_path: heading_path.to_vec() }
}

fn verdict_name(verdict: Verdict) -> String {
    serde_json::to_value(verdict)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
