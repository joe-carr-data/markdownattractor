//! The unit of a review: one section of one file, before and after.
//!
//! [`compare`] takes the two texts of a file and pairs their sections conservatively:
//!
//! 1. sections with the same hash are unchanged: first those under the same heading path, then
//!    those that moved, when no section competes for either heading path;
//! 2. of the rest, a section pairs with the one that has the same heading path when that
//!    heading path occurs exactly once on each side;
//! 3. a section left on the after side is an *addition* only when nothing is left on the
//!    before side that it could be a renamed or moved form of;
//! 4. everything else is [`NotReviewed`], with the reason. A deletion is not reviewed.

use std::collections::HashMap;

use serde::Serialize;

use crate::markdown::{self, Flavor, Section};

/// One side of a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Side {
    /// The section text as the parser normalised it.
    pub text: String,
    /// Its hash.
    pub hash: String,
    /// First line in its file, 1-based.
    pub line_start: u32,
    /// Last line in its file, inclusive.
    pub line_end: u32,
}

impl Side {
    fn of(s: &Section) -> Self {
        Self {
            text: s.text.clone(),
            hash: s.hash.clone(),
            line_start: s.line_start,
            line_end: s.line_end,
        }
    }
}

/// A section that changed, or is new.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionChange {
    /// Headings from the document root down to the section, as they are now.
    pub heading_path: Vec<String>,
    /// The section before; `None` for an addition.
    pub before: Option<Side>,
    /// The section now.
    pub after: Side,
    /// Lines of the before text that the after text no longer has (trimmed, non-blank).
    pub removed_lines: Vec<String>,
    /// Lines of the after text that the before text did not have (trimmed, non-blank).
    pub added_lines: Vec<String>,
}

/// Why a section was left out of the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// Its heading path occurs more than once on a side: pairing by position would guess.
    RepeatedHeading,
    /// A new or changed heading while sections of the old file are unaccounted for: it may be
    /// a renamed or moved section, whose earlier wording is unknown.
    RenamedOrMoved,
    /// The section is gone from the file. Deletions are not reviewed.
    Deleted,
}

/// A section that was not reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotReviewed {
    /// Its heading path (on the side where it exists).
    pub heading_path: Vec<String>,
    /// First line, on the after side when it exists there, otherwise on the before side.
    pub line_start: u32,
    /// Why.
    pub reason: Skip,
}

/// What comparing the two texts of one file gave.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FileReview {
    /// Changed and added sections, in document order of the after side.
    pub changes: Vec<SectionChange>,
    /// Sections left out, with the reason.
    pub not_reviewed: Vec<NotReviewed>,
    /// Sections identical on both sides.
    pub unchanged: usize,
}

/// Compare a file's text before and after. Both are parsed with `flavor`; line endings are
/// normalised by the parser, so a CRLF checkout against an LF blob is not a change.
#[must_use]
pub fn compare(before: &str, after: &str, flavor: Flavor) -> FileReview {
    let old = markdown::parse_str_as(before, flavor).sections;
    let new = markdown::parse_str_as(after, flavor).sections;
    let mut review = FileReview::default();

    // 1a. Unchanged in place: the same heading path and the same hash. Sections under
    // different parents can share a hash (the text does not include the parent), so the
    // heading path is matched first.
    let mut old_used = vec![false; old.len()];
    let mut new_used = vec![false; new.len()];
    for (ni, s) in new.iter().enumerate() {
        let twin = old.iter().enumerate().position(|(oi, o)| {
            !old_used[oi] && o.hash == s.hash && o.heading_path == s.heading_path
        });
        if let Some(oi) = twin {
            old_used[oi] = true;
            new_used[ni] = true;
            review.unchanged += 1;
        }
    }
    // 1b. Unchanged but moved: the same hash under another heading path, taken only when
    // neither side has a competing section on that path. Otherwise the old text stays
    // available to pair with its own heading path below (a section edited in place whose old
    // wording was also copied elsewhere is a change, not an addition).
    let path_left = |sections: &[Section], used: &[bool], path: &[String]| {
        sections.iter().zip(used).any(|(s, u)| !*u && s.heading_path == path)
    };
    for ni in 0..new.len() {
        if new_used[ni] {
            continue;
        }
        let s = &new[ni];
        let moved = old.iter().enumerate().position(|(oi, o)| {
            !old_used[oi]
                && o.hash == s.hash
                && !path_left(&new, &new_used, &o.heading_path)
                && !path_left(&old, &old_used, &s.heading_path)
        });
        if let Some(oi) = moved {
            old_used[oi] = true;
            new_used[ni] = true;
            review.unchanged += 1;
        }
    }
    let new_left: Vec<&Section> =
        new.iter().zip(&new_used).filter(|(_, u)| !**u).map(|(s, _)| s).collect();
    let old_left: Vec<&Section> =
        old.iter().zip(&old_used).filter(|(_, u)| !**u).map(|(s, _)| s).collect();

    // 2. Pair by a heading path that is unique on each whole side.
    let (old_count, new_count) = (count(&old), count(&new));
    let unique = |path: &[String]| {
        old_count.get(path).copied() == Some(1) && new_count.get(path).copied() == Some(1)
    };
    let mut paired = vec![false; old_left.len()];
    let mut unpaired_new: Vec<&Section> = Vec::new();
    for s in &new_left {
        let partner = unique(&s.heading_path)
            .then(|| old_left.iter().position(|o| o.heading_path == s.heading_path))
            .flatten();
        if let Some(i) = partner {
            paired[i] = true;
            review.changes.push(change(Some(old_left[i]), s));
        } else {
            unpaired_new.push(s);
        }
    }
    let unpaired_old: Vec<&Section> =
        old_left.iter().zip(&paired).filter(|(_, used)| !**used).map(|(s, _)| *s).collect();

    // 3 and 4. What is left.
    let repeated = |s: &Section| {
        old_count.get(s.heading_path.as_slice()).copied().unwrap_or(0) > 1
            || new_count.get(s.heading_path.as_slice()).copied().unwrap_or(0) > 1
    };
    for s in unpaired_new {
        if repeated(s) {
            review.not_reviewed.push(skip(s, Skip::RepeatedHeading));
        } else if unpaired_old.is_empty() {
            review.changes.push(change(None, s));
        } else {
            review.not_reviewed.push(skip(s, Skip::RenamedOrMoved));
        }
    }
    for s in unpaired_old {
        let reason = if repeated(s) { Skip::RepeatedHeading } else { Skip::Deleted };
        review.not_reviewed.push(skip(s, reason));
    }
    review.changes.sort_by_key(|c| c.after.line_start);
    review
}

