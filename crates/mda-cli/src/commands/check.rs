//! `mda check [--since <revision>]` — after a change, which other sections are possibly out
//! of date? Read-only: it names passages and shows them; it never says which is right and
//! never edits anything (ADR-0009).

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::impact::review::{Options, Report, ReportFinding, review};
use mda_core::pipeline::Engine;
use mda_core::worker::backend_for;

use crate::output::{self, Style};

/// Arguments for `mda check`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Compare the working files with this git revision (a commit of the current branch).
    #[arg(long, default_value = "HEAD")]
    pub since: String,
    /// Most pairs sent to the model; the rest are listed as not judged.
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u64).range(0..=1000))]
    pub max_judgments: u64,
    /// Also list every pair that was looked at, with its outcome.
    #[arg(long)]
    pub all: bool,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command. Exit code 0 for a completed review, with or without findings: a finding
/// is a pointer to two passages, not a failure.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut engine = Engine::open(&root)?;
    let cfg = engine.config().clone();
    // No backend is not an error: candidates are listed and nothing is judged. Why there is
    // none is said, so that a broken setup does not read as "not configured".
    let (backend, backend_note) = match backend_for(&cfg) {
        Ok(b) => (Some(b), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let opts = Options {
        since: args.since.clone(),
        max_judgments: usize::try_from(args.max_judgments).unwrap_or(usize::MAX),
        ..Options::new(cfg.summarization_model.clone())
    };
    let mut report = super::block_on(review(&mut engine, backend.as_deref(), &opts))??;
    report.backend_note = backend_note;
    if json {
        output::json(&report);
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    for finding in &report.findings {
        print_finding(finding, &st);
    }
    // Without a backend the candidates are the result: they are always listed.
    if args.all || report.backend.is_none() {
        for p in &report.pairs {
            let detail = p.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default();
            println!(
                "  {} {} › {}  ↔  {} › {}{}",
                st.dim(&format!("{:<19}", p.outcome)),
                p.change.rel_path,
                p.change.heading_path.join(" › "),
                p.candidate.rel_path,
                p.candidate.heading_path.join(" › "),
                st.dim(&detail)
            );
        }
    }
    println!("{}", summary(&report, &st));
    for line in coverage(&report) {
        println!("  {} {line}", st.dim("not covered:"));
    }
    Ok(ExitCode::SUCCESS)
}

fn print_finding(f: &ReportFinding, st: &Style) {
    let label = match f.verdict {
        mda_core::impact::judgment::Verdict::PossibleConflict => "possible conflict",
        _ => "possibly out of date",
    };
    let lines = |a: u32, b: u32| if a == b { format!("{a}") } else { format!("{a}-{b}") };
    println!(
        "{} {}:{} › {}",
        st.warn(label),
        st.bold(&f.candidate.rel_path),
        lines(f.candidate_quote.line_start, f.candidate_quote.line_end),
        f.candidate.heading_path.join(" › ")
    );
    println!("    it says:      {}", quoted(&f.candidate_quote.text));
    println!(
        "    {}:{} now says: {}",
        f.change.rel_path,
        lines(f.after.line_start, f.after.line_end),
        quoted(&f.after.text)
    );
    if let Some(before) = &f.before {
        println!("    and said before: {}", quoted(&before.text));
    }
    println!(
        "    {}",
        st.dim(&format!(
            "index last changed {} on {} ({})",
            f.candidate.rel_path,
            f.candidate_updated_at.strftime("%Y-%m-%d"),
            f.candidate_updated_source.as_str()
        ))
    );
    if let Some(reading) = &f.model_reading {
        println!("    {}", st.dim(&format!("the model's reading: {reading}")));
    }
}

/// A quote on one line, in quotation marks. The text is the file's own, spaces included; a
/// line break inside it is shown as `⏎` (`--json` has the exact text).
fn quoted(text: &str) -> String {
    format!("“{}”", text.replace('\n', " ⏎ "))
}

/// The one-line result.
fn summary(r: &Report, st: &Style) -> String {
    let short = r.revision.get(..10).unwrap_or(&r.revision);
    let head = match (r.findings.len(), r.backend.is_some()) {
        (0, true) => st.ok("mda check"),
        (_, true) => st.warn("mda check"),
        (_, false) => st.dim("mda check"),
    };
    let found = if r.backend.is_some() {
        format!("{} possibly out of date", r.findings.len())
    } else {
        match &r.backend_note {
            Some(why) => format!("not judged (no summarization backend: {why})"),
            None => "not judged (no summarization backend)".to_owned(),
        }
    };
    format!(
        "{head} since {short} · {} changed section(s) in {} file(s) · {} candidate pair(s) · {} judged · {found}",
        r.changes, r.files_changed, r.candidates, r.judged
    )
}

/// What the review did not cover, one line per kind. Missing coverage is never shown as "no
/// findings".
fn coverage(r: &Report) -> Vec<String> {
    let mut lines = Vec::new();
    let mut by_reason: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for n in &r.not_reviewed {
        *by_reason.entry(n.reason.as_str()).or_default() += 1;
    }
    for (reason, n) in by_reason {
        lines.push(format!("{n} × {} (see --json for the list)", reason.replace('_', " ")));
    }
    if r.not_judged > 0 && r.backend.is_some() {
        let why = r.stopped.as_deref().unwrap_or("above --max-judgments");
        lines.push(format!("{} pair(s) not judged: {why}", r.not_judged));
    }
    if r.failed > 0 {
        lines.push(format!("{} pair(s) whose call failed", r.failed));
    }
    if r.dropped_unquoted > 0 {
        lines.push(format!(
            "{} alert(s) dropped: their quotes were not found in the files",
            r.dropped_unquoted
        ));
    }
    if r.stale_during_review > 0 {
        lines.push(format!(
            "{} finding(s) withheld: a file changed during the review",
            r.stale_during_review
        ));
    }
    if !r.changes_without_candidates.is_empty() {
        lines.push(format!(
            "{} changed section(s) with no candidate: nothing else shares their wording",
            r.changes_without_candidates.len()
        ));
    }
    if r.cut_per_document > 0 {
        lines.push(format!(
            "{} section(s) passed over: their document already had two candidates",
            r.cut_per_document
        ));
    }
    if r.searches_truncated > 0 {
        lines.push(format!(
            "{} change(s) whose candidate search was cut short",
            r.searches_truncated
        ));
    }
    lines
}
