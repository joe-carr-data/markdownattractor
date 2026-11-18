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
//! 5. **Before reporting**, after the last call, every file that takes part in a finding is
//!    read again: a finding whose changed file or other file reads differently than when it
//!    was judged (its text, its front matter, or the file being gone) is withheld and counted.
//!
//! What a concurrent writer can still do: between the path check and git's own read of a
//! file a path can be swapped, and a daemon re-indexing during the review can change what a
//! section id means. The first is narrowed (every path is checked to be a regular file under
//! the root just before); the second is caught (a loaded section must still have the hash and
//! heading of the hit that chose it, and its file is compared with the index again).
//!
//! A review that could not cover everything says what it did not cover. Nothing here writes
//! to the user's files.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
    /// How long to wait before the one retry of a rate-limited call.
    pub rate_limit_pause: Duration,
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
            rate_limit_pause: Duration::from_secs(2),
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
    /// `new_file`, `deleted_file`, `not_indexed`, `unreadable`, `not_utf8`,
    /// `repeated_heading`, `renamed_or_moved`, `deleted`, `too_large`.
    pub reason: String,
    /// For `too_large`: the other file of the pair that was too large to send together.
    pub with: Option<String>,
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
    /// Markdown files that differ from it in more than line endings (new and deleted ones
    /// included).
    pub files_changed: usize,
    /// Sections that changed or are new, in files that existed at the revision.
    pub changes: usize,
    /// What was left out, with reasons.
    pub not_reviewed: Vec<Unreviewed>,
    /// Sections of the index that share a term with some change (summed over changes).
    pub matching: u64,
    /// Pairs kept after the caps.
    pub candidates: usize,
    /// Changes whose candidate search was cut short (scan, term-lookup or linked-document
    /// limit).
    pub searches_truncated: usize,
    /// Sections passed over because their document already had its share of candidates.
    pub cut_per_document: usize,
    /// Changes for which no candidate was found.
    pub changes_without_candidates: Vec<SectionRef>,
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
    /// Why there is no backend, when the caller knows.
    pub backend_note: Option<String>,
    /// Findings not shown because the same change already has one for the same document.
    pub duplicate_findings: usize,
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
    /// The file's text as compared; one copy per file.
    after_text: Arc<str>,
    change: SectionChange,
}

/// A finding that waits for the final look at its files.
struct Provisional {
    change: usize,
    candidate_doc: String,
    candidate_path: String,
    /// The other file's text when the pair was judged.
    candidate_text: String,
    finding: ReportFinding,
}

/// Run a review of `engine`'s root. `backend` is `None` when no summarization backend is
/// configured: candidates are then listed and nothing is judged.
pub async fn review(
    engine: &mut Engine,
    backend: Option<&dyn Backend>,
    opts: &Options,
) -> Result<Report> {
    let (repo, commit) = pinned_commit(engine, &opts.since)?;
    // The working files as they are now go into the raw index below (no model call).
    let mut report = Report {
        revision: commit.clone(),
        backend: backend.map(|b| b.name().to_owned()),
        ..Report::default()
    };
    let synced = engine.index_root()?;
    for (path, _why) in &synced.errors {
        // A file the index could not read is not in the review; say so.
        let rel_path = engine.rel_path(Path::new(path)).unwrap_or_else(|_| path.clone());
        report.not_reviewed.push(Unreviewed {
            rel_path,
            heading_path: None,
            reason: "unreadable".to_owned(),
            with: None,
        });
    }
    let changed = collect_changes(engine, &repo, &commit, &mut report)?;
    report.changes = changed.len();

    // Candidates, then pairs dealt one per change in turn.
    let per_change = gather(engine, &changed, opts, &mut report)?;
    let deepest = per_change.iter().map(Vec::len).max().unwrap_or(0);
    let mut queue: Vec<(usize, usize)> = Vec::new();
    for rank in 0..deepest {
        queue.extend((0..changed.len()).filter(|c| rank < per_change[*c].len()).map(|c| (c, rank)));
    }
    report.candidates = queue.len();

    let mut provisional: Vec<Provisional> = Vec::new();
    let mut sent = 0usize;
    for (c, rank) in queue {
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
            Some(_) if sent >= opts.max_judgments => Some("above --max-judgments".to_owned()),
            Some(_) => None,
        };
        if let Some(why) = why_not {
            pair.detail = Some(why);
            report.not_judged += 1;
            report.pairs.push(pair);
            continue;
        }
        let Some(backend) = backend else { continue };
        let judged =
            judge_pair(engine, backend, opts, change, candidate, &mut pair, &mut report).await?;
        if judged.sent {
            sent += 1;
        }
        if let Some((finding, candidate_text)) = judged.finding {
            provisional.push(Provisional {
                change: c,
                candidate_doc: candidate.section.doc_id.clone(),
                candidate_path: candidate.section.rel_path.clone(),
                candidate_text,
                finding,
            });
        }
        report.pairs.push(pair);
    }
    settle(engine, &changed, provisional, &mut report);
    Ok(report)
}

