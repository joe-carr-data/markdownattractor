//! `mda pause` and `mda resume` — toggle summarization without stopping the watcher.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::daemon::{Client, Request, Response};

use crate::output::{self, Style};

/// Arguments for `mda pause` / `mda resume`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run `mda pause`.
pub fn pause(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    toggle(
        args,
        json,
        &Request::Pause,
        "paused · indexing continues, no model calls until `mda resume`",
    )
}

/// Run `mda resume`.
pub fn resume(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    toggle(args, json, &Request::Resume, "resumed")
}

fn toggle(args: &Args, json: bool, req: &Request, line: &str) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let resp = super::block_on(async {
        let mut c = Client::connect(&root).await?;
        c.request(req).await
    })?;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => anyhow::bail!("{e}\n  hint: is the daemon running? `mda start`"),
    };
    match resp {
        Response::Ok => {
            if json {
                output::json(&serde_json::json!({ "ok": true, "op": req }));
            } else {
                println!("{} {line}", Style::auto().ok("mda"));
            }
            Ok(ExitCode::SUCCESS)
        }
        Response::Error { message } => anyhow::bail!("{message}"),
        other => anyhow::bail!("unexpected answer from the daemon: {other:?}"),
    }
}
