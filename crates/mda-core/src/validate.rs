//! Enforce the card contract on what the model returned.
//!
//! The JSON schema guarantees shape; this module guarantees **substance**:
//!
//! - **Caps.** Lists longer than the contract are trimmed, strings are bounded, keywords are
//!   lower-cased and de-duplicated. Trimming is not a failure; the card is still useful.
//! - **Grounding (G6).** Every `mentioned_dates` entry must quote evidence that occurs in the
//!   section, and every entity must occur in the section. Anything that does not is dropped and
//!   reported. Comparison happens after [`normalize`], so straightened quotes or a collapsed
//!   space never reject a correct extraction.
//! - **Rejection** only for a card with no `tldr` or no `summary`: there is nothing to index.

use serde::{Deserialize, Serialize};

use crate::card::{Entities, MentionedDate, SectionSummary};
use crate::{Error, Result};

/// Upper bounds applied to a summary. Defaults match the prompt contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caps {
    /// Max keywords kept.
    pub keywords: usize,
    /// Max questions kept.
    pub questions: usize,
    /// Max decisions kept.
    pub decisions: usize,
    /// Max action items kept.
    pub action_items: usize,
    /// Max entities per category.
    pub entities_per_category: usize,
    /// Max dates kept (after grounding).
    pub dates: usize,
    /// Longest string kept in any field, in characters.
    pub max_chars: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            keywords: 8,
            questions: 4,
            decisions: 4,
            action_items: 3,
            entities_per_category: 6,
            dates: 12,
            max_chars: 400,
        }
    }
}

/// Outcome of validation: the cleaned summary plus what was changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validated {
    /// The summary, cleaned and capped.
    pub summary: SectionSummary,
    /// Dates whose evidence was not found in the section.
    pub dropped_dates: Vec<MentionedDate>,
    /// Entities not found in the section, as `category:value`.
    pub dropped_entities: Vec<String>,
    /// Fields that were trimmed to a cap.
    pub trimmed: Vec<String>,
}

impl Validated {
    /// `true` when nothing was dropped or trimmed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.dropped_dates.is_empty() && self.dropped_entities.is_empty() && self.trimmed.is_empty()
    }
}

/// Validate `summary` against the section text it was produced from.
///
/// # Errors
///
/// [`Error::Validation`] when the card has no usable `tldr` or `summary`.
pub fn validate(section_text: &str, summary: SectionSummary, caps: &Caps) -> Result<Validated> {
    let haystack = normalize(section_text);
    let haystack_lower = haystack.to_lowercase();
    let mut trimmed = Vec::new();

    let tldr = clip(summary.tldr.trim(), caps.max_chars);
    let body = clip(summary.summary.trim(), caps.max_chars * 2);
    if tldr.is_empty() || body.is_empty() {
        return Err(Error::Validation {
            subject: "section summary".into(),
            reason: "tldr or summary is empty".into(),
        });
    }

    let keywords = cap_list(
        dedupe(summary.keywords.iter().map(|k| k.trim().to_lowercase()), true),
        caps.keywords,
        "keywords",
        &mut trimmed,
        caps.max_chars,
    );
    let questions_answered = cap_list(
        dedupe(summary.questions_answered.iter().map(|q| q.trim().to_owned()), false),
        caps.questions,
        "questions_answered",
        &mut trimmed,
        caps.max_chars,
    );
    let decisions = cap_list(
        dedupe(summary.decisions.iter().map(|d| d.trim().to_owned()), false),
        caps.decisions,
        "decisions",
        &mut trimmed,
        caps.max_chars,
    );
    let action_items = cap_list(
        dedupe(summary.action_items.iter().map(|a| a.trim().to_owned()), false),
        caps.action_items,
        "action_items",
        &mut trimmed,
        caps.max_chars,
    );

    let mut dropped_entities = Vec::new();
    let entities = ground_all_entities(
        &summary.entities,
        &haystack_lower,
        caps,
        &mut dropped_entities,
        &mut trimmed,
    );

    let mut dropped_dates = Vec::new();
    let mut mentioned_dates = Vec::new();
    for date in summary.mentioned_dates {
        if is_grounded_date(&date, &haystack) {
            mentioned_dates.push(MentionedDate {
                raw: clip(date.raw.trim(), caps.max_chars),
                iso: date.iso.trim().to_owned(),
                precision: date.precision,
                evidence: clip(date.evidence.trim(), caps.max_chars),
            });
        } else {
            dropped_dates.push(date);
        }
    }
    if mentioned_dates.len() > caps.dates {
        mentioned_dates.truncate(caps.dates);
        trimmed.push("mentioned_dates".to_owned());
    }

    Ok(Validated {
        summary: SectionSummary {
            tldr,
            summary: body,
            keywords,
            questions_answered,
            entities,
            mentioned_dates,
            decisions,
            action_items,
        },
        dropped_dates,
        dropped_entities,
        trimmed,
    })
}