/// The repository and the commit `since` names, which must be an ancestor of `HEAD`.
fn pinned_commit(engine: &Engine, since: &str) -> Result<(gittime::Repo, String)> {
    let repo = match gittime::discover(engine.root()) {
        Discovery::Usable(repo) => repo,
        other => {
            return Err(Error::Git(format!(
                "a review compares with git history, which is not usable here ({})",
                other.name()
            )));
        }
    };
    let commit = repo.resolve_commit(since)?;
    if !repo.is_ancestor(&commit, &repo.head()?)? {
        return Err(Error::Git(format!(
            "{since} is not an ancestor of HEAD: a review looks at what changed since an earlier commit of this branch"
        )));
    }
    Ok((repo, commit))
}

/// The candidates of every change, with what the searches left out added to the report.
fn gather(
    engine: &Engine,
    changed: &[Changed],
    opts: &Options,
    report: &mut Report,
) -> Result<Vec<Vec<Candidate>>> {
    let mut per_change: Vec<Vec<Candidate>> = Vec::with_capacity(changed.len());
    {
        let finder = Finder::new(engine.store(), opts.candidates.clone())?;
        for c in changed {
            let found = finder.for_change(&c.rel_path, &c.change)?;
            report.matching += found.matching;
            if found.scan_truncated || found.terms_unexamined > 0 || found.linked_documents_cut > 0
            {
                report.searches_truncated += 1;
            }
            report.cut_per_document += found.cut_per_document;
            if found.kept.is_empty() {
                report
                    .changes_without_candidates
                    .push(section_ref(&c.rel_path, &c.change.heading_path));
            }
            per_change.push(found.kept);
        }
    }
    Ok(per_change)
}

/// The last look, after the last call: a finding is reported only when both of its files
/// still read as they did when it was judged, and once per (change, other document).
fn settle(
    engine: &Engine,
    changed: &[Changed],
    provisional: Vec<Provisional>,
    report: &mut Report,
) {
    let read = |rel: &str| engine.safe_join(rel).ok().and_then(|p| std::fs::read_to_string(p).ok());
    let mut alerted: HashSet<(usize, String)> = HashSet::new();
    for p in provisional {
        let change = &changed[p.change];
        let fresh = read(&change.rel_path).as_deref() == Some(&*change.after_text)
            && read(&p.candidate_path).as_deref() == Some(p.candidate_text.as_str());
        if !fresh {
            report.stale_during_review += 1;
            let of_pair = |o: &&mut PairOutcome| {
                o.change == p.finding.change && o.candidate == p.finding.candidate
            };
            if let Some(o) = report.pairs.iter_mut().find(of_pair) {
                "stale_during_review".clone_into(&mut o.outcome);
            }
        } else if alerted.insert((p.change, p.candidate_doc)) {
            report.findings.push(p.finding);
        } else {
            report.duplicate_findings += 1;
        }
    }
}

