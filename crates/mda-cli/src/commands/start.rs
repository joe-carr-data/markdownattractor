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
    /// On a first run, do not wait for the first cards to show an example query. The
    /// `SessionStart` hook passes this; interactive users get the example.
    #[arg(long)]
    pub no_example: bool,
    /// How long a first run may wait for the first cards before giving up on the example.
    #[arg(long, default_value_t = 60, hide = true)]
    pub example_timeout: u64,
}

/// How long `start` waits for the daemon to answer its socket.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
/// Cards to wait for before running the example query (or every section when fewer).
const EXAMPLE_CARDS: u64 = 10;

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.path.as_deref().or(args.root.as_deref()))?;
    let root = root.canonicalize().with_context(|| format!("resolving {}", root.display()))?;
    let st = Style::auto();

    if super::block_on(is_running(&root))? {
        return report_already_running(&root, json, &st);
    }

    // Creates .markdownattractor/ and the database if this is the first run.
    let engine = Engine::open(&root)?;
    let cfg = engine.config().clone();
    let files = mda_core::walk::discover(&root, &cfg)?.len();
    let carded_before = engine.store().counts()?.summarized;
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

    // First run: show one real hit before the user walks away (plan §9.5).
    let (example, example_skipped) =
        first_run_example(args, &root, backend_warning.as_deref(), carded_before, files);

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
            "example": example,
            "example_skipped": example_skipped,
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
        match (&example, &example_skipped) {
            (Some(ex), _) => print_example(ex, &st),
            (None, Some(reason)) if !args.no_example && backend_warning.is_none() => {
                println!("  {} example query skipped: {reason}", st.dim("note:"));
            }
            _ => {}
        }
        println!("  {} mda status · mda watch · mda search \"…\"", st.dim("next:"));
        let _ = std::io::stdout().flush();
    }
    Ok(ExitCode::SUCCESS)
}

/// Decide whether this start gets an example query and, if so, wait for it. Returns the
/// example or the reason it was skipped.
fn first_run_example(
    args: &Args,
    root: &Path,
    backend_warning: Option<&str>,
    carded_before: u64,
    files: usize,
) -> (Option<mda_core::pipeline::Example>, Option<String>) {
    let skip = if args.no_example {
        Some("--no-example")
    } else if backend_warning.is_some() {
        Some("summarization is disabled")
    } else if carded_before > 0 {
        Some("this root already had cards")
    } else if files == 0 {
        Some("no markdown files")
    } else {
        None
    };
    if let Some(reason) = skip {
        return (None, Some(reason.to_owned()));
    }
    match wait_for_example(root, Duration::from_secs(args.example_timeout)) {
        Ok(Some(ex)) => (Some(ex), None),
        Ok(None) => (None, Some("no card carries a question yet".to_owned())),
        Err(reason) => (None, Some(reason)),
    }
}

/// Poll the store until the first cards land (or `timeout`), then pick a question from one
/// of them and search for it. `Err` carries the reason when nothing could be shown.
fn wait_for_example(
    root: &Path,
    timeout: Duration,
) -> std::result::Result<Option<mda_core::pipeline::Example>, String> {
    let deadline = Instant::now() + timeout;
    let engine = Engine::open(root).map_err(|e| e.to_string())?;
    loop {
        let counts = engine.store().counts().map_err(|e| e.to_string())?;
        let target = EXAMPLE_CARDS.min(counts.sections.max(1));
        if counts.sections > 0 && counts.summarized >= target {
            return engine.example().map_err(|e| e.to_string());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no cards after {}s ({} of {} sections carded); `mda status` shows progress",
                timeout.as_secs(),
                counts.summarized,
                counts.sections
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn print_example(ex: &mda_core::pipeline::Example, st: &Style) {
    println!("  {} mda search {:?}", st.bold("example:"), ex.query);
    match &ex.hit {
        Some(h) => {
            let heading = if h.heading_path.is_empty() {
                "(preamble)".to_owned()
            } else {
                h.heading_path.join(" › ")
            };
            println!(
                "    {} › {}   {} · {}",
                st.bold(&h.rel_path),
                heading,
                st.accent(&format!("L{}–{}", h.line_start, h.line_end)),
                super::humanize_age(h.updated_at),
            );
            println!("    {}", h.tldr.as_deref().unwrap_or(&h.snippet));
            println!("    {}", st.dim(&format!("mda open {}", h.section_id)));
        }
        None => println!("    (no lexical hit for that question; try `mda search` with vectors)"),
    }
}

fn report_already_running(root: &Path, json: bool, st: &Style) -> anyhow::Result<ExitCode> {
    let info = DaemonInfo::read(root)?;
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
        make_std_handles_non_inheritable();
    }
    let child = cmd.spawn().context("spawning the daemon")?;
    Ok(child.id())
}

/// Windows spawns children with `bInheritHandles = TRUE`, so every inheritable handle of this
/// process, including the stdout pipe a shell or a test harness gave *us*, would live on in
/// the daemon even though its own stdio is redirected to files. Whoever captures `mda start`'s
/// output then never sees EOF and waits for the daemon to exit. Clearing the inherit flag on
/// our three standard handles before spawning is the fix Microsoft documents (KB 315939).
#[cfg(windows)]
fn make_std_handles_non_inheritable() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: both calls take plain integers and a handle this process owns; clearing the
        // inherit flag on our own standard handles cannot touch memory this program manages,
        // and a missing or invalid handle is skipped. Failure is harmless (the spawn still
        // works, only the EOF problem above remains), so the result is ignored.
        #[allow(unsafe_code)]
        unsafe {
            let h = GetStdHandle(id);
            if h.is_null() || h == INVALID_HANDLE_VALUE {
                continue;
            }
            let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

fn tail_of(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}