/// A date is grounded when its evidence occurs in the section, the evidence contains the raw
/// date text, and `iso` is well-formed for its precision.
fn is_grounded_date(date: &MentionedDate, haystack: &str) -> bool {
    let evidence = normalize(&date.evidence);
    let raw = normalize(&date.raw);
    if evidence.is_empty() || raw.is_empty() {
        return false;
    }
    haystack.contains(&evidence)
        && evidence.to_lowercase().contains(&raw.to_lowercase())
        && iso_is_well_formed(&date.iso)
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD` with sane ranges. Anything else is not stored.
fn iso_is_well_formed(iso: &str) -> bool {
    let parts: Vec<&str> = iso.trim().split('-').collect();
    let num = |s: &str, digits: usize| -> Option<u32> {
        (s.len() == digits && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    match parts.as_slice() {
        [y] => num(y, 4).is_some_and(|y| (1000..=2999).contains(&y)),
        [y, m] => {
            num(y, 4).is_some_and(|y| (1000..=2999).contains(&y))
                && num(m, 2).is_some_and(|m| (1..=12).contains(&m))
        }
        [y, m, d] => {
            num(y, 4).is_some_and(|y| (1000..=2999).contains(&y))
                && num(m, 2).is_some_and(|m| (1..=12).contains(&m))
                && num(d, 2).is_some_and(|d| (1..=31).contains(&d))
        }
        _ => false,
    }
}

/// Ground every entity category against the section text.
fn ground_all_entities(
    e: &Entities,
    haystack_lower: &str,
    caps: &Caps,
    dropped: &mut Vec<String>,
    trimmed: &mut Vec<String>,
) -> Entities {
    let mut g =
        |name, values| ground_entities(name, values, haystack_lower, caps, dropped, trimmed);
    Entities {
        people: g("people", &e.people),
        orgs: g("orgs", &e.orgs),
        products: g("products", &e.products),
        technologies: g("technologies", &e.technologies),
        files_paths: g("files_paths", &e.files_paths),
        commands: g("commands", &e.commands),
    }
}

fn ground_entities(
    category: &'static str,
    values: &[String],
    haystack_lower: &str,
    caps: &Caps,
    dropped: &mut Vec<String>,
    trimmed: &mut Vec<String>,
) -> Vec<String> {
    let mut kept = Vec::new();
    for v in dedupe(values.iter().map(|v| v.trim().to_owned()), false) {
        let needle = normalize(&v).to_lowercase();
        if !needle.is_empty() && haystack_lower.contains(&needle) {
            kept.push(clip(&v, caps.max_chars));
        } else {
            dropped.push(format!("{category}:{v}"));
        }
    }
    if kept.len() > caps.entities_per_category {
        kept.truncate(caps.entities_per_category);
        trimmed.push(category.to_owned());
    }
    kept
}

fn cap_list(
    mut values: Vec<String>,
    cap: usize,
    name: &'static str,
    trimmed: &mut Vec<String>,
    max_chars: usize,
) -> Vec<String> {
    if values.len() > cap {
        values.truncate(cap);
        trimmed.push(name.to_owned());
    }
    values.into_iter().map(|v| clip(&v, max_chars)).collect()
}

/// Drop empties and duplicates, keeping first occurrence and order.
fn dedupe(values: impl Iterator<Item = String>, case_insensitive: bool) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in values {
        if v.is_empty() {
            continue;
        }
        let key = if case_insensitive { v.to_lowercase() } else { v.clone() };
        if seen.insert(key) {
            out.push(v);
        }
    }
    out
}

/// Truncate to `max` characters on a char boundary.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_owned() } else { s.chars().take(max).collect() }
}

/// Fold typographic variants the model tends to straighten, and collapse whitespace.
///
/// Curly quotes → straight, en/em dashes → `-`, ellipsis → `...`, non-breaking and other
/// Unicode spaces → space, runs of whitespace → one space. Case is preserved.
#[must_use]
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        let mapped: &str = match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' | '\u{02BC}' => "'",
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => "\"",
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => "-",
            '\u{2026}' => "...",
            c if c.is_whitespace() => {
                pending_space = true;
                continue;
            }
            _ => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(c);
                continue;
            }
        };
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push_str(mapped);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::DatePrecision;

    fn base() -> SectionSummary {
        SectionSummary {
            tldr: "Rollback uses deployctl.".into(),
            summary: "Run deployctl rollback. Verify afterwards.".into(),
            keywords: vec!["Rollback".into(), "deploy".into(), "rollback".into(), String::new()],
            questions_answered: vec!["How do I roll back?".into()],
            entities: Entities {
                commands: vec!["deployctl rollback --to <previous-sha>".into(), "rm -rf /".into()],
                products: vec!["PagerDuty".into()],
                technologies: vec![],
                people: vec![],
                orgs: vec![],
                files_paths: vec![],
            },
            mentioned_dates: vec![],
            decisions: vec![],
            action_items: vec![],
        }
    }

    const TEXT: &str = "### Rollback\n\nSince “March 2026” we use blue–green. Run `deployctl rollback --to <previous-sha>` and page via PagerDuty.\n";

    #[test]
    fn keywords_are_lowercased_and_deduped_and_empties_dropped() {
        let v = validate(TEXT, base(), &Caps::default()).unwrap();
        assert_eq!(v.summary.keywords, vec!["rollback", "deploy"]);
    }

    #[test]
    fn ungrounded_entities_are_dropped_and_reported() {
        let v = validate(TEXT, base(), &Caps::default()).unwrap();
        assert_eq!(v.summary.entities.commands, vec!["deployctl rollback --to <previous-sha>"]);
        assert_eq!(v.dropped_entities, vec!["commands:rm -rf /"]);
        assert_eq!(v.summary.entities.products, vec!["PagerDuty"]);
    }

    #[test]
    fn date_with_curly_quotes_in_source_is_grounded() {
        let mut s = base();
        s.mentioned_dates.push(MentionedDate {
            raw: "March 2026".into(),
            iso: "2026-03".into(),
            precision: DatePrecision::Month,
            evidence: "Since \"March 2026\" we use blue-green".into(),
        });
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert_eq!(v.summary.mentioned_dates.len(), 1);
        assert!(v.dropped_dates.is_empty());
    }

    #[test]
    fn date_with_invented_evidence_is_dropped() {
        let mut s = base();
        s.mentioned_dates.push(MentionedDate {
            raw: "April 2026".into(),
            iso: "2026-04".into(),
            precision: DatePrecision::Month,
            evidence: "In April 2026 we migrated".into(),
        });
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert!(v.summary.mentioned_dates.is_empty());
        assert_eq!(v.dropped_dates.len(), 1);
    }

    #[test]
    fn date_whose_evidence_does_not_contain_raw_is_dropped() {
        let mut s = base();
        s.mentioned_dates.push(MentionedDate {
            raw: "2026-03-01".into(),
            iso: "2026-03-01".into(),
            precision: DatePrecision::Day,
            evidence: "we use blue-green".into(),
        });
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert!(v.summary.mentioned_dates.is_empty());
    }

    #[test]
    fn malformed_iso_is_dropped() {
        let mut s = base();
        s.mentioned_dates.push(MentionedDate {
            raw: "March 2026".into(),
            iso: "2026-13".into(),
            precision: DatePrecision::Month,
            evidence: "Since \"March 2026\" we use".into(),
        });
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert!(v.summary.mentioned_dates.is_empty());
    }

    #[test]
    fn iso_forms() {
        assert!(iso_is_well_formed("2026"));
        assert!(iso_is_well_formed("2026-09"));
        assert!(iso_is_well_formed("2026-09-21"));
        assert!(!iso_is_well_formed("26"));
        assert!(!iso_is_well_formed("2026-9"));
        assert!(!iso_is_well_formed("2026-00-10"));
        assert!(!iso_is_well_formed("2026-09-32"));
        assert!(!iso_is_well_formed("2026-09-21T10:00"));
        assert!(!iso_is_well_formed("Q4 2026"));
    }

    #[test]
    fn lists_are_capped_and_reported() {
        let mut s = base();
        s.decisions = (0..10).map(|i| format!("decision {i}")).collect();
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert_eq!(v.summary.decisions.len(), 4);
        assert!(v.trimmed.iter().any(|t| t == "decisions"));
        assert!(!v.is_clean());
    }

    #[test]
    fn long_strings_are_clipped_on_char_boundary() {
        let mut s = base();
        s.tldr = "é".repeat(500);
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert_eq!(v.summary.tldr.chars().count(), 400);
    }

    #[test]
    fn empty_tldr_is_rejected() {
        let mut s = base();
        s.tldr = "   ".into();
        let err = validate(TEXT, s, &Caps::default()).unwrap_err();
        assert!(matches!(err, Error::Validation { .. }), "{err}");
    }

    #[test]
    fn normalize_folds_typography_and_whitespace() {
        assert_eq!(normalize("“a” – b\u{a0}c   d\n\te…"), "\"a\" - b c d e...");
        assert_eq!(normalize("  leading and trailing  "), "leading and trailing");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn clean_card_is_clean() {
        let mut s = base();
        s.keywords = vec!["rollback".into()];
        s.entities.commands.pop();
        let v = validate(TEXT, s, &Caps::default()).unwrap();
        assert!(v.is_clean(), "{v:?}");
    }
}