/// How many sections carry each heading path.
fn count(sections: &[Section]) -> HashMap<&[String], usize> {
    let mut m: HashMap<&[String], usize> = HashMap::new();
    for s in sections {
        *m.entry(s.heading_path.as_slice()).or_default() += 1;
    }
    m
}

fn skip(s: &Section, reason: Skip) -> NotReviewed {
    NotReviewed { heading_path: s.heading_path.clone(), line_start: s.line_start, reason }
}

fn change(before: Option<&Section>, after: &Section) -> SectionChange {
    let (removed_lines, added_lines) = match before {
        Some(b) => (lines_only_in(&b.text, &after.text), lines_only_in(&after.text, &b.text)),
        None => (Vec::new(), lines_only_in(&after.text, "")),
    };
    SectionChange {
        heading_path: after.heading_path.clone(),
        before: before.map(Side::of),
        after: Side::of(after),
        removed_lines,
        added_lines,
    }
}

/// The trimmed, non-blank lines of `a` that `b` does not have, as a multiset difference, in
/// `a`'s order. Not a minimal diff: a moved line is in neither result, which is what the
/// search terms want.
fn lines_only_in(a: &str, b: &str) -> Vec<String> {
    let mut have: HashMap<&str, usize> = HashMap::new();
    for line in b.lines().map(str::trim).filter(|l| !l.is_empty()) {
        *have.entry(line).or_default() += 1;
    }
    let mut out = Vec::new();
    for line in a.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match have.get_mut(line) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(line.to_owned()),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::panic, reason = "test assertions")]
mod tests {
    use super::*;

    fn cmp(before: &str, after: &str) -> FileReview {
        compare(before, after, Flavor::Markdown)
    }

    const DOC: &str = "# Guide\n\nIntro.\n\n## Timeouts\n\nThe default timeout is 30 seconds.\nRetries: 3.\n\n## Auth\n\nUse a token.\n";

    #[test]
    fn an_edited_section_is_one_change_with_its_replaced_and_new_lines() {
        let after = DOC.replace("30 seconds", "60 seconds");
        let r = cmp(DOC, &after);
        assert_eq!(r.unchanged, 2);
        assert!(r.not_reviewed.is_empty());
        let [c] = r.changes.as_slice() else { panic!("{r:?}") };
        assert_eq!(c.heading_path, ["Guide", "Timeouts"]);
        assert_eq!(c.removed_lines, ["The default timeout is 30 seconds."]);
        assert_eq!(c.added_lines, ["The default timeout is 60 seconds."]);
        assert_eq!(c.before.as_ref().unwrap().line_start, 5);
        assert!(c.after.text.contains("60 seconds"));
    }

    #[test]
    fn identical_texts_and_crlf_against_lf_are_not_changes() {
        assert!(cmp(DOC, DOC).changes.is_empty());
        let r = cmp(DOC, &DOC.replace('\n', "\r\n"));
        assert!(r.changes.is_empty() && r.not_reviewed.is_empty(), "{r:?}");
        assert_eq!(r.unchanged, 3);
    }