/// The changed and new sections of every markdown file that differs from `commit`; what is
/// not reviewed goes into the report.
fn collect_changes(
    engine: &Engine,
    repo: &gittime::Repo,
    commit: &str,
    report: &mut Report,
) -> Result<Vec<Changed>> {
    // A path goes to git only when it is, right now, a regular file under the root.
    let on_disk = |rel: &str| {
        engine
            .safe_join(rel)
            .ok()
            .and_then(|p| std::fs::symlink_metadata(p).ok())
            .is_some_and(|m| m.is_file())
    };
    let working: Vec<String> = engine
        .store()
        .documents()?
        .into_iter()
        .map(|d| d.rel_path)
        .filter(|rel| on_disk(rel))
        .collect();
    let tree = repo.tree(commit)?;
    let differing =
        repo.changed_since(commit, &working, &|p| crate::walk::is_markdown(Path::new(p)))?;
    let mut changed = Vec::new();
    let mut files = 0;
    let mut skip = |rel_path: &str, heading_path: Option<Vec<String>>, reason: &str| {
        report.not_reviewed.push(Unreviewed {
            rel_path: rel_path.to_owned(),
            heading_path,
            reason: reason.to_owned(),
            with: None,
        });
    };
    for (kind, rel_path) in differing {
        match kind {
            FileChange::Added => {
                files += 1;
                skip(&rel_path, None, "new_file");
            }
            // In the commit and not in the index: gone, or a file the index leaves out (an
            // ignore rule, an unreadable file). Only the first is a deletion.
            FileChange::Deleted if on_disk(&rel_path) => skip(&rel_path, None, "not_indexed"),
            FileChange::Deleted => {
                files += 1;
                skip(&rel_path, None, "deleted_file");
            }
            FileChange::Modified => {
                let Some(blob) = tree.get(&rel_path) else { continue };
                let Ok(before) = String::from_utf8(repo.blob(blob)?) else {
                    files += 1;
                    skip(&rel_path, None, "not_utf8");
                    continue;
                };
                let Ok(after) = std::fs::read_to_string(engine.safe_join(&rel_path)?) else {
                    files += 1;
                    skip(&rel_path, None, "not_utf8");
                    continue;
                };
                let file = compare(&before, &after, Flavor::of_path(Path::new(&rel_path)));
                // Line endings alone are not a change.
                if !file.changes.is_empty() || !file.not_reviewed.is_empty() {
                    files += 1;
                }
                for n in file.not_reviewed {
                    let reason = serde_json::to_value(n.reason)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    skip(&rel_path, Some(n.heading_path), &reason);
                }
                let after: Arc<str> = Arc::from(after);
                changed.extend(file.changes.into_iter().map(|change| Changed {
                    rel_path: rel_path.clone(),
                    after_text: Arc::clone(&after),
                    change,
                }));
            }
        }
    }
    report.files_changed = files;
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

/// What became of one pair.
struct Judged {
    /// Whether a call was made for it (it then counts against the cap).
    sent: bool,
    /// A finding that stood when it was judged, with the other file's text at that moment.
    finding: Option<(ReportFinding, String)>,
}

/// Judge one pair: at most two attempts, the budget read before each, each attempt written
/// to the ledger whatever its outcome.
async fn judge_pair(
    engine: &mut Engine,
    backend: &dyn Backend,
    opts: &Options,
    change: &Changed,
    candidate: &Candidate,
    pair: &mut PairOutcome,
    report: &mut Report,
) -> Result<Judged> {
    let section = &candidate.section;
    let id = pair_id(&change.rel_path, &change.change.heading_path, &section.section_id);
    let candidate_doc = doc_context(engine, &section.rel_path)?;
    let request = judgment::request(
        &id,
        &doc_context(engine, &change.rel_path)?,
        &change.change,
        &candidate_doc,
        section,
        &opts.judgment,
    );
    let mut judged = Judged { sent: false, finding: None };
    let Ok(request) = request else {
        "too_large".clone_into(&mut pair.outcome);
        report.not_reviewed.push(Unreviewed {
            rel_path: change.rel_path.clone(),
            heading_path: Some(change.change.heading_path.clone()),
            reason: "too_large".to_owned(),
            with: Some(section.rel_path.clone()),
        });
        return Ok(judged);
    };

    let mut last_failure: Option<String> = None;
    for _attempt in 0..2 {
        if let Some(why) = budget_used_up(engine)? {
            report.stopped = Some(why.clone());
            match last_failure {
                // A first attempt that never started: the pair was not judged.
                None => {
                    pair.detail = Some(why);
                    report.not_judged += 1;
                }
                Some(failure) => {
                    "failed".clone_into(&mut pair.outcome);
                    pair.detail = Some(format!("{failure}; not retried: {why}"));
                    report.failed += 1;
                }
            }
            return Ok(judged);
        }
        judged.sent = true;
        let result = match backend.structured(&request, &opts.model).await {
            Ok(result) => result,
            Err(e) => {
                // The call may or may not have been made: its usage is unknown, and said so.
                report.usage_unknown += 1;
                engine.store_mut().record_usage(
                    &section.section_hash,
                    &opts.model,
                    &Usage::default(),
                    "impact_error_usage_unknown",
                )?;
                report.stopped = Some(format!("the backend failed: {e}"));
                "failed".clone_into(&mut pair.outcome);
                pair.detail = Some(e.to_string());
                report.failed += 1;
                return Ok(judged);
            }
        };
        record_attempt(engine, opts, &section.section_hash, &result, report)?;
        let rate_limited = matches!(result.status, StructuredStatus::RateLimited { .. });
        let answer = match result.status {
            StructuredStatus::Ok { value } => judgment::parse_answer(&value),
            StructuredStatus::Fatal { reason } => {
                report.stopped = Some(format!("the backend stopped: {reason}"));
                "failed".clone_into(&mut pair.outcome);
                pair.detail = Some(reason);
                report.failed += 1;
                return Ok(judged);
            }
            StructuredStatus::Malformed { reason, .. }
            | StructuredStatus::Retryable { reason }
            | StructuredStatus::RateLimited { reason } => Err(reason),
        };
        let answer = match answer {
            Ok(answer) => answer,
            Err(why) => {
                last_failure = Some(why);
                if rate_limited {
                    tokio::time::sleep(opts.rate_limit_pause).await;
                }
                continue;
            }
        };
        judged.finding = conclude(engine, change, candidate, &candidate_doc, &answer, pair, report);
        return Ok(judged);
    }
    "failed".clone_into(&mut pair.outcome);
    pair.detail = last_failure;
    report.failed += 1;
    Ok(judged)
}

/// Count an answer and turn it into the pair's outcome; the finding, when one stands.
fn conclude(
    engine: &Engine,
    change: &Changed,
    candidate: &Candidate,
    candidate_doc: &DocContext,
    answer: &judgment::Answer,
    pair: &mut PairOutcome,
    report: &mut Report,
) -> Option<(ReportFinding, String)> {
    let section = &candidate.section;
    report.judged += 1;
    let name = verdict_name(answer.verdict);
    *report.verdicts.entry(name.clone()).or_default() += 1;
    match judgment::assess(answer, &change.change, section) {
        Assessed::NotAlert(_) => pair.outcome = name,
        Assessed::DroppedUnquoted { why, .. } => {
            "dropped_unquoted".clone_into(&mut pair.outcome);
            pair.detail = Some(format!("{name}: {why}"));
            report.dropped_unquoted += 1;
        }
        Assessed::Finding(finding) => {
            let standing = stands_now(engine, change, candidate, candidate_doc, finding);
            if standing.is_some() {
                pair.outcome = name;
            } else {
                "stale_during_review".clone_into(&mut pair.outcome);
                report.stale_during_review += 1;
            }
            return standing;
        }
    }
    None
}

/// Write one attempt to the ledger and add it to the report. The model is the one the
/// backend says ran, when it says. An unknown usage is written as zero and named.
fn record_attempt(
    engine: &mut Engine,
    opts: &Options,
    section_hash: &str,
    result: &crate::worker::StructuredResult,
    report: &mut Report,
) -> Result<()> {
    let kind = result.kind();
    let (spent, outcome, model) = if let Some(u) = &result.usage {
        let spent = Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cost_usd: u.cost_usd,
        };
        let model = if u.model.is_empty() { opts.model.as_str() } else { u.model.as_str() };
        (spent, format!("impact_{kind}"), model)
    } else {
        report.usage_unknown += 1;
        (Usage::default(), format!("impact_{kind}_usage_unknown"), opts.model.as_str())
    };
    report.usage.input_tokens += spent.input_tokens;
    report.usage.output_tokens += spent.output_tokens;
    report.usage.cost_usd += spent.cost_usd;
    engine.store_mut().record_usage(section_hash, model, &spent, &outcome)
}

