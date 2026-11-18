//! Search terms from a change, the replaced wording first.
//!
//! A stale copy elsewhere still carries what the change removed, so tokens that are in the
//! removed lines and not in the added lines rank first. Tokens on both sides say what the
//! passage is about and come next; tokens only on the added side come last. Within a role,
//! identifiers and numbers (with their unit) rank above plain words. Nothing here touches the
//! store: rarity by document frequency is applied by the caller.

use std::collections::HashSet;

use serde::Serialize;

use super::changes::SectionChange;
use crate::markdown::{self, Piece};

/// What a term looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TermKind {
    /// A code span, a path, a version, a name with `_`, `-`, `.`, `/`, or mixed case.
    Identifier,
    /// A number, with the unit word that follows it when there is one (`30 seconds`).
    Number,
    /// A plain word of four letters or more that is not a stop word.
    Word,
}

/// Where a term stands in the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// In the removed lines only: the wording that was replaced.
    Replaced,
    /// On both sides: what the passage is about.
    Context,
    /// In the added lines only.
    Added,
}

/// One search term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Term {
    /// The term as written (a number term may hold a space: `30 seconds`).
    pub text: String,
    /// What it looks like.
    pub kind: TermKind,
    /// Where it stands in the change.
    pub role: Role,
}

const STOP: &[&str] = &[
    "about", "after", "also", "been", "before", "between", "both", "does", "each", "from", "have",
    "here", "into", "just", "like", "more", "most", "must", "need", "only", "other", "over",
    "same", "should", "some", "such", "than", "that", "their", "them", "then", "there", "these",
    "they", "this", "those", "through", "used", "uses", "using", "very", "what", "when", "where",
    "which", "while", "will", "with", "would", "your",
];

/// The terms of a change, best first, without duplicates (case-insensitive). For an addition
/// every term has the role [`Role::Added`].
#[must_use]
pub fn terms_for(change: &SectionChange) -> Vec<Term> {
    let removed = extract(&change.removed_lines);
    let added = extract(&change.added_lines);
    let key = |t: &(String, TermKind)| t.0.to_lowercase();
    let removed_keys: HashSet<String> = removed.iter().map(key).collect();
    let added_keys: HashSet<String> = added.iter().map(key).collect();

    let mut terms: Vec<Term> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |text: &str, kind: TermKind, role: Role| {
        if seen.insert(text.to_lowercase()) {
            terms.push(Term { text: text.to_owned(), kind, role });
        }
    };
    for t in &removed {
        let role = if added_keys.contains(&key(t)) { Role::Context } else { Role::Replaced };
        push(&t.0, t.1, role);
    }
    for t in &added {
        if !removed_keys.contains(&key(t)) {
            push(&t.0, t.1, Role::Added);
        }
    }
    // Stable: within a role and kind the order of appearance is kept.
    terms.sort_by_key(|t| (t.role, t.kind));
    terms
}

/// Tokens of some lines with their kind, in order of appearance. Each line is read by the
/// markdown parser, so a code span of any delimiter length and a link destination are known
/// for what they are instead of being guessed from punctuation.
fn extract(lines: &[String]) -> Vec<(String, TermKind)> {
    let mut out = Vec::new();
    for line in lines {
        for (piece, what) in markdown::inline_pieces(line) {
            match what {
                // A code span is an identifier whatever its length or spelling: `on`, `from`.
                Piece::Code => {
                    // Any spelling counts, symbols included: the index says whether it is
                    // found anywhere.
                    for t in piece.split_whitespace().filter(|t| t.chars().count() <= 80) {
                        out.push((t.to_owned(), TermKind::Identifier));
                    }
                }
                Piece::LinkTarget => {
                    let url = piece.trim();
                    if usable(url) && !url.contains(char::is_whitespace) {
                        out.push((url.to_owned(), TermKind::Identifier));
                    }
                }
                Piece::Text => text_tokens(&piece, &mut out),
            }
        }
    }
    out
}

