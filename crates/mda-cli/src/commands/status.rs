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
    let live = super::live_status(engine.root());
    let mut embed_check = mda_core::embed::check(cfg);
    // The id the vectors are stored under carries the embedding-text variant.
    if let Some(e) = mda_core::embed::embedder_for(cfg) {
        embed_check.model = Some(e.model().to_owned());
    }
    let embed_counts = match &embed_check.model {
        Some(m) => Some(engine.store().embedding_counts(m)?),
        None => None,
    };

    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "counts": counts,
            "config": cfg,
            "schema_version": engine.store().schema_version()?,
            "daemon": live,
            "embeddings": { "check": embed_check, "counts": embed_counts },
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
    match (&embed_check.model, embed_counts) {
        (Some(m), Some(c)) => println!(
            "  embeddings {} · {} of {} cards embedded · model {}",
            if embed_check.cached { st.ok("ready") } else { st.warn("not downloaded") },
            c.embedded,
            c.carded,
            st.accent(m),
        ),
        _ => println!("  embeddings {} · search is lexical only", st.dim("off")),
    }
    if let Some(l) = &live {
        let state = if l.paused {
            st.warn("paused")
        } else if l.backoff_secs > 0 {
            st.warn(&format!("backing off {}s", l.backoff_secs))
        } else if l.watching {
            st.ok("watching")
        } else {
            st.fail("watcher down")
        };
        println!(
            "  daemon {} · pid {} · up {} · {state} · {} synced · {} renamed · {} cards · {} failed · ${:.4} this run",
            st.ok("running"),
            l.pid,
            super::humanize_secs(l.uptime_secs),
            l.synced,
            l.renamed,
            l.cards,
            l.failures,
            l.cost_usd,
        );
        if let Some(e) = &l.last_error {
            println!("  {} {e}", st.warn("last error:"));
        }
    } else {
        println!("  daemon {} · `mda start` keeps the index live", st.dim("not running"));
        if counts.pending > 0 {
            println!("  {} `mda index` summarizes pending sections now", st.dim("hint:"));
        }
    }
    Ok(ExitCode::SUCCESS)
}
