//! `mda diagnostics [--out <file>]` — a redacted bundle to attach to an issue.
//!
//! What goes in: versions, build features, OS and architecture, the config with the home
//! directory and the workspace id redacted (the config never holds an API key), store counts
//! and schema version, the embedding check, the daemon's live status, `doctor`'s checks, and
//! the tail of the newest daemon log. What never goes in: document content, or any path
//! outside the state directory except the root, which is shown relative to `~`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda diagnostics`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Write the bundle to this file instead of stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// How many log lines the bundle keeps.
const LOG_TAIL: usize = 40;

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let bundle = bundle(&root)?;
    let text = serde_json::to_string_pretty(&bundle)?;
    let st = Style::auto();
    match &args.out {
        Some(path) => {
            std::fs::write(path, &text)?;
            if json {
                output::json(&serde_json::json!({ "written": path, "bytes": text.len() }));
            } else {
                println!(
                    "{} bundle written to {} ({} bytes) · attach it to your issue",
                    st.ok("diagnostics"),
                    st.dim(&path.display().to_string()),
                    text.len()
                );
            }
        }
        None => println!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

/// Build the bundle. Everything that can fail is captured as a string so a broken store or
/// a missing daemon still yields a report.
fn bundle(root: &Path) -> anyhow::Result<serde_json::Value> {
    // Both spellings of the home directory: as given and canonical (`/var` vs `/private/var`
    // on macOS), so a canonical root is redacted too.
    let homes: Vec<String> = std::env::home_dir()
        .into_iter()
        .flat_map(|h| [h.canonicalize().unwrap_or_else(|_| h.clone()), h])
        .map(|h| h.display().to_string())
        .filter(|h| !h.is_empty() && h != "/")
        .collect();
    let redact = |s: &str| redact_text(s, &homes);

    let cfg = mda_core::config::Config::load(root).unwrap_or_default();
    let mut config = serde_json::to_value(&cfg)?;
    if let Some(obj) = config.as_object_mut() {
        if obj.get("api_workspace_id").is_some_and(|v| !v.is_null()) {
            obj.insert("api_workspace_id".into(), serde_json::Value::String("<redacted>".into()));
        }
        if let Some(d) = obj.get("embedding_cache_dir").and_then(|v| v.as_str()) {
            let r = redact(d);
            obj.insert("embedding_cache_dir".into(), serde_json::Value::String(r));
        }
    }

    let store = match Engine::open(root) {
        Ok(engine) => serde_json::json!({
            "counts": engine.store().counts().map_err(|e| e.to_string()).ok(),
            "schema_version": engine.store().schema_version().map_err(|e| e.to_string()).ok(),
            "stale": engine.stale().map(|s| s.len()).map_err(|e| e.to_string()).ok(),
        }),
        Err(e) => serde_json::json!({ "error": redact(&e.to_string()) }),
    };

    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let daemon = super::live_status(&canon).map(|mut l| {
        l.root = redact(&l.root);
        l.socket = redact(&l.socket);
        l.last_error = l.last_error.as_deref().map(redact);
        l.hot_paths.clear(); // document paths are not needed to diagnose the daemon
        l
    });
    let mut embed = mda_core::embed::check(&cfg);
    embed.cache_dir = PathBuf::from(redact(&embed.cache_dir.display().to_string()));

    let checks: Vec<serde_json::Value> = super::doctor::checks(root, &cfg)
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap_or_default();
            if let Some(d) = v.get("detail").and_then(|d| d.as_str()) {
                let r = redact(d);
                v["detail"] = serde_json::Value::String(r);
            }
            v
        })
        .collect();

    let log_tail: Vec<String> = newest_log(&canon)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| {
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(LOG_TAIL);
            lines[start..].iter().map(|l| redact(l)).collect()
        })
        .unwrap_or_default();

    Ok(serde_json::json!({
        "generated_at": jiff::Timestamp::now(),
        "mda_version": mda_core::VERSION,
        "built_with_embeddings": mda_core::embed::BUILT_WITH_EMBEDDINGS,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "root": redact(&canon.display().to_string()),
        "config": config,
        "store": store,
        "embeddings": embed,
        "daemon": daemon,
        "doctor": checks,
        "daemon_log_tail": log_tail,
        "redacted": ["home directory", "api_workspace_id", "hot paths"],
    }))
}

/// Replace every spelling of the home directory with `~` everywhere in `s`.
fn redact_text(s: &str, homes: &[String]) -> String {
    homes.iter().fold(s.to_owned(), |acc, h| acc.replace(h.as_str(), "~"))
}

/// The most recent `daemon.<date>.log` under the state directory, if any.
fn newest_log(root: &Path) -> Option<PathBuf> {
    let dir = root.join(mda_core::config::STATE_DIR).join("logs");
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("daemon."))
                && p.extension().is_some_and(|e| e == "log")
        })
        .collect();
    logs.sort();
    logs.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_replaces_every_home_occurrence() {
        let homes = vec!["/private/var/x".to_owned(), "/var/x".to_owned()];
        assert_eq!(redact_text("/var/x/a and /private/var/x/b", &homes), "~/a and ~/b");
        assert_eq!(redact_text("/Users/x/a", &[]), "/Users/x/a");
    }
}
