//! The crate's clock: the real time, or one fixed instant under `MDA_NOW`.
//!
//! Every timestamp the engine stores or compares (`first_seen_at`, `updated_at`, tombstones,
//! `summarized_at`, relative query times) comes from [`now`], never from `Timestamp::now()`
//! directly. That single seam is what makes the benchmark's simulated historical replay
//! (execution plan §4 T5) possible: the harness checks a repository out commit by commit and
//! runs `MDA_NOW=<author date> mda index`, so the store's dates are the history's dates.
//!
//! The override is a **test and benchmark facility**, not a user feature (ADR-0006):
//!
//! - it is read once per process, from the environment, and never changes afterwards;
//! - a malformed value is refused by [`check`], which [`crate::pipeline::Engine::open`] calls,
//!   so nothing is indexed under a clock that could not be parsed (fail closed);
//! - the daemon refuses to run with it set ([`override_active`]): a watcher under a frozen
//!   clock would stamp every later edit with the same instant.

use std::sync::OnceLock;

use jiff::Timestamp;

use crate::{Error, Result};

/// The environment variable that fixes the clock: an RFC 3339 instant
/// (`2026-08-01T12:00:00Z`) or a calendar date (`2026-08-01`, midnight UTC).
pub const ENV: &str = "MDA_NOW";

type Loaded = std::result::Result<Option<Timestamp>, String>;

static OVERRIDE: OnceLock<Loaded> = OnceLock::new();

fn load() -> &'static Loaded {
    OVERRIDE.get_or_init(|| match std::env::var(ENV) {
        Ok(v) => parse_override(&v).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err("not valid UTF-8".to_owned()),
    })
}

/// Parse an override value. Relative forms (`7d`) are rejected: they would be relative to
/// the clock being overridden.
///
/// # Errors
///
/// Returns the reason the value is not an instant or a date.
pub fn parse_override(s: &str) -> std::result::Result<Timestamp, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty".to_owned());
    }
    if let Ok(t) = s.parse::<Timestamp>() {
        return Ok(t);
    }
    if let Ok(d) = s.parse::<jiff::civil::Date>() {
        return d
            .to_zoned(jiff::tz::TimeZone::UTC)
            .map(|z| z.timestamp())
            .map_err(|e| e.to_string());
    }
    Err(format!("{s:?} is not an RFC 3339 instant or a YYYY-MM-DD date"))
}

/// The current instant: the real clock, or the `MDA_NOW` override when one is set.
///
/// A malformed override falls back to the real clock here; [`check`] is where it is refused,
/// before any engine opens.
#[must_use]
pub fn now() -> Timestamp {
    match load() {
        Ok(Some(t)) => *t,
        Ok(None) | Err(_) => Timestamp::now(),
    }
}

/// Whether a (well-formed) override is in force.
#[must_use]
pub fn override_active() -> bool {
    matches!(load(), Ok(Some(_)))
}

/// The override in force, if any (for status output and the replay harness's validation).
#[must_use]
pub fn override_value() -> Option<Timestamp> {
    match load() {
        Ok(Some(t)) => Some(*t),
        Ok(None) | Err(_) => None,
    }
}

/// Refuse a malformed `MDA_NOW`. Called before anything is indexed or served.
///
/// # Errors
///
/// [`Error::Config`] naming the variable and the parse failure.
pub fn check() -> Result<()> {
    match load() {
        Err(e) => Err(Error::Config(format!("{ENV}: {e}"))),
        Ok(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_instants_and_dates_and_rejects_the_rest() {
        assert_eq!(
            parse_override("2026-08-01T12:00:00Z").unwrap().to_string(),
            "2026-08-01T12:00:00Z"
        );
        assert_eq!(parse_override(" 2026-08-01 ").unwrap().to_string(), "2026-08-01T00:00:00Z");
        assert_eq!(
            parse_override("2026-08-01T12:00:00+02:00").unwrap().to_string(),
            "2026-08-01T10:00:00Z"
        );
        for bad in ["", "7d", "yesterday", "2026-13-01", "1690000000"] {
            assert!(parse_override(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn without_an_override_now_is_the_real_clock() {
        if std::env::var_os(ENV).is_some() {
            return; // the process is running under an override; nothing to prove here
        }
        assert!(!override_active());
        assert!(override_value().is_none());
        assert!(check().is_ok());
        let (a, b) = (now(), Timestamp::now());
        assert!((b.as_second() - a.as_second()).abs() <= 2);
    }
}