/// Tokens of running text. Table bars separate cells; a call such as `fn()` stays whole.
fn text_tokens(text: &str, out: &mut Vec<(String, TermKind)>) {
    let spaced = text.replace('|', " ");
    let tokens: Vec<&str> =
        spaced.split_whitespace().map(clean).filter(|t| !t.is_empty()).collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(kind) = classify(t) {
            if kind == TermKind::Number
                && let Some(unit) = tokens.get(i + 1).filter(|u| is_unit(u))
            {
                out.push((format!("{t} {unit}"), kind));
                i += 2;
                continue;
            }
            out.push((t.to_owned(), kind));
        }
        i += 1;
    }
}

/// [`strip`], except that a token with a name before an opening parenthesis (`fn()`,
/// `get(id)`) keeps its parentheses and loses only sentence punctuation at its end.
fn clean(token: &str) -> &str {
    let call = token.find('(').is_some_and(|i| token[..i].chars().any(char::is_alphanumeric));
    if call && token.contains(')') {
        token
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .trim_start_matches(['(', '[', '"', '\''])
    } else {
        strip(token)
    }
}

/// A token short enough to search for that holds a letter or a digit.
fn usable(t: &str) -> bool {
    t.chars().count() <= 80 && t.chars().any(char::is_alphanumeric)
}

/// Remove markdown and sentence punctuation around a token, keeping what is inside it.
fn strip(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '`' | '*'
                | '_'
                | '"'
                | '\''
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | '.'
                | '|'
                | '#'
                | '='
                | '~'
        )
    })
}

fn is_unit(word: &str) -> bool {
    (2..=12).contains(&word.chars().count()) && word.chars().all(char::is_alphabetic)
}

