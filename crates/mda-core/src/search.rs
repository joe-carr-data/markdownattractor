//! Hybrid retrieval over the store.
//!
//! Two BM25 lists (cards, raw section text) are fused with reciprocal rank fusion, then
//! re-weighted by recency so that, all else equal, what changed last week outranks what
//! changed last year. Filters on time and path are applied after fusion, on the stored rows.
//!
//! Card embeddings are the third list (ADR-0004): when an [`Embedder`] is available and its
//! model is ready, the query is embedded and the [`VectorIndex`] contributes its top hits to
//! the same fusion. Search never waits for a model download; without a ready embedder the
//! result is lexical only.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::embed::{Embedder, dot};
use crate::store::{FtsHit, SectionState, Store, StoredSection, VectorSet, fts_escape};

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
    /// Neither lexical index matched; only the card vector did.
    Vector,
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
    /// Use the vector list when an embedder is available. `raw_only` implies `false`.
    pub vectors: bool,
    /// Reciprocal-rank-fusion constant (`1 / (k + rank)`); 60 by default.
    pub rrf_k: f64,
    /// Weight of the raw-text list in the fusion (cards and vectors weigh 1.0); 1.0 by default.
    pub raw_list_weight: f64,
    /// BM25 weight of the cards index's `questions_answered` column; 2.0 by default.
    pub questions_weight: f64,
    /// Drop English stop-words from the AND form of the query (never from the OR fallback).
    pub and_stopwords: bool,
}

impl SearchOptions {
    /// The defaults with the tunables a root's `config.toml` sets (benchmark tuning, plan §3).
    #[must_use]
    pub fn for_config(cfg: &crate::config::Config) -> Self {
        Self {
            rrf_k: if cfg.search_rrf_k > 0.0 { cfg.search_rrf_k } else { RRF_K },
            raw_list_weight: cfg.search_raw_weight.max(0.0),
            questions_weight: cfg.search_questions_weight.max(0.0),
            and_stopwords: cfg.search_and_stopwords,
            ..Self::default()
        }
    }
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
            vectors: true,
            rrf_k: RRF_K,
            raw_list_weight: 1.0,
            questions_weight: 2.0,
            and_stopwords: false,
        }
    }
}

/// Every vector of one model plus the live section ids each hash currently maps to, loaded
/// once per query (or kept around by long-lived callers) and scanned by dot product.
#[derive(Debug, Clone)]
pub struct VectorIndex {
    set: VectorSet,
    ids: Vec<Vec<String>>,
}

impl VectorIndex {
    /// Load the vectors of `model`. `None` when there are none.
    pub fn load(store: &Store, model: &str) -> Result<Option<Self>> {
        let set = store.vector_set(model)?;
        if set.is_empty() {
            return Ok(None);
        }
        let hashes: Vec<&str> = set.hashes.iter().map(String::as_str).collect();
        let mut by_hash = store.section_ids_by_hashes(&hashes)?;
        let ids = set.hashes.iter().map(|h| by_hash.remove(h).unwrap_or_default()).collect();
        Ok(Some(Self { set, ids }))
    }

    /// Number of vectors.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// `true` when empty (never, for a loaded index).
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Model the vectors came from.
    pub fn model(&self) -> &str {
        &self.set.model
    }

    /// The `k` best sections for a unit-length `query`, best first, as `(section_id, cosine)`.
    /// A hash shared by several live sections yields one entry per section.
    pub fn top_k(&self, query: &[f32], k: usize) -> Vec<(String, f32)> {
        if query.len() != self.set.dim || k == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(f32, usize)> =
            (0..self.set.len()).map(|i| (dot(self.set.row(i), query), i)).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let mut out = Vec::with_capacity(k);
        for (score, i) in scored {
            for id in &self.ids[i] {
                out.push((id.clone(), score));
                if out.len() == k {
                    return out;
                }
            }
        }
        out
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
    /// `true` when the card vector was among the vector hits for this query.
    pub vector: bool,
    /// Cosine similarity between the query and the card vector, when `vector` is set.
    pub vector_score: Option<f32>,
    /// `true` when the section has no card yet.
    pub pending: bool,
    /// When the section content last changed.
    pub updated_at: Timestamp,
    /// Whether the AND query had to fall back to OR.
    pub via_or_fallback: bool,
}

/// Run a lexical-only hybrid search (cards + raw text). See [`search_with`] for vectors.
pub fn search(store: &Store, query: &str, opts: &SearchOptions) -> Result<Vec<Hit>> {
    search_with(store, query, opts, None)
}

/// Run a hybrid search. With an `embedder` whose model is ready and `opts.vectors` set, the
/// card vectors join the fusion as a third list; otherwise the search is lexical.
pub fn search_with(
    store: &Store,
    query: &str,
    opts: &SearchOptions,
    embedder: Option<&dyn Embedder>,
) -> Result<Vec<Hit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Filters discard candidates after ranking, so fetch deeper when any filter is set.
    let filtered = opts.since.is_some() || opts.until.is_some() || opts.path_prefix.is_some();
    let fetch =
        if filtered { opts.k.saturating_mul(4).max(200) } else { opts.k.saturating_mul(4).max(16) };

