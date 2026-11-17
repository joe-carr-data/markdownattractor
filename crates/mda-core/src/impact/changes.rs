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

use std::collections::{HashMap, VecDeque};

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
    // heading path is matched first. Queues keep document order; every step is a map lookup.
    let mut old_used = vec![false; old.len()];
    let mut new_used = vec![false; new.len()];
    let mut in_place: HashMap<(&[String], &str), VecDeque<usize>> = HashMap::new();
    for (oi, o) in old.iter().enumerate() {
        in_place.entry((o.heading_path.as_slice(), o.hash.as_str())).or_default().push_back(oi);
    }
    for (ni, s) in new.iter().enumerate() {
        let twin = in_place
            .get_mut(&(s.heading_path.as_slice(), s.hash.as_str()))
            .and_then(VecDeque::pop_front);
        if let Some(oi) = twin {
            old_used[oi] = true;
            new_used[ni] = true;
            review.unchanged += 1;
        }
    }
    // 1b. Unchanged but moved: the same hash under another heading path, taken only when
    // neither side has an unmatched section competing for either path. Otherwise the old text
    // stays available to pair with its own heading path below (a section edited in place
    // whose old wording was also copied elsewhere is a change, not an addition).
    let mut old_open = open_paths(&old, &old_used);
    let mut new_open = open_paths(&new, &new_used);
    let mut by_hash: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for (oi, o) in old.iter().enumerate().filter(|(oi, _)| !old_used[*oi]) {
        by_hash.entry(o.hash.as_str()).or_default().push_back(oi);
    }
    for ni in 0..new.len() {
        if new_used[ni] {
            continue;
        }
        let s = &new[ni];
        let Some(&oi) = by_hash.get(s.hash.as_str()).and_then(VecDeque::front) else { continue };
        let o = &old[oi];
        let contested = new_open.get(o.heading_path.as_slice()).copied().unwrap_or(0) > 0
            || old_open.get(s.heading_path.as_slice()).copied().unwrap_or(0) > 0;
        if contested {
            continue;
        }
        if let Some(q) = by_hash.get_mut(s.hash.as_str()) {
            q.pop_front();
        }
        old_used[oi] = true;
        new_used[ni] = true;
        review.unchanged += 1;
        if let Some(n) = old_open.get_mut(o.heading_path.as_slice()) {
            *n = n.saturating_sub(1);
        }
        if let Some(n) = new_open.get_mut(s.heading_path.as_slice()) {
            *n = n.saturating_sub(1);
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
    let old_at: HashMap<&[String], usize> =
        old_left.iter().enumerate().map(|(i, o)| (o.heading_path.as_slice(), i)).collect();
    let mut paired = vec![false; old_left.len()];
    let mut unpaired_new: Vec<&Section> = Vec::new();
    for s in &new_left {
        let partner = unique(&s.heading_path)
            .then(|| old_at.get(s.heading_path.as_slice()).copied())
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

/// How many unmatched sections each heading path still has.
fn open_paths<'a>(sections: &'a [Section], used: &[bool]) -> HashMap<&'a [String], usize> {
    let mut m: HashMap<&[String], usize> = HashMap::new();
    for (s, _) in sections.iter().zip(used).filter(|(_, u)| !**u) {
        *m.entry(s.heading_path.as_slice()).or_default() += 1;
    }
    m
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

/// The non-blank lines of `a` that `b` does not have, as a multiset difference, in `a`'s
/// order. Lines are compared with their indentation (it carries meaning in code and nested
/// lists) and returned trimmed. Not a minimal diff: a line that only moved is in neither
/// result, which is what the search terms want.
fn lines_only_in(a: &str, b: &str) -> Vec<String> {
    let mut have: HashMap<&str, usize> = HashMap::new();
    for line in b.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        *have.entry(line).or_default() += 1;
    }
    let mut out = Vec::new();
    for line in a.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        match have.get_mut(line) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(line.trim().to_owned()),
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
        // The copy sits under the same heading text (`## X`) in another parent, so its hash
        // equals the old A/X hash: only the competition rule keeps the old text with A/X.
        let before = "# A\n\n## X\n\nThe limit is 5.\n\n# B\n\nOther.\n";
        let after = "# A\n\n## X\n\nThe limit is 9.\n\n# B\n\nOther.\n\n## X\n\nThe limit is 5.\n";
        let old_hash = markdown::parse_str_as(before, Flavor::Markdown).sections[1].hash.clone();
        let new_sections = markdown::parse_str_as(after, Flavor::Markdown).sections;
        assert_eq!(new_sections[3].hash, old_hash, "the fixture must collide");
        let r = cmp(before, after);
        let edited = r.changes.iter().find(|c| c.heading_path == ["A", "X"]).unwrap();
        assert_eq!(edited.removed_lines, ["The limit is 5."], "the old wording stays with A/X");
        assert_eq!(edited.added_lines, ["The limit is 9."]);
        let copy = r.changes.iter().find(|c| c.heading_path == ["B", "X"]).unwrap();
        assert!(copy.before.is_none(), "the copy is new text in the file");
        assert_eq!(r.changes.len(), 2, "{r:?}");
    }

    #[test]
    fn an_indentation_only_change_in_code_is_a_changed_line() {
        let before = "# P\n\n```python\nif enabled:\n    send()\n```\n";
        let after = "# P\n\n```python\nif enabled:\n    pass\nsend()\n```\n";
        let r = cmp(before, after);
        let [c] = r.changes.as_slice() else { panic!("{r:?}") };
        assert_eq!(c.removed_lines, ["send()"], "the indented call is gone");
        assert_eq!(c.added_lines, ["pass", "send()"], "and an unindented one is new");
    }

    #[test]
    fn a_large_identical_file_is_compared_in_linear_time() {
        use std::fmt::Write as _;
        let mut doc = String::new();
        for i in 0..20_000 {
            let _ = write!(doc, "# Section {i}\n\nText {i}\n\n");
        }
        let started = std::time::Instant::now();
        let r = cmp(&doc, &doc);
        assert_eq!(r.unchanged, 20_000);
        assert!(r.changes.is_empty() && r.not_reviewed.is_empty());
        // Parsing dominates; the quadratic matching this replaces took minutes at this size.
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
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
