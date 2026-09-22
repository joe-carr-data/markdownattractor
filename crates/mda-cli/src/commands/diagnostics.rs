//! `mda diagnostics [--out <file>]` — a redacted bundle to attach to an issue.
//!
//! What goes in: versions, build features, OS and architecture, an **allowlisted** view of the
//! config (never the raw file: URLs are reduced to scheme and host, free-form strings are
//! scrubbed), store counts and schema version, the embedding check, the daemon's live status,
//! `doctor`'s checks, and a bounded tail of the newest daemon log. Every free-form string
//! (errors, log lines, doctor details) goes through the same scrubber: the home directory
//! becomes `~`, any other absolute path becomes `<path>`, and anything that looks like a
//! credential in a URL is dropped. What never goes in: document content, document paths,
//! or the config file's own text.

use std::io::{Read as _, Seek as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mda_core::pipeline::Engine;

use crate::output::{self, Style};

/// Arguments for `mda diagnostics`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Write the bundle to this file (must not exist yet) instead of stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// How many log lines the bundle keeps.
const LOG_TAIL: usize = 40;
/// How many bytes of the newest log are read to find those lines.
const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let bundle = bundle(&root);
    let text = serde_json::to_string_pretty(&bundle)?;
    let st = Style::auto();
    match &args.out {
        Some(path) => {
            write_new(path, &text)?;
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

/// Create `path` fresh: never follows a symlink, never truncates an existing file.
fn write_new(path: &Path, text: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| {
        anyhow::anyhow!("cannot create {} ({e}); pick a path that does not exist", path.display())
    })?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

/// The scrubber every free-form string goes through.
struct Scrub {
    homes: Vec<String>,
}

impl Scrub {
    fn new() -> Self {
        // Both spellings of the home directory: as given and canonical (`/var` vs
        // `/private/var` on macOS), so a canonical root is redacted too.
        let homes = std::env::home_dir()
            .into_iter()
            .flat_map(|h| [h.canonicalize().unwrap_or_else(|_| h.clone()), h])
            .map(|h| h.display().to_string())
            .filter(|h| !h.is_empty() && h != "/")
            .collect();
        Self { homes }
    }

    /// `~` for the home directory, `<path>` for any other absolute path, credentials
    /// stripped from URLs.
    fn text(&self, s: &str) -> String {
        let mut out = self.homes.iter().fold(s.to_owned(), |acc, h| acc.replace(h.as_str(), "~"));
        out = strip_url_credentials(&out);
        redact_absolute_paths(&out)
    }

    fn opt(&self, s: Option<&str>) -> Option<String> {
        s.map(|s| self.text(s))
    }
}

/// `scheme://user:pass@host/…` → `scheme://host/…`.
fn strip_url_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(at) => out.push_str(&authority[at + 1..]),
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Replace every absolute path (Unix `/…`, Windows `C:\…` or `\\?\…`) with `<path>`, except
/// the `~` form the home replacement already produced. Paths in diagnostics are almost
/// always document paths or machine layout, neither of which the reader needs.
fn redact_absolute_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    let is_path_char =
        |c: char| !c.is_whitespace() && !matches!(c, '"' | '\'' | '`' | ')' | ']' | ',' | ';');
    while let Some((i, c)) = chars.next() {
        // A `/` opens a path unless it continues a word (`a/b`, `1/2`, `~/x`), a URL
        // (`://`, `//host`) or a path already being consumed.
        let starts_path = (c == '/'
            && (i == 0
                || !s[..i].ends_with(|p: char| {
                    p.is_alphanumeric() || matches!(p, '~' | '.' | ':' | '/')
                })))
            || (c.is_ascii_alphabetic()
                && s[i + c.len_utf8()..].starts_with(":\\")
                && (i == 0 || !s[..i].ends_with(|p: char| p.is_alphanumeric())))
            || (c == '\\' && s[i..].starts_with("\\\\"));
        if starts_path && s[i..].len() > 1 {
            // Consume the path token.
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if is_path_char(d) {
                    end = j + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            if s[i..end].len() > 1 {
                out.push_str("<path>");
                continue;
            }
            out.push_str(&s[i..end]);
            continue;
        }
        out.push(c);
    }
    out
}

/// `https://api.example.com` from a full URL: scheme and host only.
fn host_only(url: &str) -> String {
    match url.find("://") {
        Some(i) => {
            let (scheme, rest) = url.split_at(i + 3);
            let end = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..end];
            let host = authority.rsplit('@').next().unwrap_or(authority);
            format!("{scheme}{host}")
        }
        None => "<url>".to_owned(),
    }
}

/// Build the bundle. Everything that can fail is captured as a string so a broken store or
/// a missing daemon still yields a report.
fn bundle(root: &Path) -> serde_json::Value {
    let scrub = Scrub::new();
    let (cfg, config_error) = match mda_core::config::Config::load(root) {
        Ok(c) => (c, None),
        // The error can quote the offending line, so only its shape is kept.
        Err(e) => (mda_core::config::Config::default(), Some(config_error_kind(&e))),
    };
    // Allowlisted, never the raw file: URLs reduced to host, free strings scrubbed.
    let config = serde_json::json!({
        "backend": cfg.backend.as_str(),
        "summarization_model": scrub.text(&cfg.summarization_model),
        "escalation_model": scrub.opt(cfg.escalation_model.as_deref()),
        "api_base_url": host_only(&cfg.api_base_url),
        "api_key_env": cfg.api_key_env,
        "api_workspace_id_set": cfg.api_workspace_id.is_some(),
        "local_base_url": host_only(&cfg.local_base_url),
        "local_model": scrub.text(&cfg.local_model),
        "concurrency": cfg.concurrency,
        "daily_token_budget": cfg.daily_token_budget,
        "per_call_budget_usd": cfg.per_call_budget_usd,
        "worker_timeout_secs": cfg.worker_timeout_secs,
        "ignore_patterns": cfg.ignore.len(),
        "retention_days": cfg.retention_days,
        "nudge": cfg.nudge,
        "embeddings": cfg.embeddings.as_str(),
        "embedding_cache_dir_set": cfg.embedding_cache_dir.is_some(),
        "load_error": config_error,
    });

    let store = match Engine::open(root) {
        Ok(engine) => serde_json::json!({
            "counts": engine.store().counts().map_err(|e| e.to_string()).ok(),
            "schema_version": engine.store().schema_version().map_err(|e| e.to_string()).ok(),
            "stale": engine.stale().map(|s| s.len()).map_err(|e| e.to_string()).ok(),
        }),
        Err(e) => serde_json::json!({ "error": scrub.text(&e.to_string()) }),
    };

    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let daemon = super::live_status(&canon).map(|mut l| {
        l.root = scrub.text(&l.root);
        l.socket = scrub.text(&l.socket);
        l.last_error = scrub.opt(l.last_error.as_deref());
        l.watcher_error = scrub.opt(l.watcher_error.as_deref());
        l.embedding_error = scrub.opt(l.embedding_error.as_deref());
        l.hot_paths.clear(); // document paths are not needed to diagnose the daemon
        l
    });
    let embed = mda_core::embed::check(&cfg);
    let embeddings = serde_json::json!({
        "setting": embed.embeddings.as_str(),
        "model": embed.model,
        "cached": embed.cached,
        "cache_dir": scrub.text(&embed.cache_dir.display().to_string()),
    });

    let checks: Vec<serde_json::Value> = super::doctor::checks(root, &cfg)
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "status": c.status,
                "detail": scrub.text(&c.detail),
                "fix": c.fix,
            })
        })
        .collect();

    let log_tail: Vec<String> =
        log_tail(&canon).unwrap_or_default().iter().map(|l| scrub.text(l)).collect();

    serde_json::json!({
        "generated_at": jiff::Timestamp::now(),
        "mda_version": mda_core::VERSION,
        "built_with_embeddings": mda_core::embed::BUILT_WITH_EMBEDDINGS,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "root": scrub.text(&canon.display().to_string()),
        "config": config,
        "store": store,
        "embeddings": embeddings,
        "daemon": daemon,
        "doctor": checks,
        "daemon_log_tail": log_tail,
        "redacted": [
            "home directory (~)", "absolute paths (<path>)", "URL credentials",
            "config file text (allowlisted fields only)", "workspace id", "hot paths"
        ],
    })
}