fn classify(t: &str) -> Option<TermKind> {
    let chars = t.chars().count();
    if !usable(t) {
        return None;
    }
    let first = t.chars().next()?;
    if first.is_ascii_digit()
        && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '%'))
    {
        return Some(TermKind::Number);
    }
    if t.contains('(') && t.contains(')') {
        return Some(TermKind::Identifier);
    }
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    let joined = t.chars().any(|c| matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '\\' | '$'));
    let inner_upper =
        t.chars().skip(1).any(char::is_uppercase) && t.chars().any(char::is_lowercase);
    let all_caps = chars >= 2 && t.chars().all(|c| c.is_uppercase() || c.is_ascii_digit());
    if chars >= 2 && (has_digit || joined || inner_upper || all_caps) {
        return Some(TermKind::Identifier);
    }
    let word = t.chars().all(char::is_alphabetic) && chars >= 4;
    (word && !STOP.contains(&t.to_lowercase().as_str())).then_some(TermKind::Word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impact::changes::compare;
    use crate::markdown::Flavor;

    fn terms(before: &str, after: &str) -> Vec<Term> {
        let r = compare(before, after, Flavor::Markdown);
        assert_eq!(r.changes.len(), 1, "{r:?}");
        terms_for(&r.changes[0])
    }

    fn texts(ts: &[Term], role: Role) -> Vec<&str> {
        ts.iter().filter(|t| t.role == role).map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn the_replaced_value_ranks_first_then_what_the_passage_is_about() {
        let ts = terms(
            "# T\n\nThe default timeout is 30 seconds for `fetchUser`.\n",
            "# T\n\nThe default timeout is 60 seconds for `fetchUser`.\n",
        );
        assert_eq!(texts(&ts, Role::Replaced), ["30 seconds"]);
        assert_eq!(texts(&ts, Role::Added), ["60 seconds"]);
        assert_eq!(texts(&ts, Role::Context), ["fetchUser", "default", "timeout"]);
        assert_eq!(ts[0].text, "30 seconds", "the wording a stale copy still carries comes first");
        assert_eq!(ts.last().unwrap().role, Role::Added);
    }

    #[test]
    fn identifiers_of_every_shape_are_recognised_and_punctuation_is_stripped() {
        let ts = terms(
            "# T\n\nEdit `svelte.config.js`, set MAX_RETRIES, call pnpm/setup@v2 (see docs/guide.md).\n",
            "# T\n\nNothing.\n",
        );
        let got = texts(&ts, Role::Replaced);
        for want in ["svelte.config.js", "MAX_RETRIES", "pnpm/setup@v2", "docs/guide.md"] {
            assert!(got.contains(&want), "{want} missing from {got:?}");
        }
        assert!(ts.iter().all(|t| !t.text.contains('`') && !t.text.ends_with(')')));
        let kinds: Vec<TermKind> =
            ts.iter().filter(|t| t.role == Role::Replaced).map(|t| t.kind).collect();
        assert!(kinds.windows(2).all(|w| w[0] <= w[1]), "identifiers before words: {kinds:?}");
    }

    #[test]
    fn table_cells_and_links_are_split_before_classification() {
        let ts = terms("# T\n\n|timeout|30|seconds|\n", "# T\n\n|timeout|60|seconds|\n");
        assert_eq!(texts(&ts, Role::Replaced), ["30 seconds"]);
        assert_eq!(texts(&ts, Role::Context), ["timeout"]);

        let ts = terms(
            "# T\n\nSee the [guide](docs/v2.md) first.\n",
            "# T\n\nSee the [guide](docs/v3.md) first.\n",
        );
        assert_eq!(texts(&ts, Role::Replaced), ["docs/v2.md"]);
        assert!(texts(&ts, Role::Context).contains(&"guide"));
    }

    #[test]
    fn a_code_span_is_an_identifier_however_short_or_common() {
        let ts = terms("# Config\n\nUse `on`.\n", "# Config\n\nUse `off`.\n");
        assert_eq!(texts(&ts, Role::Replaced), ["on"]);
        assert_eq!(texts(&ts, Role::Added), ["off"]);
        let ts = terms("# Q\n\nSelect `from` the table with `a b`.\n", "# Q\n\nNothing.\n");
        let got = texts(&ts, Role::Replaced);
        assert!(got.contains(&"from") && got.contains(&"a") && got.contains(&"b"), "{got:?}");
        // A stray backtick opens nothing: the line is plain text.
        let ts = terms("# Q\n\nA stray ` and on.\n", "# Q\n\nNothing.\n");
        assert!(!texts(&ts, Role::Replaced).contains(&"on"));
    }

    #[test]
    fn code_spans_of_any_delimiter_calls_and_urls_with_parentheses_keep_the_replaced_value() {
        let ts = terms("# C\n\nUse ``on``.\n", "# C\n\nUse ``off``.\n");
        assert_eq!(texts(&ts, Role::Replaced), ["on"], "a double-backtick span");
        let ts = terms("# C\n\nA literal \\` then `on`.\n", "# C\n\nA literal \\` then `off`.\n");
        assert_eq!(texts(&ts, Role::Replaced), ["on"], "an escaped backtick does not open a span");
        let ts = terms("# C\n\nCall fn() first.\n", "# C\n\nCall go() first.\n");
        assert_eq!(texts(&ts, Role::Replaced), ["fn()"]);
        assert_eq!(texts(&ts, Role::Added), ["go()"]);
        let ts = terms(
            "# C\n\nSee [guide](https://example.test/a(on)).\n",
            "# C\n\nSee [guide](https://example.test/a(off)).\n",
        );
        assert_eq!(texts(&ts, Role::Replaced), ["https://example.test/a(on)"]);
        let ts = terms("# C\n\nRun `fn()` now.\n", "# C\n\nRun `go()` now.\n");
        assert_eq!(texts(&ts, Role::Replaced), ["fn()"]);
    }

    #[test]
    fn stop_words_short_words_and_bare_punctuation_are_not_terms() {
        let ts =
            terms("# T\n\nThis is what they would use with it --- or not.\n", "# T\n\nGone.\n");
        assert!(texts(&ts, Role::Replaced).is_empty(), "{ts:?}");
    }

    #[test]
    fn an_addition_has_only_added_terms_and_duplicates_collapse() {
        let r = compare(
            "# T\n\nA.\n",
            "# T\n\nA.\n\n## Limits\n\nThe limit is 10 requests. The Limit applies per key.\n",
            Flavor::Markdown,
        );
        let ts = terms_for(&r.changes[0]);
        assert!(ts.iter().all(|t| t.role == Role::Added));
        assert_eq!(ts.iter().filter(|t| t.text.eq_ignore_ascii_case("limit")).count(), 1);
        assert!(ts.iter().any(|t| t.text == "10 requests"));
    }
}
