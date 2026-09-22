//! `mda start [path]` — spawn the daemon for a root and confirm it is up.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use mda_core::config::STATE_DIR;
use mda_core::daemon::{DaemonInfo, is_running};
use mda_core::pipeline::Engine;
use mda_core::worker::backend_for;

use crate::output::{self, Style};

/// Arguments for `mda start`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Folder to watch. Defaults to the nearest indexed ancestor, else the current directory.
    pub path: Option<PathBuf>,
    /// Same as `path`, for symmetry with the other commands.
    #[arg(long, conflicts_with = "path")]
    pub root: Option<PathBuf>,
    /// Run in this terminal instead of the background (logs to stderr; Ctrl-C stops).
    #[arg(long)]
    pub foreground: bool,
}

/// How long `start` waits for the daemon to answer its socket.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.path.as_deref().or(args.root.as_deref()))?;
    let root = root.canonicalize().with_context(|| format!("resolving {}", root.display()))?;
    let st = Style::auto();

    if super::block_on(is_running(&root))? {
        let info = DaemonInfo::read(&root)?;
        if json {
            output::json(&serde_json::json!({ "started": false, "running": true, "info": info }));
        } else {
            let pid = info.map_or("?".to_owned(), |i| i.pid.to_string());
            println!(
                "{} already running · pid {pid} · {}",
                st.ok("mda"),
                st.dim(&root.display().to_string())
            );
            println!("  {} mda status · mda watch · mda stop", st.dim("next:"));
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Creates .markdownattractor/ and the database if this is the first run.
    let engine = Engine::open(&root)?;
    let cfg = engine.config().clone();
    let files = mda_core::walk::discover(&root, &cfg)?.len();
    drop(engine);
    let backend_warning = backend_for(&cfg).err().map(|e| e.to_string());

    if args.foreground {
        if !json {
            println!(
                "{} watching {} · {files} markdown files · Ctrl-C stops",
                st.ok("mda"),
                st.dim(&root.display().to_string())
            );
            if let Some(w) = &backend_warning {
                println!("  {} summarization disabled: {w}", st.warn("warning:"));
            }
        }
        let d = super::daemon::Args { root, log_dir: None, debounce_ms: 1000 };
        return super::daemon::run(&d, json);
    }

    let log_dir = root.join(STATE_DIR).join("logs");
    std::fs::create_dir_all(&log_dir).with_context(|| format!("creating {}", log_dir.display()))?;
    let pid = spawn_detached(&root, &log_dir)?;

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut up = false;
    while Instant::now() < deadline {
        if super::block_on(is_running(&root))? {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !up {
        let tail = tail_of(&log_dir.join("daemon.out"), 8);
        anyhow::bail!(
            "daemon (pid {pid}) did not answer within {}s; last log lines:\n{tail}\n(see {})",
            STARTUP_TIMEOUT.as_secs(),
            log_dir.display()
        );
    }
    let info = DaemonInfo::read(&root)?;
    let pid = info.as_ref().map_or(pid, |i| i.pid);

    if json {
        output::json(&serde_json::json!({
            "started": true,
            "running": true,
            "pid": pid,
            "root": root,
            "files": files,
            "log_dir": log_dir,
            "backend_warning": backend_warning,
            "info": info,
        }));
    } else {
        println!(
            "{} started · pid {pid} · watching {} · {files} markdown file{} · logs in {}",
            st.ok("mda"),
            st.dim(&root.display().to_string()),
            if files == 1 { "" } else { "s" },
            st.dim(&log_dir.display().to_string()),
        );
        if let Some(w) = &backend_warning {
            println!("  {} summarization disabled: {w}", st.warn("warning:"));
            println!(
                "  {} raw search works now; fix the backend (`mda doctor`) and run `mda restart`",
                st.dim("hint:")
            );
        } else {
            println!(
                "  {} raw search is live as files are parsed; cards follow within seconds",
                st.dim("hint:")
            );
        }
        println!("  {} mda status · mda watch · mda search \"…\"", st.dim("next:"));
        let _ = std::io::stdout().flush();
    }
    Ok(ExitCode::SUCCESS)
}

/// Spawn `mda daemon` detached from this terminal, logging to `log_dir`.
fn spawn_detached(root: &Path, log_dir: &Path) -> anyhow::Result<u32> {
    let exe = std::env::current_exe().context("locating the mda binary")?;
    let out_path = log_dir.join("daemon.out");
    let out = std::fs::OpenOptions::new().create(true).append(true).open(&out_path)?;
    let err = out.try_clone()?;
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .arg("--root")
        .arg(root)
        .arg("--log-dir")
        .arg(log_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .current_dir(root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group: closing the terminal or Ctrl-C here never reaches the daemon.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd.spawn().context("spawning the daemon")?;
    Ok(child.id())
}

fn tail_of(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}
