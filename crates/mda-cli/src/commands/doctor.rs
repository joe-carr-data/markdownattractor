//! `mda doctor` — check the environment and print fixes. Never modifies anything.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use serde::Serialize;

use crate::output::{self, Style};

/// Arguments for `mda doctor`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Watched root to check (defaults to the current directory).
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
struct Check {
    name: &'static str,
    status: Status,
    detail: String,
    fix: Option<&'static str>,
}

/// Run the command. Exit code is non-zero when any check fails.
#[expect(clippy::unnecessary_wraps, reason = "every command shares the same signature")]
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let checks = vec![check_claude_cli(), check_root(&args.root), check_state_dir(&args.root)];
    let failed = checks.iter().any(|c| matches!(c.status, Status::Fail));

    if json {
        output::json(&serde_json::json!({ "ok": !failed, "checks": checks }));
    } else {
        let st = Style::auto();
        for c in &checks {
            let mark = match c.status {
                Status::Ok => st.ok("✓"),
                Status::Warn => st.warn("!"),
                Status::Fail => st.fail("✗"),
            };
            println!("{mark} {:<14} {}", c.name, c.detail);
            if let Some(fix) = c.fix {
                println!("  {} {fix}", st.dim("fix:"));
            }
        }
        println!();
        if failed {
            println!("{}", st.fail("mda is not ready."));
        } else {
            println!("{}", st.ok("mda is ready."));
        }
    }
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn check_claude_cli() -> Check {
    match Command::new("claude").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            Check { name: "claude", status: Status::Ok, detail: version, fix: None }
        }
        Ok(out) => Check {
            name: "claude",
            status: Status::Fail,
            detail: format!("`claude --version` exited with {}", out.status),
            fix: Some("reinstall Claude Code: https://code.claude.com/docs/en/setup"),
        },
        Err(e) => Check {
            name: "claude",
            status: Status::Fail,
            detail: format!("not found on PATH ({e})"),
            fix: Some("install Claude Code and make sure `claude` is on PATH"),
        },
    }
}

fn check_root(root: &std::path::Path) -> Check {
    match std::fs::metadata(root) {
        Ok(m) if m.is_dir() => {
            let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            Check {
                name: "root",
                status: Status::Ok,
                detail: canon.display().to_string(),
                fix: None,
            }
        }
        _ => Check {
            name: "root",
            status: Status::Fail,
            detail: format!("{} is not a directory", root.display()),
            fix: Some("pass --root <dir> or run from inside the folder to index"),
        },
    }
}

fn check_state_dir(root: &std::path::Path) -> Check {
    let dir = root.join(mda_core::config::STATE_DIR);
    if dir.exists() {
        match mda_core::config::Config::load(root) {
            Ok(_) => Check {
                name: "state dir",
                status: Status::Ok,
                detail: format!("{} (config ok)", dir.display()),
                fix: None,
            },
            Err(e) => Check {
                name: "state dir",
                status: Status::Fail,
                detail: format!("{e}"),
                fix: Some("fix or delete .markdownattractor/config.toml"),
            },
        }
    } else {
        Check {
            name: "state dir",
            status: Status::Warn,
            detail: "not initialised yet".to_owned(),
            fix: Some("run `mda start` (or `/mda start` in Claude Code)"),
        }
    }
}
