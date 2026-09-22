//! Time parsing and rendering shared by the CLI and the MCP server.

use jiff::{Span, Timestamp};

use crate::{Error, Result};

/// Parse `7d`, `24h`, `30m`, `2w`, or an ISO date/time into an absolute timestamp.
///
/// Relative forms count back from now; `YYYY-MM-DD` means midnight UTC of that day.
pub fn parse_time(s: &str) -> Result<Timestamp> {
    let s = s.trim();
    let bad = |msg: String| Error::Config(msg);
    if let Some((num, unit)) = split_relative(s) {
        // Every step is fallible: a huge number must be an error, never a panic (the MCP
        // server runs with `panic = "abort"` and takes these strings from tool arguments).
        let n: i64 = num.parse().map_err(|_| bad(format!("bad number in {s:?}")))?;
        let too_big = || bad(format!("{s:?} is too far back"));
        let span: Span = match unit {
            "m" | "min" => Span::new().try_minutes(n).map_err(|_| too_big())?,
            "h" | "hr" | "hour" | "hours" => Span::new().try_hours(n).map_err(|_| too_big())?,
            "d" | "day" | "days" => {
                let hours = n.checked_mul(24).ok_or_else(too_big)?;
                Span::new().try_hours(hours).map_err(|_| too_big())?
            }
            "w" | "week" | "weeks" => {
                let hours = n.checked_mul(24 * 7).ok_or_else(too_big)?;
                Span::new().try_hours(hours).map_err(|_| too_big())?
            }
            _ => return Err(bad(format!("unknown time unit {unit:?} in {s:?} (use m, h, d, w)"))),
        };
        return Timestamp::now().checked_sub(span).map_err(|_| too_big());
    }
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return date
            .to_zoned(jiff::tz::TimeZone::UTC)
            .map(|z| z.timestamp())
            .map_err(|e| bad(e.to_string()));
    }
    Err(bad(format!("cannot parse {s:?} as a duration (7d, 24h) or a date (2026-09-21)")))
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
#[must_use]
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
    fn relative_and_absolute_forms() {
        let now = Timestamp::now();
        let t = parse_time("7d").unwrap();
        assert!(((now.as_second() - t.as_second()) - 7 * 86_400).abs() <= 2);
        assert!(parse_time("90m").is_ok());
        assert!(parse_time("2w").is_ok());
        assert!(parse_time("3x").is_err());
        assert_eq!(parse_time("2026-09-21").unwrap().to_string(), "2026-09-21T00:00:00Z");
        assert!(parse_time("2026-09-21T10:00:00Z").is_ok());
        assert!(parse_time("yesterday").is_err());
        assert_eq!(humanize_age(now), "just now");
        assert_eq!(humanize_age(now - jiff::SignedDuration::from_mins(90)), "1 hour ago");
    }

    #[test]
    fn absurd_durations_are_errors_not_panics() {
        for s in [
            "9223372036854775807h",
            "9223372036854775807d",
            "999999999999999w",
            "99999999999999999999m",
        ] {
            assert!(parse_time(s).is_err(), "{s}");
        }
    }
}
