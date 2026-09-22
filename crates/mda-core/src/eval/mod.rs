//! Offline evaluation over public datasets (benchmark plan `docs/plans/2026-09-benchmarks.md`).
//!
//! The golden-set eval lives in the CLI (`mda eval --golden`); the dataset adapters live here
//! because they carry logic worth testing on its own: page-level metrics, coverage against a
//! store, and the seeded split. Nothing in this module writes anywhere.

pub mod docsqa;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Which part of a dataset a question belongs to (plan rule 0.2: tune on `Dev`, publish
/// `Test`, open `Holdout` once at 1.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    /// 30%: for tuning.
    Dev,
    /// 55%: published on every release, reuse stated.
    Test,
    /// 15%: sealed until 1.0.
    Holdout,
}

impl Split {
    /// Parse `dev`, `test` or `holdout`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "dev" => Some(Self::Dev),
            "test" => Some(Self::Test),
            "holdout" => Some(Self::Holdout),
            _ => None,
        }
    }
}

/// Assign each id to a split, stratified within the group it is called for (one call per
/// project): ids are ordered by `blake3(seed ‖ id)`, the first 30% are `Dev`, the next 55%
/// `Test`, the rest `Holdout`. Deterministic for a given seed whatever the input order.
#[must_use]
pub fn split_ids(seed: u64, ids: &[String]) -> HashMap<String, Split> {
    let mut keyed: Vec<(String, &String)> = ids
        .iter()
        .map(|id| {
            let mut h = blake3::Hasher::new();
            h.update(&seed.to_le_bytes());
            h.update(id.as_bytes());
            (h.finalize().to_hex().to_string(), id)
        })
        .collect();
    keyed.sort();
    let n = keyed.len();
    let dev_end = n * 30 / 100;
    let test_end = dev_end + n * 55 / 100;
    keyed
        .into_iter()
        .enumerate()
        .map(|(i, (_, id))| {
            let split = if i < dev_end {
                Split::Dev
            } else if i < test_end {
                Split::Test
            } else {
                Split::Holdout
            };
            (id.clone(), split)
        })
        .collect()
}

/// Retrieval metrics at page granularity over one set of questions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Questions scored.
    pub questions: usize,
    /// Fraction whose first relevant page is in the top 5.
    pub success_at_5: f64,
    /// Mean reciprocal rank of the first relevant page within the top 5 (0 beyond).
    pub mrr_at_5: f64,
    /// Mean normalised DCG at 10 with binary gains over the relevant pages.
    pub ndcg_at_10: f64,
    /// Mean query latency in milliseconds.
    pub mean_ms: f64,
}

/// Score one ranked list of pages (deduplicated, best first) against the relevant pages.
/// Returns `(rank of first relevant, 1-based; reciprocal rank at 5; nDCG@10)`.
#[must_use]
#[allow(clippy::cast_precision_loss)] // ranks and counts are tiny
pub fn score_pages(ranked: &[String], relevant: &[String]) -> (Option<usize>, f64, f64) {
    let rank = ranked.iter().position(|p| relevant.contains(p)).map(|r| r + 1);
    let rr = match rank {
        Some(r) if r <= 5 => 1.0 / r as f64,
        _ => 0.0,
    };
    let dcg: f64 = ranked
        .iter()
        .take(10)
        .enumerate()
        .filter(|(_, p)| relevant.contains(p))
        .map(|(i, _)| 1.0 / ((i + 2) as f64).log2())
        .sum();
    let distinct = relevant.iter().collect::<std::collections::HashSet<_>>().len();
    let ideal: f64 = (0..distinct.min(10)).map(|i| 1.0 / ((i + 2) as f64).log2()).sum();
    let ndcg = if ideal > 0.0 { dcg / ideal } else { 0.0 };
    (rank, rr, ndcg)
}

/// Deduplicate a ranked list of section paths into a ranked list of pages: a page's rank is
/// the rank of its first section (plan axis A).
#[must_use]
pub fn pages_of<'a>(section_paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    section_paths.into_iter().filter(|p| seen.insert(*p)).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_deterministic_stratified_and_order_independent() {
        let ids: Vec<String> = (0..100).map(|i| format!("q{i}")).collect();
        let a = split_ids(20_260_922, &ids);
        let mut shuffled = ids.clone();
        shuffled.reverse();
        let b = split_ids(20_260_922, &shuffled);
        assert_eq!(a, b);
        let count = |s: Split| a.values().filter(|v| **v == s).count();
        assert_eq!((count(Split::Dev), count(Split::Test), count(Split::Holdout)), (30, 55, 15));
        assert_ne!(a, split_ids(1, &ids), "another seed is another split");
        assert!(split_ids(7, &[]).is_empty());
        assert_eq!(split_ids(7, &["only".to_owned()]).values().next(), Some(&Split::Holdout));
    }

    #[test]
    #[allow(clippy::float_cmp)] // exact zeros and ones are the point
    fn page_metrics() {
        let ranked = pages_of(["a.md", "b.md", "a.md", "c.md", "d.md", "e.md", "f.md"]);
        assert_eq!(ranked, vec!["a.md", "b.md", "c.md", "d.md", "e.md", "f.md"]);
        let (rank, rr, ndcg) = score_pages(&ranked, &["c.md".to_owned()]);
        assert_eq!(rank, Some(3));
        assert!((rr - 1.0 / 3.0).abs() < 1e-9);
        assert!((ndcg - 1.0 / 4f64.log2()).abs() < 1e-9);
        let (rank, rr, ndcg) = score_pages(&ranked, &["f.md".to_owned()]);
        assert_eq!(rank, Some(6));
        assert_eq!(rr, 0.0, "beyond 5");
        assert!(ndcg > 0.0, "still inside 10");
        assert_eq!(score_pages(&ranked, &["zz.md".to_owned()]), (None, 0.0, 0.0));
        let (_, _, perfect) = score_pages(&ranked, &["a.md".to_owned(), "b.md".to_owned()]);
        assert!((perfect - 1.0).abs() < 1e-9);
        assert_eq!(score_pages(&[], &["a.md".to_owned()]), (None, 0.0, 0.0));
        assert_eq!(score_pages(&ranked, &[]).2, 0.0, "no relevant pages: nDCG is 0, not NaN");
        let (_, _, dup) =
            score_pages(&["a.md".to_owned()], &["a.md".to_owned(), "a.md".to_owned()]);
        assert!((dup - 1.0).abs() < 1e-9, "a duplicated label is one page: {dup}");
    }

    #[test]
    fn split_parse() {
        assert_eq!(Split::parse("dev"), Some(Split::Dev));
        assert_eq!(Split::parse("nope"), None);
    }
}
