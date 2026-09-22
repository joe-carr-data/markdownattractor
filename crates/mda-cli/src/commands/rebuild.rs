//! `mda rebuild --embeddings` — regenerate derived artefacts.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda rebuild`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Embed every carded section that has no vector for the current model (downloads the
    /// model on first use).
    #[arg(long)]
    pub embeddings: bool,
    /// Also drop vectors made by other models.
    #[arg(long)]
    pub drop_other_models: bool,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut engine = Engine::open(&root)?;
    let st = Style::auto();
    if !args.embeddings {
        anyhow::bail!("nothing to rebuild: pass --embeddings");
    }
    let Some(embedder) = super::embedder_for(engine.config()) else {
        anyhow::bail!("embeddings are off (`mda embeddings local-small` turns them on)");
    };
    if !embedder.ready() && !json {
        println!("{} downloading the embedding model (~33 MB, once)…", st.dim("note:"));
    }
    let report = engine.embed_pending(&*embedder, usize::MAX)?;
    let dropped = if args.drop_other_models {
        engine.store_mut().delete_embeddings_not(embedder.model())?
    } else {
        0
    };
    if json {
        output::json(&serde_json::json!({ "embeddings": report, "dropped_other_models": dropped }));
    } else {
        println!(
            "{} {} card(s) embedded with {} · {} remaining · {} ms{}",
            st.ok("rebuilt"),
            report.embedded,
            st.accent(&report.model),
            report.remaining,
            report.ms,
            if dropped > 0 {
                format!(" · {dropped} vector(s) of other models dropped")
            } else {
                String::new()
            }
        );
    }
    Ok(ExitCode::SUCCESS)
}
