//! Hybrid retrieval over the store.
//!
//! Two BM25 lists (cards, raw section text) are fused with reciprocal rank fusion, then
//! re-weighted by recency so that, all else equal, what changed last week outranks what
//! changed last year. Filters on time and path are applied after fusion, on the stored rows.
//!
//! No vectors yet: that is Phase 2, and it slots in as a third ranked list.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::store::{FtsHit, SectionState, Store, StoredSection, fts_escape};

/// RRF constant. 60 is the value from Cormack et al. and works well for short lists.
const RRF_K: f64 = 60.0;

/// Which index produced a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Matched {
    /// Only the card fields matched.
    Cards,
    /// Only the raw section text matched.
    Raw,
    /// Both.
    Both,
}

/// Search parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Number of hits to return.
    pub k: usize,
    /// Search only the raw section text (skip cards). The recovery path in the skill.
    pub raw_only: bool,
    /// Keep only sections updated at or after this time.
    pub since: Option<Timestamp>,
    /// Keep only sections updated at or before this time.
    pub until: Option<Timestamp>,
    /// Keep only documents whose relative path starts with this prefix.
    pub path_prefix: Option<String>,
    /// Half-life of the recency bonus, in days. `0` disables recency.
    pub recency_half_life_days: f64,
    /// Fall back to an OR query when the AND query returns nothing.
    pub or_fallback: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            k: 8,
            raw_only: false,
            since: None,
            until: None,
            path_prefix: None,
            recency_half_life_days: 30.0,
            or_fallback: true,
        }
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    /// `<doc_id>#<index>`.
    pub section_id: String,
    /// Path relative to the root.
    pub rel_path: String,
    /// Document title, if any.
    pub title: Option<String>,
    /// Headings down to the section.
    pub heading_path: Vec<String>,
    /// First line, 1-based.
    pub line_start: u32,
    /// Last line, 1-based, inclusive.
    pub line_end: u32,
    /// Estimated tokens of the section.
    pub token_estimate: u32,
    /// The card's one-liner, when the section has a card.
    pub tldr: Option<String>,
    /// First ~200 characters of the section body (after the heading), for pending sections
    /// and for showing what matched.
    pub snippet: String,
    /// Fused, recency-adjusted score. Higher is better; only comparable within one query.
    pub score: f64,
    /// Which index matched.
    pub matched: Matched,
    /// `true` when the section has no card yet.
    pub pending: bool,
    /// When the section content last changed.
    pub updated_at: Timestamp,
    /// Whether the AND query had to fall back to OR.
    pub via_or_fallback: bool,
}

/// Run a hybrid search.
pub fn search(store: &Store, query: &str, opts: &SearchOptions) -> Result<Vec<Hit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Filters discard candidates after ranking, so fetch deeper when any filter is set.
    let filtered = opts.since.is_some() || opts.until.is_some() || opts.path_prefix.is_some();
    let fetch =
        if filtered { opts.k.saturating_mul(4).max(200) } else { opts.k.saturating_mul(4).max(16) };

    let (hits, via_or) = {
        let and_expr = fts_escape(query);
        let hits = run(store, &and_expr, fetch, opts)?;
        if hits.is_empty() && opts.or_fallback {
            let or_expr = or_expression(query);
            if or_expr != and_expr && !or_expr.is_empty() {
                (run(store, &or_expr, fetch, opts)?, true)
            } else {
                (hits, false)
            }
        } else {
            (hits, false)
        }
    };

    let now = Timestamp::now();
    let mut scored: Vec<Hit> = hits
        .into_iter()
        .map(|(section, matched, rrf)| {
            let fused = rrf * recency_factor(section.updated_at, now, opts.recency_half_life_days);
            to_hit(section, matched, fused, via_or)
        })
        .collect();

    scored
        .sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.section_id.cmp(&b.section_id)));
    scored.truncate(opts.k);
    Ok(scored)
}

/// Query both indexes and fuse. Returns sections with their match kind and RRF score.
fn run(
    store: &Store,
    expr: &str,
    fetch: usize,
    opts: &SearchOptions,
) -> Result<Vec<(StoredSection, Matched, f64)>> {
    let raw = store.search_raw(expr, fetch)?;
    let cards = if opts.raw_only { Vec::new() } else { store.search_cards(expr, fetch)? };

    let mut fused: std::collections::HashMap<String, (f64, bool, bool)> =
        std::collections::HashMap::new();
    for (rank, h) in cards.iter().enumerate() {
        let e = fused.entry(h.section_id.clone()).or_insert((0.0, false, false));
        e.0 += rrf(rank);
        e.1 = true;
    }
    for (rank, h) in raw.iter().enumerate() {
        let e = fused.entry(h.section_id.clone()).or_insert((0.0, false, false));
        e.0 += rrf(rank);
        e.2 = true;
    }

    let mut out = Vec::with_capacity(fused.len());
    for (id, (score, in_cards, in_raw)) in fused {
        let Some(section) = store.section(&id)? else { continue };
        if !passes(&section, opts) {
            continue;
        }
        let matched = match (in_cards, in_raw) {
            (true, true) => Matched::Both,
            (true, false) => Matched::Cards,
            _ => Matched::Raw,
        };
        out.push((section, matched, score));
    }
    Ok(out)
}