    let vector_hits = vector_list(store, query, fetch, opts, embedder)?;
    let (lexical, via_or) = lexical_lists(store, query, fetch, opts)?;
    let fused = fuse(store, &lexical, &vector_hits, opts)?;

    let now = Timestamp::now();
    let mut scored: Vec<Hit> = fused
        .into_iter()
        .map(|(section, matched, vector_score, rrf)| {
            let fused = rrf * recency_factor(section.updated_at, now, opts.recency_half_life_days);
            to_hit(section, matched, vector_score, fused, via_or)
        })
        .collect();

    scored
        .sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.section_id.cmp(&b.section_id)));
    scored.truncate(opts.k);
    Ok(scored)
}

/// The two BM25 lists for a query, with the OR fallback applied when the AND form is empty.
struct Lexical {
    cards: Vec<FtsHit>,
    raw: Vec<FtsHit>,
}

fn lexical_lists(
    store: &Store,
    query: &str,
    fetch: usize,
    opts: &SearchOptions,
) -> Result<(Lexical, bool)> {
    let and_expr =
        if opts.and_stopwords { fts_escape(&without_stopwords(query)) } else { fts_escape(query) };
    let lists = |expr: &str| -> Result<Lexical> {
        Ok(Lexical {
            raw: store.search_raw(expr, fetch)?,
            cards: if opts.raw_only {
                Vec::new()
            } else {
                store.search_cards_weighted(expr, fetch, opts.questions_weight)?
            },
        })
    };
    let first = lists(&and_expr)?;
    // Fall back when nothing *eligible* matched: an AND hit outside the time or path filter
    // must not hide OR hits inside it.
    let any_eligible = first
        .cards
        .iter()
        .chain(&first.raw)
        .map(|h| h.section_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .try_fold(false, |found, id| -> Result<bool> {
            Ok(found || store.section(id)?.is_some_and(|s| passes(&s, opts)))
        })?;
    if !any_eligible && opts.or_fallback {
        let or_expr = or_expression(query);
        if or_expr != and_expr && !or_expr.is_empty() {
            return Ok((lists(&or_expr)?, true));
        }
    }
    Ok((first, false))
}

/// The vector list: empty when vectors are off, no embedder is given, the model is not ready
/// (never download inside a query), or nothing is embedded yet.
fn vector_list(
    store: &Store,
    query: &str,
    fetch: usize,
    opts: &SearchOptions,
    embedder: Option<&dyn Embedder>,
) -> Result<Vec<(String, f32)>> {
    if !opts.vectors || opts.raw_only {
        return Ok(Vec::new());
    }
    let Some(embedder) = embedder else { return Ok(Vec::new()) };
    if !embedder.ready() {
        tracing::debug!(model = embedder.model(), "embedding model not ready; lexical only");
        return Ok(Vec::new());
    }
    let Some(index) = VectorIndex::load(store, embedder.model())? else { return Ok(Vec::new()) };
    // A broken model must not take lexical search down with it: log, answer lexical.
    let mut q = match embedder.embed(&[query.to_owned()]) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "query embedding failed; lexical only");
            return Ok(Vec::new());
        }
    };
    let Some(qv) = q.pop() else { return Ok(Vec::new()) };
    Ok(index.top_k(&qv, fetch))
}

type Fused = Vec<(StoredSection, Matched, Option<f32>, f64)>;