/// The shape of a config load error without its text (which may quote the file).
fn config_error_kind(e: &mda_core::Error) -> &'static str {
    match e {
        mda_core::Error::Config(_) => "invalid config.toml",
        mda_core::Error::Io { .. } => "config.toml unreadable",
        _ => "config load failed",
    }
}

/// The last [`LOG_TAIL`] lines of the newest `daemon.<date>.log`, read from a bounded tail
/// of a regular file inside the state directory. Symlinks, FIFOs and anything outside
/// `.markdownattractor/logs` are skipped.
fn log_tail(root: &Path) -> Option<Vec<String>> {
    let dir = mda_core::config::state_dir(root).ok()?.join("logs");
    let mut logs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file())) // no symlinks, no FIFOs
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("daemon."))
                && p.extension().is_some_and(|e| e == "log")
        })
        .collect();
    logs.sort();
    let newest = logs.pop()?;
    let mut f = std::fs::File::open(&newest).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    f.seek(std::io::SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(len - start).unwrap_or(0));
    f.take(LOG_TAIL_BYTES).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 {
        lines.remove(0); // the first line is almost certainly cut
    }
    let from = lines.len().saturating_sub(LOG_TAIL);
    Some(lines[from..].iter().map(|l| (*l).to_owned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubber_redacts_home_paths_and_url_credentials() {
        let s = Scrub { homes: vec!["/private/var/x".to_owned(), "/var/x".to_owned()] };
        assert_eq!(s.text("/var/x/a and /private/var/x/b"), "~/a and ~/b");
        assert_eq!(s.text("read /etc/passwd failed"), "read <path> failed");
        assert_eq!(
            s.text("key https://user:secret@api.example.com/v1 ok"),
            "key https://api.example.com/v1 ok"
        );
        assert_eq!(s.text("at C:\\Users\\x\\doc.md"), "at <path>");
        assert_eq!(s.text("a/b relative stays"), "a/b relative stays");
        assert_eq!(s.text("1/2 of them"), "1/2 of them");
        assert_eq!(s.text("nothing here"), "nothing here");
    }

    #[test]
    fn host_only_keeps_scheme_and_host() {
        assert_eq!(host_only("https://user:pw@api.example.com/v1?x=1"), "https://api.example.com");
        assert_eq!(host_only("http://127.0.0.1:8080/v1"), "http://127.0.0.1:8080");
        assert_eq!(host_only("garbage"), "<url>");
    }
}
