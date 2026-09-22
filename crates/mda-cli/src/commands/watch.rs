//! `mda watch` — stream the daemon's events until Ctrl-C.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::daemon::{Client, DaemonEvent, Request, Response};

use crate::output::{self, Style};

/// Arguments for `mda watch`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Exit after this many events (for scripts and tests).
    #[arg(long)]
    pub count: Option<usize>,
}

/// Run the command. With `--json`, one JSON object per line.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let st = Style::auto();
    let count = args.count;
    super::block_on(async move {
        let mut c = match Client::connect(&root).await {
            Ok(c) => c,
            Err(e) => anyhow::bail!("{e}\n  hint: is the daemon running? `mda start`"),
        };
        match c.request(&Request::Watch).await? {
            Response::Ok => {}
            Response::Error { message } => anyhow::bail!("{message}"),
            other => anyhow::bail!("unexpected answer from the daemon: {other:?}"),
        }
        if !json {
            println!(
                "{} watching {} · Ctrl-C to stop",
                st.bold("mda"),
                st.dim(&root.display().to_string())
            );
        }
        let mut seen = 0usize;
        loop {
            let next = tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                r = c.next_response() => r?,
            };
            let Some(resp) = next else { break };
            if let Response::Event(ev) = resp {
                if json {
                    output::json_line(&ev);
                } else {
                    render(&ev, &st);
                }
                seen += 1;
                if matches!(ev, DaemonEvent::Stopping) || count.is_some_and(|n| seen >= n) {
                    break;
                }
            }
        }
        let _ = std::io::stdout().flush();
        Ok(ExitCode::SUCCESS)
    })?
}

fn render(ev: &DaemonEvent, st: &Style) {
    let line = match ev {
        DaemonEvent::Started { root } => format!("{} {root}", st.ok("started")),
        DaemonEvent::Indexed { rel_path, sections, new_hashes, created } => format!(
            "{} {rel_path} · {sections} section{} · {new_hashes} need a card{}",
            st.accent("indexed"),
            if *sections == 1 { "" } else { "s" },
            if *created { " · new" } else { "" }
        ),
        DaemonEvent::Tombstoned { rel_path } => format!("{} {rel_path}", st.warn("removed")),
        DaemonEvent::Renamed { from, to } => format!("{} {from} → {to}", st.accent("renamed")),
        DaemonEvent::Rescanned { files, changed, tombstoned } => {
            format!(
                "{} {files} files · {changed} changed · {tombstoned} removed",
                st.accent("rescanned")
            )
        }
        DaemonEvent::RoundStarted { pending, hot } => {
            format!("{} {pending} pending · {hot} hot", st.dim("round"))
        }
        DaemonEvent::Progress(p) => {
            let last: String = p.last.as_deref().unwrap_or("").chars().take(70).collect();
            format!(
                "{} {}/{} · {} ok · {} failed · {last}",
                st.dim("  ·"),
                p.done,
                p.total,
                p.ok,
                p.failed
            )
        }
        DaemonEvent::RoundFinished { ok, deterministic, failed, deferred, cost_usd } => format!(
            "{} {ok} card{} (+{deterministic} heading-only) · {failed} failed · {deferred} deferred · ${cost_usd:.4}",
            st.ok("summarized"),
            if *ok == 1 { "" } else { "s" }
        ),
        DaemonEvent::Embedded { count, remaining, ms } => {
            format!("{} {count} card(s) · {remaining} remaining · {ms} ms", st.accent("embedded"))
        }
        DaemonEvent::Backoff { secs, reason } => {
            format!("{} {secs}s · {reason}", st.warn("backoff"))
        }
        DaemonEvent::Paused => st.warn("paused"),
        DaemonEvent::Resumed => st.ok("resumed"),
        DaemonEvent::Error { message } => format!("{} {message}", st.fail("error")),
        DaemonEvent::Stopping => st.warn("stopping"),
    };
    println!("{line}");
}
