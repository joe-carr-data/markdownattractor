//! `mda status` — what is indexed, what is pending, what it cost.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda status`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let counts = engine.store().counts()?;
    let cfg = engine.config();

    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "counts": counts,
            "config": cfg,
            "schema_version": engine.store().schema_version()?,
        }));
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    #[allow(clippy::cast_precision_loss)] // section counts are far below 2^52
    let coverage = if counts.sections == 0 {
        0.0
    } else {
        100.0 * counts.summarized as f64 / counts.sections as f64
    };
    println!(
        "{} {} · {} docs · {} sections · {} carded ({coverage:.0}%) · {} pending · {} failed",
        st.bold("mda"),
        st.dim(&engine.root().display().to_string()),
        counts.docs,
        counts.sections,
        st.ok(&counts.summarized.to_string()),
        if counts.pending > 0 { st.warn(&counts.pending.to_string()) } else { "0".to_owned() },
        if counts.failed > 0 { st.fail(&counts.failed.to_string()) } else { "0".to_owned() },
    );
    println!(
        "  model {} · backend {:?} · concurrency {} · tokens {} in / {} out · ${:.4} · {} tombstoned",
        st.accent(&cfg.summarization_model),
        cfg.backend,
        cfg.concurrency.map_or("auto".to_owned(), |c| c.to_string()),
        counts.total_input_tokens,
        counts.total_output_tokens,
        counts.total_cost_usd,
        counts.tombstoned,
    );
    if counts.pending > 0 {
        println!("  {} `mda index` summarizes pending sections", st.dim("hint:"));
    }
    Ok(ExitCode::SUCCESS)
}
