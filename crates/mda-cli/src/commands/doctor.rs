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
    let cfg = mda_core::config::Config::load(&args.root).unwrap_or_default();
    let mut checks = vec![
        check_root(&args.root),
        check_state_dir(&args.root),
        check_backend(&cfg),
        check_embeddings(&cfg),
        check_daemon(&args.root),
    ];
    if matches!(cfg.backend, mda_core::config::Backend::ClaudeCli) {
        checks.push(check_claude_cli());
    }
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

fn check_backend(cfg: &mda_core::config::Config) -> Check {
    use mda_core::config::Backend;
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            return Check {
                name: "backend",
                status: Status::Fail,
                detail: format!("cannot start runtime: {e}"),
                fix: None,
            };
        }
    };
    match cfg.backend {
        Backend::Api => {
            if !std::env::var(&cfg.api_key_env).is_ok_and(|v| !v.trim().is_empty()) {
                return Check {
                    name: "backend",
                    status: Status::Fail,
                    detail: format!("api: ${} is not set", cfg.api_key_env),
                    fix: Some(
                        "export ANTHROPIC_API_KEY=… (console.anthropic.com), or `mda backend local` to use a local model",
                    ),
                };
            }
            match rt.block_on(mda_core::worker::api::check(cfg)) {
                Ok(model) => Check {
                    name: "backend",
                    status: Status::Ok,
                    detail: format!("api: {} reachable, model {model}", cfg.api_base_url),
                    fix: None,
                },
                Err(e) => {
                    let msg = e.to_string();
                    let fix = if msg.contains("anthropic-workspace-id") {
                        "this key is not scoped to a workspace: set `api_workspace_id = \"wrkspc_…\"` in .markdownattractor/config.toml (or export ANTHROPIC_WORKSPACE_ID); the id is under Settings → Workspaces in the Anthropic console"
                    } else {
                        "check the key and `summarization_model` in .markdownattractor/config.toml"
                    };
                    Check {
                        name: "backend",
                        status: Status::Fail,
                        detail: format!("api: {msg}"),
                        fix: Some(fix),
                    }
                }
            }
        }
        Backend::Local => match rt.block_on(mda_core::worker::local::check(cfg)) {
            Ok(model) => Check {
                name: "backend",
                status: Status::Ok,
                detail: format!("local: {} serving {model}", cfg.local_base_url),
                fix: None,
            },
            Err(e) => Check {
                name: "backend",
                status: Status::Fail,
                detail: format!("local: {e}"),
                fix: Some("start the server (docs/guides/local-model.md) or `mda backend api`"),
            },
        },
        Backend::ClaudeCli => Check {
            name: "backend",
            status: Status::Warn,
            detail: "claude-cli: opt-in backend; routes requests through your Claude subscription"
                .to_owned(),
            fix: Some("prefer `mda backend api` or `mda backend local` (ADR-0002)"),
        },
    }
}

fn check_embeddings(cfg: &mda_core::config::Config) -> Check {
    let c = mda_core::embed::check(cfg);
    match c.model {
        None if !mda_core::embed::BUILT_WITH_EMBEDDINGS
            && cfg.embeddings != mda_core::config::Embeddings::Off =>
        {
            Check {
                name: "embeddings",
                status: Status::Warn,
                detail: "unavailable: this binary was built without the `embeddings` feature"
                    .to_owned(),
                fix: Some(
                    "install a build with embeddings, or `mda embeddings off` to silence this",
                ),
            }
        }
        None => Check {
            name: "embeddings",
            status: Status::Warn,
            detail: "off: search is lexical only".to_owned(),
            fix: Some(
                "`mda embeddings local-small` turns vectors on (33 MB model, downloaded once)",
            ),
        },
        Some(m) if c.cached => Check {
            name: "embeddings",
            status: Status::Ok,
            detail: format!("{m} in {}", c.cache_dir.display()),
            fix: None,
        },
        Some(m) => Check {
            name: "embeddings",
            status: Status::Warn,
            detail: format!("{m} not downloaded yet (cache {})", c.cache_dir.display()),
            fix: Some(
                "`mda rebuild --embeddings` downloads it now; otherwise the first index run does",
            ),
        },
    }
}

fn check_daemon(root: &std::path::Path) -> Check {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match super::live_status(&root) {
        Some(l) => Check {
            name: "daemon",
            status: if l.watching { Status::Ok } else { Status::Warn },
            detail: format!(
                "running · pid {} · up {} · {}",
                l.pid,
                super::humanize_secs(l.uptime_secs),
                l.watcher_error.as_deref().unwrap_or("watching")
            ),
            fix: (!l.watching).then_some("restart it: `mda restart`"),
        },
        None if mda_core::daemon::pid_path(&root).exists() => Check {
            name: "daemon",
            status: Status::Warn,
            detail: "stale pid file: the last daemon did not exit cleanly".to_owned(),
            fix: Some("`mda start` (stale files are cleaned up)"),
        },
        None => Check {
            name: "daemon",
            status: Status::Warn,
            detail: "not running".to_owned(),
            fix: Some("`mda start` keeps the index live as you edit"),
        },
    }
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
