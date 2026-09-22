//! `mda stale` — documents whose index is not final. Should print nothing while the daemon
//! keeps up.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda stale`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command. Exit code 1 when anything is stale, so scripts can use it as a check.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let stale = engine.stale()?;
    if json {
        output::json(&serde_json::json!({ "stale": stale }));
        return Ok(if stale.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE });
    }
    let st = Style::auto();
    if stale.is_empty() {
        println!(
            "{} nothing stale: every section has a card and every file matches its index",
            st.ok("mda")
        );
        return Ok(ExitCode::SUCCESS);
    }
    for d in &stale {
        let mut why = Vec::new();
        if d.pending > 0 {
            why.push(format!("{} pending", d.pending));
        }
        if d.failed > 0 {
            why.push(st.fail(&format!("{} failed", d.failed)));
        }
        if d.changed_on_disk {
            why.push(st.warn("changed on disk"));
        }
        if d.missing {
            why.push(st.warn("file missing"));
        }
        println!("{} · {}", st.bold(&d.rel_path), why.join(" · "));
    }
    println!("  {} `mda index` (or a running daemon) brings these up to date", st.dim("hint:"));
    Ok(ExitCode::FAILURE)
}