/// Does a finding stand in the files as they are at this moment? The changed file must read
/// as it did when it was compared; the other file must hold the candidate section under the
/// same heading with the front matter the judge was shown. Its lines are taken from this
/// read. Returns the finding with the other file's text, for the last look in `settle`.
fn stands_now(
    engine: &Engine,
    change: &Changed,
    candidate: &Candidate,
    candidate_doc: &DocContext,
    finding: judgment::Finding,
) -> Option<(ReportFinding, String)> {
    let read = |rel: &str| engine.safe_join(rel).ok().and_then(|p| std::fs::read_to_string(p).ok());
    if read(&change.rel_path).as_deref() != Some(&*change.after_text) {
        return None;
    }
    let section = &candidate.section;
    let now = read(&section.rel_path)?;
    let parsed = markdown::parse_str_as(&now, Flavor::of_path(Path::new(&section.rel_path)));
    if parsed.frontmatter != candidate_doc.frontmatter {
        return None;
    }
    let same = |s: &&markdown::Section| {
        s.hash == section.section_hash && s.heading_path == section.heading_path
    };
    let current = parsed.sections.iter().find(same)?;
    // The section may have moved in its file since the index read it.
    let moved = |line: u32| line.saturating_sub(section.line_start) + current.line_start;
    let candidate_quote = Quote {
        text: finding.candidate.text,
        line_start: moved(finding.candidate.line_start),
        line_end: moved(finding.candidate.line_end),
    };
    let report = ReportFinding {
        verdict: finding.verdict,
        change: section_ref(&change.rel_path, &change.change.heading_path),
        candidate: section_ref(&section.rel_path, &section.heading_path),
        before: finding.before,
        after: finding.after,
        candidate_quote,
        candidate_updated_at: section.updated_at,
        candidate_updated_source: section.updated_source,
        model_reading: finding.model_reading,
    };
    Some((report, now))
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
