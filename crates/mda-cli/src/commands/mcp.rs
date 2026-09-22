//! `mda mcp` — serve the index to Claude over MCP on stdin/stdout.

use std::path::PathBuf;
use std::process::ExitCode;

/// Arguments for `mda mcp`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root. Defaults to `$MDA_ROOT`, then the nearest indexed ancestor of the
    /// working directory.
    #[arg(long, env = "MDA_ROOT")]
    pub root: Option<PathBuf>,
}

/// Run the server until the client disconnects. stdout is the protocol channel: nothing
/// else may print there (logs go to stderr).
pub fn run(args: &Args, _json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let root = root.canonicalize()?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(mda_core::mcp::serve_stdio(&root))?;
    Ok(ExitCode::SUCCESS)
}