/// Time and path filters, applied to candidates before fusion results are cut to `k` and
/// before the OR-fallback decision, so an excluded high-ranked hit never hides an eligible one.
fn passes(section: &StoredSection, opts: &SearchOptions) -> bool {
    if let Some(since) = opts.since
        && section.updated_at < since
    {
        return false;
    }
    if let Some(until) = opts.until
        && section.updated_at > until
    {
        return false;
    }
    if let Some(prefix) = &opts.path_prefix
        && !section.rel_path.starts_with(prefix.trim_start_matches("./"))
    {
        return false;
    }
    true
}

#[allow(clippy::cast_precision_loss)] // ranks are tiny
fn rrf(rank: usize) -> f64 {
    1.0 / (RRF_K + rank as f64 + 1.0)
}

/// Exponential decay on age: 1.0 now, 0.5 after one half-life, floor at 0.5 so old but
/// relevant sections are never buried.
#[allow(clippy::cast_precision_loss)] // seconds since epoch fit comfortably in f64
fn recency_factor(updated_at: Timestamp, now: Timestamp, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let age_days = (now.as_second() - updated_at.as_second()).max(0) as f64 / 86_400.0;
    let decay = (-age_days * std::f64::consts::LN_2 / half_life_days).exp();
    0.5 + 0.5 * decay
}

/// `"term1" OR "term2" …` for the fallback query.
fn or_expression(query: &str) -> String {
    query
        .split_whitespace()
        .map(fts_escape)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn to_hit(section: StoredSection, matched: Matched, score: f64, via_or: bool) -> Hit {
    let pending = section.state != SectionState::Summarized;
    let tldr = section.summary.as_ref().map(|s| s.tldr.clone());
    Hit {
        section_id: section.section_id,
        rel_path: section.rel_path,
        title: None,
        heading_path: section.heading_path,
        line_start: section.line_start,
        line_end: section.line_end,
        token_estimate: section.token_estimate,
        tldr,
        snippet: snippet_of(&section.text),
        score,
        matched,
        pending,
        updated_at: section.updated_at,
        via_or_fallback: via_or,
    }
}

/// Body text after the heading line, whitespace-collapsed, cut to ~200 chars.
fn snippet_of(text: &str) -> String {
    let body: String = text
        .lines()
        .skip_while(|l| l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: String = body.chars().take(200).collect();
    if body.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// Tell the caller which FTS hits exist for a query without fusing (used by `mda explain`).
pub fn explain(store: &Store, query: &str, k: usize) -> Result<(Vec<FtsHit>, Vec<FtsHit>)> {
    let expr = fts_escape(query.trim());
    Ok((store.search_cards(&expr, k)?, store.search_raw(&expr, k)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_is_one_now_and_half_plus_at_one_half_life() {
        let now = Timestamp::now();
        assert!((recency_factor(now, now, 30.0) - 1.0).abs() < 1e-9);
        let month_ago = now - jiff::SignedDuration::from_hours(24 * 30);
        assert!((recency_factor(month_ago, now, 30.0) - 0.75).abs() < 0.01);
        let ancient = now - jiff::SignedDuration::from_hours(24 * 3650);
        assert!(recency_factor(ancient, now, 30.0) >= 0.5);
        assert!((recency_factor(ancient, now, 0.0) - 1.0).abs() < 1e-9, "disabled");
    }

    #[test]
    fn rrf_decreases_with_rank() {
        assert!(rrf(0) > rrf(1) && rrf(1) > rrf(10));
    }

    #[test]
    fn snippet_skips_heading_and_collapses() {
        let s = snippet_of("## Title\n\nfirst line\n  second   line\n");
        assert_eq!(s, "first line second line");
        let long = format!("# H\n{}", "x".repeat(500));
        assert!(snippet_of(&long).ends_with('…'));
        assert_eq!(snippet_of(&long).chars().count(), 201);
    }

    #[test]
    fn or_expression_joins_escaped_terms() {
        let e = or_expression("roll back deploy");
        assert!(e.contains(" OR "));
        assert_eq!(e.matches(" OR ").count(), 2);
    }
}
