//! One module per subcommand. Each exposes `Args` and `run`, returns an `ExitCode`, and does
//! all presentation through [`crate::output`].

pub mod backend;
pub mod card;
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

/// Run a short async operation (socket round-trips) on a fresh current-thread runtime.
pub fn block_on<F: std::future::Future>(f: F) -> anyhow::Result<F::Output> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    Ok(rt.block_on(f))
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
        let now = Timestamp::now();
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
        let now = Timestamp::now();
        assert_eq!(humanize_age(now), "just now");
        assert_eq!(humanize_age(now - 90.minutes()), "1 hour ago");
        assert_eq!(humanize_age(now - (3 * 24).hours()), "3 days ago");
    }
}
