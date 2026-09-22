//! `mda daemon --root <root>` — the daemon process. Hidden: `mda start` spawns it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use mda_core::config::Config;
use mda_core::daemon::{self, DaemonConfig};
use mda_core::worker::{DynBackend, Unavailable, backend_for};
use tokio_util::sync::CancellationToken;

/// Arguments for `mda daemon`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: PathBuf,
    /// Write logs to `daemon.log` in this directory instead of stderr.
    #[arg(long)]
    pub log_dir: Option<PathBuf>,
    /// Quiet period before a changed file is indexed, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    pub debounce_ms: u64,
}

/// Run the daemon until Ctrl-C, SIGTERM, or `mda stop`.
pub fn run(args: &Args, _json: bool) -> anyhow::Result<ExitCode> {
    let root = args.root.canonicalize()?;
    let cfg = Config::load(&root)?;
    let backend = backend_or_unavailable(&cfg);
    let dcfg = DaemonConfig {
        debounce: Duration::from_millis(args.debounce_ms),
        ..DaemonConfig::default()
    };

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let cancel = CancellationToken::new();
    rt.spawn(stop_on_signal(cancel.clone()));
    rt.block_on(daemon::run(&root, backend, dcfg, cancel))?;
    Ok(ExitCode::SUCCESS)
}

/// The configured backend, or a stand-in that fails every call and says why, so the daemon
/// still watches and keeps raw search live when summarization cannot run.
pub fn backend_or_unavailable(cfg: &Config) -> DynBackend {
    match backend_for(cfg) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "summarization backend unavailable; indexing only");
            Arc::new(Unavailable::new(e.to_string()))
        }
    }
}

async fn stop_on_signal(cancel: CancellationToken) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "cannot listen for SIGTERM");
                let _ = tokio::signal::ctrl_c().await;
                cancel.cancel();
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("signal received");
    cancel.cancel();
}
