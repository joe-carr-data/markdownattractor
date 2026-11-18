//! One module per subcommand. Each exposes `Args` and `run`, returns an `ExitCode`, and does
//! all presentation through [`crate::output`].

pub mod backend;
pub mod card;
pub mod check;
pub mod cost;
pub mod daemon;
pub mod diagnostics;
pub mod doctor;
pub mod embeddings;
pub mod eval;
pub mod explain;
pub mod index;
pub mod mcp;
pub mod nudge;
pub mod open;
pub mod parse;
pub mod pause;
pub mod rebuild;
pub mod recent;
pub mod schema;
pub mod search;
pub mod stale;
pub mod start;
pub mod status;
pub mod stop;
pub mod timeline;
pub mod watch;

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use mda_core::config::STATE_DIR;

/// Resolve the watched root: an explicit `--root`, else the nearest ancestor of the current
/// directory that already has a `.markdownattractor/` state directory, else the current
/// directory itself.
pub fn resolve_root(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(r) = explicit {
        return Ok(r.to_path_buf());
    }
    let cwd = std::env::current_dir()?;
    let mut dir: Option<&Path> = Some(&cwd);
    while let Some(d) = dir {
        if d.join(STATE_DIR).is_dir() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    Ok(cwd)
}

/// The `--in` filter of `search` and `timeline` as a root-relative prefix.
///
/// The user names their own files here, so the path may be looked up: an absolute path is
/// compared with the root after symlinks in its existing part are resolved (`/var/x` and
/// `/private/var/x` are one place), and refused when it is outside the root. A relative path
/// is relative to the root; only when nothing is there and something is at that path below
/// the current folder is it read from the current folder. The root wins when both exist.
pub fn in_prefix(root: &Path, raw: &str) -> anyhow::Result<Option<String>> {
    use mda_core::search::{canonical_where_it_exists, path_prefix_under};
    let given = Path::new(raw);
    let absolute = if given.is_absolute() {
        Some(given.to_path_buf())
    } else {
        let cwd = std::env::current_dir().ok();
        cwd.map(|c| c.join(given)).filter(|p| !root.join(given).exists() && p.exists())
    };
    let Some(abs) = absolute else {
        return Ok(path_prefix_under(&[root], raw)?);
    };
    let mut text = canonical_where_it_exists(&abs).to_string_lossy().into_owned();
    if (raw.ends_with('/') || raw.ends_with(std::path::MAIN_SEPARATOR)) && !text.ends_with('/') {
        text.push('/');
    }
    // The typed spelling is tried too: the root may itself be reached through a symlink.
    path_prefix_under(&[root], &text)
        .or_else(|_| path_prefix_under(&[root], &abs.to_string_lossy()))
        .map_err(Into::into)
}

/// When the root was found above the current folder (no explicit root given), one phrase that
/// says so: a command run inside a project is then working on a wider index than the user may
/// think. `None` when the root is the current folder or was given explicitly.
pub fn ancestor_note(root: &Path, explicit: bool) -> Option<String> {
    if explicit {
        return None;
    }
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let cwd = canon(&std::env::current_dir().ok()?);
    (canon(root) != cwd).then(|| {
        "the index found above the current folder; `--root .` uses this folder instead".to_owned()
    })
}

/// Run a short async operation (socket round-trips) on a fresh current-thread runtime.
pub fn block_on<F: std::future::Future>(f: F) -> anyhow::Result<F::Output> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    Ok(rt.block_on(f))
}

/// What to say when the daemon that answers was started by another version of mda than the
/// one running this command (each plugin version has its own binary, and a daemon outlives
/// an update until it is restarted). `None` when the versions are the same.
#[must_use]
pub fn daemon_version_note(daemon_version: &str) -> Option<String> {
    (daemon_version != mda_core::VERSION).then(|| {
        format!(
            "the running daemon is version {daemon_version}, this command is {}: `mda restart` starts it on this version",
            mda_core::VERSION
        )
    })
}

/// The live status of the daemon for `root`, if one answers.
pub fn live_status(root: &Path) -> Option<mda_core::daemon::LiveStatus> {
    use mda_core::daemon::{Client, Request, Response};
    block_on(async {
        let mut c = Client::connect(root).await.ok()?;
        match c.request(&Request::Status).await {
            Ok(Response::Status(s)) => Some(*s),
            _ => None,
        }
    })
    .ok()
    .flatten()
}

/// `1h 02m`, `3d 04h`, `45s`.
pub fn humanize_secs(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m {:02}s", secs / 60, secs % 60),
        3_600..=86_399 => format!("{}h {:02}m", secs / 3_600, (secs % 3_600) / 60),
        _ => format!("{}d {:02}h", secs / 86_400, (secs % 86_400) / 3_600),
    }
}

/// Parse `7d`, `24h`, `30m`, `2w`, or an ISO date/time into an absolute timestamp.
pub fn parse_time(s: &str) -> anyhow::Result<Timestamp> {
    Ok(mda_core::timefmt::parse_time(s)?)
}

/// `2 days ago`, `3 hours ago`, `just now`.
pub fn humanize_age(ts: Timestamp) -> String {
    mda_core::timefmt::humanize_age(ts)
}

/// The embedder the config asks for (`None` when embeddings are off).
pub fn embedder_for(
    cfg: &mda_core::config::Config,
) -> Option<std::sync::Arc<dyn mda_core::embed::Embedder>> {
    mda_core::embed::embedder_for(cfg)
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan;

    use super::*;

    #[test]
    fn relative_forms_parse() {
        let now = mda_core::clock::now();
        let t = parse_time("7d").unwrap();
        let diff = now.as_second() - t.as_second();
        assert!((diff - 7 * 86_400).abs() <= 2);
        assert!(parse_time("90m").is_ok());
        assert!(parse_time("2w").is_ok());
        assert!(parse_time("3x").is_err());
    }

    #[test]
    fn absolute_forms_parse() {
        assert_eq!(parse_time("2026-09-21").unwrap().to_string(), "2026-09-21T00:00:00Z");
        assert!(parse_time("2026-09-21T10:00:00Z").is_ok());
        assert!(parse_time("yesterday").is_err());
    }

    #[test]
    fn humanize_secs_forms() {
        assert_eq!(humanize_secs(45), "45s");
        assert_eq!(humanize_secs(125), "2m 05s");
        assert_eq!(humanize_secs(3_720), "1h 02m");
        assert_eq!(humanize_secs(100_000), "1d 03h");
    }

    #[test]
    fn humanize() {
        let now = mda_core::clock::now();
        assert_eq!(humanize_age(now), "just now");
        assert_eq!(humanize_age(now - 90.minutes()), "1 hour ago");
        assert_eq!(humanize_age(now - (3 * 24).hours()), "3 days ago");
    }
}

#[cfg(test)]
mod version_note_tests {
    use super::daemon_version_note;

    #[test]
    fn a_daemon_of_another_version_is_named_and_the_same_version_is_silent() {
        assert_eq!(daemon_version_note(mda_core::VERSION), None);
        let note = daemon_version_note("0.0.1").unwrap();
        assert!(note.contains("0.0.1") && note.contains(mda_core::VERSION), "{note}");
        assert!(note.contains("mda restart"), "{note}");
    }
}