/// Reciprocal rank fusion of the three lists, then the time and path filters.
fn fuse(
    store: &Store,
    lexical: &Lexical,
    vector: &[(String, f32)],
    opts: &SearchOptions,
) -> Result<Fused> {
    struct Acc {
        score: f64,
        cards: bool,
        raw: bool,
        vector: Option<f32>,
    }
    fn entry<'a>(map: &'a mut std::collections::HashMap<String, Acc>, id: &str) -> &'a mut Acc {
        map.entry(id.to_owned()).or_insert(Acc {
            score: 0.0,
            cards: false,
            raw: false,
            vector: None,
        })
    }
    let mut fused: std::collections::HashMap<String, Acc> = std::collections::HashMap::new();
    for (rank, h) in lexical.cards.iter().enumerate() {
        let e = entry(&mut fused, &h.section_id);
        e.score += rrf(rank, opts.rrf_k);
        e.cards = true;
    }
    for (rank, h) in lexical.raw.iter().enumerate() {
        let e = entry(&mut fused, &h.section_id);
        e.score += rrf(rank, opts.rrf_k) * opts.raw_list_weight;
        e.raw = true;
    }
    for (rank, (id, cosine)) in vector.iter().enumerate() {
        let e = entry(&mut fused, id);
        e.score += rrf(rank, opts.rrf_k);
        e.vector = Some(*cosine);
    }

    let mut out = Vec::with_capacity(fused.len());
    for (id, acc) in fused {
        let Some(section) = store.section(&id)? else { continue };
        if !passes(&section, opts) {
            continue;
        }
        let matched = match (acc.cards, acc.raw, acc.vector.is_some()) {
            (true, true, _) => Matched::Both,
            (true, false, _) => Matched::Cards,
            (false, true, _) => Matched::Raw,
            (false, false, _) => Matched::Vector,
        };
        out.push((section, matched, acc.vector, acc.score));
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
fn rrf(rank: usize, k: f64) -> f64 {
    1.0 / (k + rank as f64 + 1.0)
}

/// English stop-words dropped from the AND form when `and_stopwords` is set (plan §3
/// candidate 7): the long community questions are mostly function words, and an AND over
/// all of them almost never matches. A query made only of stop-words is left as it is.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do", "does", "for", "from",
    "how", "i", "if", "in", "into", "is", "it", "its", "my", "of", "on", "or", "that", "the",
    "this", "to", "was", "we", "what", "when", "where", "which", "why", "with", "you", "your",
];

fn without_stopwords(query: &str) -> String {
    let kept: Vec<&str> = query
        .split_whitespace()
        .filter(|w| {
            !STOPWORDS
                .contains(&w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase().as_str())
        })
        .collect();
    if kept.is_empty() { query.to_owned() } else { kept.join(" ") }
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

fn to_hit(
    section: StoredSection,
    matched: Matched,
    vector_score: Option<f32>,
    score: f64,
    via_or: bool,
) -> Hit {
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
        vector: vector_score.is_some(),
        vector_score,
        pending,
        updated_at: section.updated_at,
        via_or_fallback: via_or,
    }
}

