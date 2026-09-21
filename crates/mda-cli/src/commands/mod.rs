//! One module per subcommand. Each exposes `Args` and `run`, returns an `ExitCode`, and does
//! all presentation through [`crate::output`].

pub mod card;
pub mod doctor;
pub mod index;
pub mod open;
pub mod parse;
pub mod schema;
pub mod search;
pub mod status;

use std::path::{Path, PathBuf};

use jiff::{Span, Timestamp, ToSpan};
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

/// Parse `7d`, `24h`, `30m`, `2w`, or an ISO date/time into an absolute timestamp.
///
/// Relative forms count back from now; `YYYY-MM-DD` means midnight UTC of that day.
pub fn parse_time(s: &str) -> anyhow::Result<Timestamp> {
    let s = s.trim();
    if let Some((num, unit)) = split_relative(s) {
        let n: i64 = num.parse()?;
        let span: Span = match unit {
            "m" | "min" => n.minutes(),
            "h" | "hr" | "hour" | "hours" => n.hours(),
            "d" | "day" | "days" => (n * 24).hours(),
            "w" | "week" | "weeks" => (n * 24 * 7).hours(),
            _ => anyhow::bail!("unknown time unit {unit:?} in {s:?} (use m, h, d, w)"),
        };
        return Ok(Timestamp::now().checked_sub(span)?);
    }
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return Ok(date.to_zoned(jiff::tz::TimeZone::UTC)?.timestamp());
    }
    anyhow::bail!("cannot parse {s:?} as a duration (7d, 24h) or a date (2026-09-21)")
}

fn split_relative(s: &str) -> Option<(&str, &str)> {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits == s.len() {
        return None;
    }
    let (num, unit) = s.split_at(digits);
    unit.chars().all(|c| c.is_ascii_alphabetic()).then_some((num, unit))
}

/// `2 days ago`, `3 hours ago`, `just now`.
pub fn humanize_age(ts: Timestamp) -> String {
    let secs = (Timestamp::now().as_second() - ts.as_second()).max(0);
    let (n, unit) = match secs {
        0..=59 => return "just now".to_owned(),
        60..=3_599 => (secs / 60, "min"),
        3_600..=86_399 => (secs / 3_600, "hour"),
        86_400..=2_591_999 => (secs / 86_400, "day"),
        2_592_000..=31_535_999 => (secs / 2_592_000, "month"),
        _ => (secs / 31_536_000, "year"),
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}

#[cfg(test)]
mod tests {
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
    fn humanize() {
        let now = Timestamp::now();
        assert_eq!(humanize_age(now), "just now");
        assert_eq!(humanize_age(now - 90.minutes()), "1 hour ago");
        assert_eq!(humanize_age(now - (3 * 24).hours()), "3 days ago");
    }
}
