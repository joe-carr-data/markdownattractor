//! `mda embeddings [local-small|off]` — show or switch the embedding setting.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::ValueEnum;
use mda_core::config::{Config, Embeddings};

use crate::output::{self, Style};

/// Choices, as typed on the command line.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Choice {
    /// bge-small-en-v1.5 (quantised), local, ~33 MB. Default.
    LocalSmall,
    /// No vectors; nothing is downloaded.
    Off,
}

/// Arguments for `mda embeddings`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Setting to switch to. Omit to show the current one.
    #[arg(value_enum)]
    pub setting: Option<Choice>,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut cfg = Config::load(&root)?;
    let st = Style::auto();
    if let Some(choice) = args.setting {
        cfg.embeddings = match choice {
            Choice::LocalSmall => Embeddings::LocalSmall,
            Choice::Off => Embeddings::Off,
        };
        cfg.save(&root)?;
    }
    let check = mda_core::embed::check(&cfg);
    // A running daemon or MCP server keeps the embedder it started with.
    let running = args.setting.is_some()
        && super::block_on(mda_core::daemon::is_running(&root)).unwrap_or(false);
    if json {
        output::json(&serde_json::json!({
            "embeddings": cfg.embeddings.as_str(),
            "check": check,
            "saved": args.setting.is_some(),
            "needs_restart": running,
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let verb = if args.setting.is_some() { "embeddings set to" } else { "embeddings" };
    match check.model {
        Some(m) => println!(
            "{} {} · model {} · cache {} · {}",
            st.ok(verb),
            st.accent(cfg.embeddings.as_str()),
            m,
            st.dim(&check.cache_dir.display().to_string()),
            if check.cached {
                "downloaded"
            } else {
                "not downloaded yet (first `mda index` or `mda rebuild --embeddings` fetches it)"
            }
        ),
        None if cfg.embeddings == Embeddings::Off => {
            println!("{} {} · search is lexical only", st.ok(verb), st.accent("off"));
        }
        None => println!(
            "{} {} · this binary was built without the `embeddings` feature; search is lexical only",
            st.ok(verb),
            st.accent(cfg.embeddings.as_str())
        ),
    }
    if running {
        println!("  {} the daemon keeps its current setting until `mda restart`", st.warn("note:"));
    }
    Ok(ExitCode::SUCCESS)
}
