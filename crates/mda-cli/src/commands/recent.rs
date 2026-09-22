//! `mda recent [n]` — the most recently updated documents.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda recent`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// How many documents.
    #[arg(default_value_t = 10)]
    pub n: usize,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let docs = engine.recent(args.n)?;
    if json {
        output::json(&serde_json::json!({ "docs": docs }));
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    if docs.is_empty() {
        println!("{} nothing indexed yet", st.bold("recent"));
        return Ok(ExitCode::SUCCESS);
    }
    for d in &docs {
        let pending = if d.pending > 0 {
            st.warn(&format!(" · {} pending", d.pending))
        } else {
            String::new()
        };
        println!(
            "{:<12} {} · {} section{}{} · {}",
            super::humanize_age(d.updated_at),
            st.bold(&d.rel_path),
            d.sections,
            if d.sections == 1 { "" } else { "s" },
            pending,
            st.dim(d.title.as_deref().unwrap_or("")),
        );
    }
    Ok(ExitCode::SUCCESS)
}
