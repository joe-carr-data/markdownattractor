//! `mda open <section_id>` — the exact source lines of a section, re-checked at read time.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda open`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Section id from a search hit, e.g. `1f3a9c0b2d4e5f60#2`.
    pub section_id: String,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut engine = Engine::open(&root)?;
    let opened = engine.open_section(&args.section_id)?;

    if json {
        output::json(&opened);
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    let heading = if opened.heading_path.is_empty() {
        String::new()
    } else {
        opened.heading_path.join(" › ")
    };
    println!(
        "{} {} {}{}",
        st.bold(&opened.rel_path),
        st.accent(&format!("L{}–{}", opened.line_start, opened.line_end)),
        st.dim(&heading),
        if opened.stale {
            st.warn("  (file changed since indexing; showing current lines)")
        } else {
            String::new()
        }
    );
    let width = opened.line_end.to_string().len();
    for (i, line) in opened.text.lines().enumerate() {
        println!("{} │ {line}", st.dim(&format!("{:>width$}", opened.line_start as usize + i)));
    }
    Ok(ExitCode::SUCCESS)
}
