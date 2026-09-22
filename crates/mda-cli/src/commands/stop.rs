//! `mda stop` and `mda restart`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use mda_core::daemon::{Client, DaemonInfo, Request, Response, is_running, pid_path};

use crate::output::{self, Style};

/// Arguments for `mda stop` / `mda restart`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// How long `stop` waits for the daemon to go away after acknowledging.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// Run `mda stop`. Idempotent: stopping a daemon that is not running succeeds.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let st = Style::auto();
    let info = DaemonInfo::read(&root)?;

    if !super::block_on(is_running(&root))? {
        let stale = pid_path(&root).exists();
        if stale {
            // A crashed daemon leaves its files behind; a dead socket means it is not there.
            DaemonInfo::remove(&root);
        }
        if json {
            output::json(
                &serde_json::json!({ "stopped": false, "running": false, "stale_files_removed": stale, "info": info }),
            );
        } else {
            println!("{} not running", st.bold("mda"));
            if let Some(i) = info {
                println!(
                    "  {} pid {} left stale files behind; removed. If that process is still alive, kill it by hand.",
                    st.dim("note:"),
                    i.pid
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    let resp = super::block_on(async {
        let mut c = Client::connect(&root).await?;
        c.request(&Request::Stop).await
    })??;
    if let Response::Error { message } = resp {
        anyhow::bail!("daemon refused to stop: {message}");
    }
    // The socket closes first; the pid file goes last, after in-flight work is recorded.
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !pid_path(&root).exists() && !super::block_on(is_running(&root))? {
            let pid = info.as_ref().map_or("?".to_owned(), |i| i.pid.to_string());
            if json {
                output::json(
                    &serde_json::json!({ "stopped": true, "running": false, "pid": info.as_ref().map(|i| i.pid) }),
                );
            } else {
                println!(
                    "{} stopped · pid {pid} · unfinished sections stay pending for the next start",
                    st.ok("mda")
                );
            }
            return Ok(ExitCode::SUCCESS);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!(
        "daemon acknowledged but is still answering after {}s (pid {}); in-flight model calls may be finishing",
        STOP_TIMEOUT.as_secs(),
        info.map_or("?".to_owned(), |i| i.pid.to_string())
    )
}

/// Run `mda restart`: stop if running, then start.
pub fn restart(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    if super::block_on(is_running(&root))? {
        let code = run(args, json)?;
        if code != ExitCode::SUCCESS {
            return Ok(code);
        }
    }
    let start = super::start::Args {
        path: None,
        root: Some(root),
        foreground: false,
        no_example: true,
        example_timeout: 0,
    };
    super::start::run(&start, json)
}