/// Body text after the heading line, whitespace-collapsed, cut to ~200 chars. Lines that
/// carry no prose (a lone JSX/HTML tag, the attribute lines of a tag spread over several
/// lines, an MDX `import`/`export` statement) are skipped so the snippet shows text, not
/// markup; a line with words outside its tags is kept whole.
fn snippet_of(text: &str) -> String {
    let mut in_tag = false;
    let mut words: Vec<&str> = Vec::new();
    for line in text.lines().skip_while(|l| l.trim_start().starts_with('#')) {
        let mut t = line.trim();
        if in_tag {
            // The tag ends at its first `>`; whatever follows on the line is prose.
            let Some(end) = t.find('>') else { continue };
            in_tag = false;
            t = t[end + 1..].trim();
        }
        if opens_tag(t) && !t.contains('>') {
            in_tag = true;
            continue;
        }
        if !t.is_empty() && !is_markup_only(t) {
            words.extend(t.split_whitespace());
        }
    }
    let body = words.join(" ");
    let mut out: String = body.chars().take(200).collect();
    if body.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// `<Tag`, `</Tag` or `<!--`: the start of markup, as opposed to `<` in prose ("a < b").
fn opens_tag(t: &str) -> bool {
    t.starts_with('<')
        && t[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!')
}

/// A trimmed line that carries no prose: nothing but tags (`<Tabs>`, `</TabPanel>
/// </Tabs>`, `<Figure />`), an MDX `import`/`export` statement, or a bare JSX brace.
fn is_markup_only(t: &str) -> bool {
    if t.starts_with("import ") || t.starts_with("export ") || t == "{" || t == "}" {
        return true;
    }
    if !opens_tag(t) {
        return false;
    }
    let mut depth = 0u32;
    t.chars().all(|c| match c {
        '<' => {
            depth = depth.saturating_add(1);
            true
        }
        '>' => {
            depth = depth.saturating_sub(1);
            true
        }
        _ => depth > 0 || c.is_whitespace(),
    })
}

/// The three ranked lists behind a query and the fused result, for `mda explain`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Explain {
    /// BM25 hits on the cards index, best first.
    pub cards: Vec<FtsHit>,
    /// BM25 hits on the raw-text index, best first.
    pub raw: Vec<FtsHit>,
    /// Vector hits as `(section_id, cosine)`, best first; empty when vectors did not run.
    pub vector: Vec<(String, f32)>,
    /// Why the vector list is empty, when it is.
    pub vector_note: Option<String>,
    /// The lexical lists come from the OR form of the query because the AND form was empty.
    pub via_or_fallback: bool,
    /// The fused, recency-weighted, filtered result exactly as `search_with` returns it.
    pub fused: Vec<Hit>,
}

/// Show every list behind a query. The lexical lists are the ones the search used: the AND
/// form, or the OR form when the AND form matched nothing (`via_or_fallback`).
pub fn explain(
    store: &Store,
    query: &str,
    opts: &SearchOptions,
    embedder: Option<&dyn Embedder>,
) -> Result<Explain> {
    let query = query.trim();
    // Same candidate depth as the search itself, so every fused winner is visible in the
    // list that produced it.
    let filtered = opts.since.is_some() || opts.until.is_some() || opts.path_prefix.is_some();
    let k =
        if filtered { opts.k.saturating_mul(4).max(200) } else { opts.k.saturating_mul(4).max(16) };
    let (lexical, via_or_fallback) = lexical_lists(store, query, k, opts)?;
    let vector_note = match embedder {
        _ if !opts.vectors => Some("vectors disabled for this query".to_owned()),
        None => Some("embeddings are off".to_owned()),
        Some(e) if !e.ready() => Some(format!("model {} not downloaded yet", e.model())),
        Some(_) => None,
    };
    let vector = if vector_note.is_none() {
        vector_list(store, query, k, opts, embedder)?
    } else {
        Vec::new()
    };
    Ok(Explain {
        cards: lexical.cards,
        raw: lexical.raw,
        vector,
        vector_note,
        via_or_fallback,
        fused: search_with(store, query, opts, embedder)?,
    })
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
        assert!(rrf(0, 60.0) > rrf(1, 60.0) && rrf(1, 60.0) > rrf(10, 60.0));
        assert!(rrf(0, 30.0) > rrf(0, 60.0), "a smaller k rewards the top ranks more");
    }

    #[test]
    fn stop_words_leave_the_and_form_but_never_empty_it() {
        assert_eq!(
            without_stopwords("how do I roll back a deploy with deployctl"),
            "roll back deploy deployctl"
        );
        assert_eq!(without_stopwords("What is the CAP theorem?"), "CAP theorem?");
        assert_eq!(
            without_stopwords("what is it"),
            "what is it",
            "all stop-words: the query stays"
        );
    }

    #[test]
    fn tunables_come_from_the_config_and_change_the_fusion() {
        let cfg = crate::config::Config {
            search_rrf_k: 30.0,
            search_raw_weight: 0.5,
            search_questions_weight: 3.0,
            search_and_stopwords: true,
            ..crate::config::Config::default()
        };
        let o = SearchOptions::for_config(&cfg);
        assert!((o.rrf_k - 30.0).abs() < 1e-9 && (o.raw_list_weight - 0.5).abs() < 1e-9);
        assert!((o.questions_weight - 3.0).abs() < 1e-9 && o.and_stopwords);
        let bad = crate::config::Config { search_rrf_k: 0.0, ..crate::config::Config::default() };
        assert!((SearchOptions::for_config(&bad).rrf_k - RRF_K).abs() < 1e-9, "a bad k falls back");
        // With the raw list weighted 0, a section found by the raw index alone scores 0 and
        // sinks below one found by the cards index.
        let (store, _, _) = carded_store();
        let default = search(&store, "deployctl", &SearchOptions::default()).unwrap();
        assert!(!default.is_empty());
        let no_raw = SearchOptions { raw_list_weight: 0.0, ..SearchOptions::default() };
        let hits = search(&store, "deployctl", &no_raw).unwrap();
        assert!(hits.iter().all(|h| h.matched != Matched::Raw || h.score <= 0.0 + 1e-12));
    }

    #[test]
    fn snippet_skips_markup_only_lines() {
        let s = snippet_of(
            "## Title\n\n<Tabs\n  scrollable\n  size=\"small\"\n>\n<TabPanel id=\"a\"> </TabPanel>\n\nprose here\n\n</TabPanel>\n</Tabs>\n",
        );
        assert_eq!(s, "prose here");
        assert_eq!(
            snippet_of("## T\n\nimport X from 'y'\n\n<Note>with text</Note>\nrun --to <sha>\n"),
            "<Note>with text</Note> run --to <sha>"
        );
        // A tag closed mid-line keeps the prose after it; `<` in prose is not a tag.
        assert_eq!(
            snippet_of("# H\n<Note\n kind=\"tip\">Important prose\nFollowing prose.\na < b > c\n"),
            "Important prose Following prose. a < b > c"
        );
        assert_eq!(snippet_of("# H\n<Note\n kind=\"tip\"\nnever closed\n"), "");
    }

    #[test]
    fn snippet_skips_heading_and_collapses() {
        let s = snippet_of("## Title\n\nfirst line\n  second   line\n");
        assert_eq!(s, "first line second line");
        let long = format!("# H\n{}", "x".repeat(500));
        assert!(snippet_of(&long).ends_with('…'));
        assert_eq!(snippet_of(&long).chars().count(), 201);
    }

    /// Returns the same fixed unit vector for every text, so "closeness" is under the test's
    /// control: a stored vector equal to it scores 1, an orthogonal one scores 0.
    struct Fixed(Vec<f32>);
    impl Embedder for Fixed {
        fn model(&self) -> &'static str {
            "fixed"
        }
        fn dim(&self) -> usize {
            self.0.len()
        }
        fn ready(&self) -> bool {
            true
        }
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| self.0.clone()).collect())
        }
    }

    fn carded_store() -> (Store, String, String) {
        use crate::card::{Entities, Provenance, SCHEMA_VERSION, SectionSummary};
        use crate::markdown::parse_str;
        use crate::store::{DocTimes, Usage};
        let mut store = Store::open_in_memory().unwrap();
        let doc = parse_str(
            "# Ops\n\n## Rollback\n\nrun deployctl rollback\n\n## Pancakes\n\nflour eggs milk\n",
        );
        let t = Timestamp::from_second(1_700_000_000).unwrap();
        store
            .upsert_document(
                "ops.md",
                &doc,
                &DocTimes { created_at: None, modified_at: t, now: t, size_bytes: 1 },
            )
            .unwrap();
        let card = |tldr: &str| SectionSummary {
            tldr: tldr.into(),
            summary: tldr.into(),
            keywords: vec![],
            questions_answered: vec![],
            entities: Entities::default(),
            mentioned_dates: vec![],
            decisions: vec![],
            action_items: vec![],
        };
        let prov = Provenance {
            model: "m".into(),
            prompt_version: "p".into(),
            schema_version: SCHEMA_VERSION,
            backend: "mock".into(),
            summarized_at: t,
            truncated: false,
        };
        let (h_roll, h_cake) = (doc.sections[1].hash.clone(), doc.sections[2].hash.clone());
        store
            .attach_summary(&h_roll, &card("Revert a release."), &prov, &Usage::default())
            .unwrap();
        store
            .attach_summary(&h_cake, &card("Breakfast recipe."), &prov, &Usage::default())
            .unwrap();
        store.put_embedding(&h_roll, "fixed", &[1.0, 0.0]).unwrap();
        store.put_embedding(&h_cake, "fixed", &[0.0, 1.0]).unwrap();
        let doc_id = crate::store::doc_id_for("ops.md");
        (store, format!("{doc_id}#1"), format!("{doc_id}#2"))
    }

    #[test]
    fn vector_only_hit_is_found_and_labelled() {
        let (store, rollback_id, _) = carded_store();
        let e = Fixed(vec![1.0, 0.0]);
        // No lexical match anywhere for "undo"; the vector list still points at Rollback.
        let hits = search_with(&store, "undo", &SearchOptions::default(), Some(&e)).unwrap();
        assert_eq!(hits.len(), 2, "both vectors are returned, ranked");
        assert_eq!(hits[0].section_id, rollback_id);
        assert_eq!(hits[0].matched, Matched::Vector);
        assert!(hits[0].vector);
        assert!((hits[0].vector_score.unwrap() - 1.0).abs() < 1e-6);
        assert!(hits[0].score > hits[1].score);
        // Without an embedder, the same query finds nothing.
        assert!(search(&store, "undo", &SearchOptions::default()).unwrap().is_empty());
        // Vectors can be switched off per query.
        let off = SearchOptions { vectors: false, ..SearchOptions::default() };
        assert!(search_with(&store, "undo", &off, Some(&e)).unwrap().is_empty());
    }

    #[test]
    fn vector_and_lexical_agreement_outranks_lexical_alone() {
        let (store, rollback_id, pancake_id) = carded_store();
        // "deployctl" matches Rollback lexically; the vector points at Pancakes.
        let e = Fixed(vec![0.0, 1.0]);
        let hits = search_with(&store, "deployctl", &SearchOptions::default(), Some(&e)).unwrap();
        let roll = hits.iter().find(|h| h.section_id == rollback_id).unwrap();
        let cake = hits.iter().find(|h| h.section_id == pancake_id).unwrap();
        assert_eq!(roll.matched, Matched::Raw);
        assert!(roll.vector, "every stored vector is in the top list of a two-vector index");
        assert_eq!(cake.matched, Matched::Vector);
        // Now the vector agrees with the lexical hit: it must come out first.
        let e = Fixed(vec![1.0, 0.0]);
        let hits = search_with(&store, "deployctl", &SearchOptions::default(), Some(&e)).unwrap();
        assert_eq!(hits[0].section_id, rollback_id);
        assert!(hits[0].vector_score.unwrap() > hits[1].vector_score.unwrap());
    }

    #[test]
    fn or_fallback_decision_ignores_hits_the_filters_exclude() {
        let (store, _, _) = carded_store();
        // "flour deployctl": AND matches nothing; OR matches Pancakes (flour) and Rollback.
        let hits = search(&store, "flour deployctl", &SearchOptions::default()).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits[0].via_or_fallback);
        // An AND hit that the time filter excludes must not suppress the fallback decision,
        // and with nothing eligible either way the result is simply empty.
        let future = Timestamp::from_second(1_800_000_000).unwrap();
        let opts = SearchOptions { since: Some(future), ..SearchOptions::default() };
        assert!(search(&store, "deployctl", &opts).unwrap().is_empty(), "nothing is that new");
    }

    struct Broken;
    impl Embedder for Broken {
        fn model(&self) -> &'static str {
            "fixed"
        }
        fn dim(&self) -> usize {
            2
        }
        fn ready(&self) -> bool {
            true
        }
        fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>> {
            Err(crate::Error::Embed("corrupt model".into()))
        }
    }

    #[test]
    fn a_broken_embedder_leaves_search_lexical() {
        let (store, rollback_id, _) = carded_store();
        let hits =
            search_with(&store, "deployctl", &SearchOptions::default(), Some(&Broken)).unwrap();
        assert_eq!(hits[0].section_id, rollback_id);
        assert!(!hits[0].vector);
    }

    #[test]
    fn explain_lists_every_source() {
        let (store, rollback_id, _) = carded_store();
        let e = Fixed(vec![1.0, 0.0]);
        let ex = explain(&store, "deployctl", &SearchOptions::default(), Some(&e)).unwrap();
        assert_eq!(ex.raw[0].section_id, rollback_id);
        assert!(ex.cards.is_empty(), "the card says 'revert', not 'deployctl'");
        assert_eq!(ex.vector[0].0, rollback_id);
        assert_eq!(ex.vector_note, None);
        assert_eq!(ex.fused[0].section_id, rollback_id);
        let ex = explain(&store, "deployctl", &SearchOptions::default(), None).unwrap();
        assert!(ex.vector.is_empty());
        assert_eq!(ex.vector_note.as_deref(), Some("embeddings are off"));
    }

    #[test]
    fn vector_index_top_k_expands_shared_hashes_and_checks_dim() {
        let (store, _, _) = carded_store();
        let idx = VectorIndex::load(&store, "fixed").unwrap().unwrap();
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.model(), "fixed");
        assert!(idx.top_k(&[1.0, 0.0, 0.0], 5).is_empty(), "dimension mismatch");
        assert_eq!(idx.top_k(&[1.0, 0.0], 1).len(), 1);
        assert!(VectorIndex::load(&store, "none").unwrap().is_none());
    }

    #[test]
    fn or_expression_joins_escaped_terms() {
        let e = or_expression("roll back deploy");
        assert!(e.contains(" OR "));
        assert_eq!(e.matches(" OR ").count(), 2);
    }
}