    #[test]
    fn a_section_that_only_moved_is_unchanged_and_lines_shift() {
        let after = "# Guide\n\nIntro.\n\n## Auth\n\nUse a token.\n\n## Timeouts\n\nThe default timeout is 30 seconds.\nRetries: 3.\n";
        let r = cmp(DOC, after);
        assert!(r.changes.is_empty() && r.not_reviewed.is_empty(), "{r:?}");
    }

    #[test]
    fn a_new_section_in_an_otherwise_accounted_file_is_an_addition() {
        let after = format!("{DOC}\n## Limits\n\nAt most 10 requests per second.\n");
        let r = cmp(DOC, &after);
        let [c] = r.changes.as_slice() else { panic!("{r:?}") };
        assert!(c.before.is_none());
        assert!(c.removed_lines.is_empty());
        assert_eq!(c.added_lines, ["## Limits", "At most 10 requests per second."]);
    }

    #[test]
    fn a_renamed_heading_is_not_reviewed_and_never_an_addition() {
        let after =
            DOC.replace("## Timeouts", "## Time limits").replace("30 seconds", "60 seconds");
        let r = cmp(DOC, &after);
        assert!(r.changes.is_empty(), "the old wording is unknown, so nothing is claimed: {r:?}");
        let reasons: Vec<Skip> = r.not_reviewed.iter().map(|n| n.reason).collect();
        assert_eq!(reasons, [Skip::RenamedOrMoved, Skip::Deleted]);
    }

    #[test]
    fn repeated_headings_are_not_paired_by_position() {
        let before = "# Log\n\n## Entry\n\nfirst\n\n## Entry\n\nsecond\n";
        let after =
            "# Log\n\n## Entry\n\nzeroth\n\n## Entry\n\nfirst\n\n## Entry\n\nsecond, edited\n";
        let r = cmp(before, after);
        assert!(r.changes.is_empty(), "{r:?}");
        assert!(r.not_reviewed.iter().all(|n| n.reason == Skip::RepeatedHeading));
        assert_eq!(
            r.not_reviewed.len(),
            3,
            "two new texts and the one old text they could pair with"
        );
        assert_eq!(r.unchanged, 2);
    }

    #[test]
    fn identical_sections_under_different_parents_keep_their_identity() {
        let before = "# A\n\n## X\n\nsame\n\n# B\n\n## X\n\nsame\n";
        // The copy under A is removed: A/X (and A itself) are the deletions, B/X is untouched.
        let r = cmp(before, "# B\n\n## X\n\nsame\n");
        assert!(r.changes.is_empty(), "{r:?}");
        let gone: Vec<&[String]> =
            r.not_reviewed.iter().map(|n| n.heading_path.as_slice()).collect();
        assert!(gone.contains(&["A".to_owned(), "X".to_owned()].as_slice()), "{gone:?}");
        assert!(!gone.contains(&["B".to_owned(), "X".to_owned()].as_slice()), "{gone:?}");
        assert!(r.not_reviewed.iter().all(|n| n.reason == Skip::Deleted));
    }

    #[test]
    fn an_edited_section_whose_old_text_was_copied_elsewhere_is_still_a_change() {
        let before = "# A\n\n## X\n\nThe limit is 5.\n\n# B\n\nOther.\n";
        let after = "# A\n\n## X\n\nThe limit is 9.\n\n# B\n\nOther.\n\n## Y\n\nThe limit is 5.\n";
        let r = cmp(before, after);
        let edited = r.changes.iter().find(|c| c.heading_path == ["A", "X"]).unwrap();
        assert_eq!(edited.removed_lines, ["The limit is 5."], "the old wording stays with A/X");
        assert!(edited.before.is_some());
        assert!(
            r.changes.iter().all(|c| c.before.is_some() || c.heading_path == ["B", "Y"]),
            "{r:?}"
        );
    }

    #[test]
    fn a_deleted_section_is_listed_as_deleted() {
        let after = DOC.replace("## Auth\n\nUse a token.\n", "");
        let r = cmp(DOC, &after);
        assert!(r.changes.is_empty());
        assert_eq!(r.not_reviewed.len(), 1);
        assert_eq!(r.not_reviewed[0].reason, Skip::Deleted);
        assert_eq!(r.not_reviewed[0].heading_path, ["Guide", "Auth"]);
    }

    #[test]
    fn a_moved_line_inside_a_section_is_neither_removed_nor_added() {
        let after = DOC.replace(
            "The default timeout is 30 seconds.\nRetries: 3.",
            "Retries: 3.\nThe default timeout is 30 seconds.\nBackoff: 2x.",
        );
        let r = cmp(DOC, &after);
        let [c] = r.changes.as_slice() else { panic!("{r:?}") };
        assert!(c.removed_lines.is_empty());
        assert_eq!(c.added_lines, ["Backoff: 2x."]);
    }
}
