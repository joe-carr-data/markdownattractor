//! `mda timeline` — what was created, changed, renamed or deleted, grouped by day.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;
use mda_core::store::EventKind;

use crate::output::{self, Style};

/// Arguments for `mda timeline`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Start of the window: `30d`, `2026-09-01`.
    #[arg(long, default_value = "30d")]
    pub since: String,
    /// End of the window.
    #[arg(long)]
    pub until: Option<String>,
    /// Only documents under this path.
    #[arg(long = "in")]
    pub path_prefix: Option<String>,
    /// Maximum entries.
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let since = Some(super::parse_time(&args.since)?);
    let until = args.until.as_deref().map(super::parse_time).transpose()?;
    let entries = engine.timeline(since, until, args.path_prefix.as_deref(), args.limit)?;

    if json {
        output::json(&serde_json::json!({ "entries": entries }));
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    if entries.is_empty() {
        println!("{} nothing since {}", st.bold("timeline"), args.since);
        return Ok(ExitCode::SUCCESS);
    }
    let mut day = String::new();
    for e in &entries {
        let d = e.at.to_string();
        let d = d.get(..10).unwrap_or(&d).to_owned();
        if d != day {
            println!("{}", st.bold(&d));
            day = d;
        }
        let what = match e.kind {
            EventKind::DocCreated => st.ok("created"),
            EventKind::DocChanged => st.accent("changed"),
            EventKind::DocDeleted => st.warn("deleted"),
            EventKind::DocRenamed => st.accent("renamed"),
            EventKind::SectionSummarized => st.dim("carded"),
            EventKind::SectionFailed => st.fail("failed"),
        };
        let detail = match (&e.kind, &e.detail) {
            (EventKind::DocRenamed, Some(from)) => format!(" (from {from})"),
            (_, Some(d)) => format!(" ({d})"),
            _ => String::new(),
        };
        let section = e.section_id.as_deref().map(|s| format!(" {s}")).unwrap_or_default();
        println!(
            "  {} {what:<9} {}{section}{detail}",
            st.dim(&e.at.to_string()[11..19]),
            e.rel_path
        );
    }
    Ok(ExitCode::SUCCESS)
}
