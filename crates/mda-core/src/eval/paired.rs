//! Paired comparison of two archived runs (execution plan §3, 2026-09-24 amendment): for each
//! project the questions are the same, so a candidate is compared with its baseline
//! question by question (wins, losses, ties), and the uncertainty of the mean difference is
//! a within-project paired bootstrap. The objective of the tuning loop is the mean over
//! projects of success@5, so the objective difference is resampled jointly: one draw
//! resamples every project's questions at once and averages the per-project mean
//! differences.
//!
//! The intervals are descriptive. They do not authorize adoption and are not a
//! significance test across several trials; the plan's 0.01 screen stays an engineering
//! threshold.
//!
//! [`intervals`] gives one run's own per-project intervals (success@5, MRR@5, nDCG@10 over
//! its questions, the same bootstrap and generator) for the published tables (plan §4, T1:
//! "complete, with per-project intervals").

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Everything that can go wrong in a paired comparison.
#[derive(Debug, thiserror::Error)]
pub enum PairedError {
    /// A results file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// A results file is not the adapter's `results.json`.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: String,
        /// The error.
        #[source]
        source: serde_json::Error,
    },
    /// The named run is not in the file.
    #[error("{path}: no run named {run:?}")]
    RunNotFound {
        /// The file.
        path: String,
        /// The run looked for.
        run: String,
    },
    /// The two sides do not score the same questions (the plan keeps ids, eligibility and
    /// denominators fixed; anything else is not a paired comparison).
    #[error(
        "{label}: question sets differ ({only_baseline} only in the baseline, {only_candidate} only in the candidate; first: {first:?})"
    )]
    IdMismatch {
        /// The project.
        label: String,
        /// Ids only the baseline has.
        only_baseline: usize,
        /// Ids only the candidate has.
        only_candidate: usize,
        /// One of them, for the message.
        first: String,
    },
    /// A side lists the same question twice.
    #[error("{label}: question {id:?} appears twice in the {side}")]
    Duplicate {
        /// The project.
        label: String,
        /// `baseline` or `candidate`.
        side: &'static str,
        /// The id.
        id: String,
    },
    /// No questions at all.
    #[error("{label}: no scored questions")]
    Empty {
        /// The project.
        label: String,
    },
    /// Fewer than two draws or no project.
    #[error("{0}")]
    Invalid(&'static str),
}

/// One project's two sides: `(question_id, success@5)` rows in any order.
#[derive(Debug, Clone)]
pub struct PairedInput {
    /// The project (or any label the report repeats).
    pub label: String,
    /// The reference run.
    pub baseline: Vec<(String, bool)>,
    /// The candidate run.
    pub candidate: Vec<(String, bool)>,
}

/// One project's paired numbers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairedProject {
    /// The label.
    pub label: String,
    /// Questions compared (the fixed denominator).
    pub n: usize,
    /// Baseline success@5.
    pub baseline: f64,
    /// Candidate success@5.
    pub candidate: f64,
    /// `candidate − baseline`, unrounded.
    pub delta: f64,
    /// Questions the candidate gets and the baseline misses.
    pub wins: usize,
    /// Questions the baseline gets and the candidate misses.
    pub losses: usize,
    /// Questions with the same outcome.
    pub ties: usize,
    /// 2.5th and 97.5th percentiles of the resampled `delta`.
    pub ci95: (f64, f64),
}

/// The comparison of a candidate with its baseline over one or more projects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairedReport {
    /// Per project, in input order.
    pub projects: Vec<PairedProject>,
    /// Mean over projects of the baseline success@5 (the tuning objective).
    pub objective_baseline: f64,
    /// Mean over projects of the candidate success@5.
    pub objective_candidate: f64,
    /// `objective_candidate − objective_baseline`, unrounded.
    pub objective_delta: f64,
    /// 2.5th and 97.5th percentiles of the jointly resampled objective difference.
    pub objective_ci95: (f64, f64),
    /// Total wins over every project.
    pub wins: usize,
    /// Total losses over every project.
    pub losses: usize,
    /// Bootstrap draws.
    pub draws: usize,
    /// Bootstrap seed.
    pub seed: u64,
}

/// `SplitMix64`: a tiny, fully specified generator so the draws are the same on every
/// platform and every release (a library generator's stream may change between versions).
/// The order of the projects does not matter: each has its own stream (`for_label`).
pub(crate) struct SplitMix64(pub(crate) u64);

impl SplitMix64 {
    /// A stream for one project: the seed, then every byte of the label folded in.
    pub(crate) fn for_label(seed: u64, label: &str) -> Self {
        let mut g = Self(seed);
        for b in label.bytes() {
            g.0 ^= u64::from(b);
            g.next_u64();
        }
        g
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform index in `0..n` (`n > 0`), by rejection so every index is equally likely.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        let n64 = n as u64;
        let zone = u64::MAX - u64::MAX % n64;
        loop {
            let v = self.next_u64();
            if v < zone {
                #[allow(clippy::cast_possible_truncation)]
                return (v % n64) as usize;
            }
        }
    }
}

/// Nearest-rank percentile of a sorted slice (`q` in `0..=1`): the `⌈N·q⌉`-th smallest value
/// (so the 2.5th percentile of 5,000 draws is the 125th, the 97.5th the 4,875th).
pub(crate) fn percentile(sorted: &[f64], q: f64) -> f64 {
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rank = (sorted.len() as f64 * q).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn align(input: &PairedInput) -> Result<Vec<(bool, bool)>, PairedError> {
    let index = |rows: &[(String, bool)], side: &'static str| {
        let mut map = HashMap::with_capacity(rows.len());
        for (id, hit) in rows {
            if map.insert(id.clone(), *hit).is_some() {
                return Err(PairedError::Duplicate {
                    label: input.label.clone(),
                    side,
                    id: id.clone(),
                });
            }
        }
        Ok(map)
    };
    let base = index(&input.baseline, "baseline")?;
    let cand = index(&input.candidate, "candidate")?;
    let base_ids: BTreeSet<&String> = base.keys().collect();
    let cand_ids: BTreeSet<&String> = cand.keys().collect();
    if base_ids != cand_ids {
        let only_baseline = base_ids.difference(&cand_ids).count();
        let only_candidate = cand_ids.difference(&base_ids).count();
        let first = base_ids
            .symmetric_difference(&cand_ids)
            .next()
            .map(|s| (*s).clone())
            .unwrap_or_default();
        return Err(PairedError::IdMismatch {
            label: input.label.clone(),
            only_baseline,
            only_candidate,
            first,
        });
    }
    if base.is_empty() {
        return Err(PairedError::Empty { label: input.label.clone() });
    }
    Ok(base_ids.into_iter().map(|id| (base[id], cand[id])).collect())
}

#[allow(clippy::cast_precision_loss)]
fn mean_delta(pairs: &[(bool, bool)]) -> f64 {
    pairs.iter().map(|&(b, c)| f64::from(u8::from(c)) - f64::from(u8::from(b))).sum::<f64>()
        / pairs.len() as f64
}

/// Compare a candidate with its baseline, question by question, with a within-project
/// paired bootstrap of the mean differences (`draws` resamples, `seed`).
///
/// # Errors
///
/// When the two sides of a project do not score exactly the same question ids, a side
/// repeats an id, a project is empty, there is no project, or `draws < 2`.
#[allow(clippy::cast_precision_loss)]
pub fn compare(
    inputs: &[PairedInput],
    draws: usize,
    seed: u64,
) -> Result<PairedReport, PairedError> {
    if inputs.is_empty() {
        return Err(PairedError::Invalid("no project to compare"));
    }
    if draws < 2 {
        return Err(PairedError::Invalid("at least two bootstrap draws are needed"));
    }
    let aligned: Vec<Vec<(bool, bool)>> = inputs.iter().map(align).collect::<Result<_, _>>()?;
    // One generator per project, keyed by the seed and the label, so the draws do not depend
    // on the order the projects are given in; draw `d` of the objective combines draw `d` of
    // every project.
    let mut rngs: Vec<SplitMix64> =
        inputs.iter().map(|i| SplitMix64::for_label(seed, &i.label)).collect();
    let mut per_project: Vec<Vec<f64>> = vec![Vec::with_capacity(draws); inputs.len()];
    let mut objective: Vec<f64> = Vec::with_capacity(draws);
    let mut sample: Vec<(bool, bool)> = Vec::new();
    for _ in 0..draws {
        let mut sum = 0.0;
        for (p, pairs) in aligned.iter().enumerate() {
            let rng = &mut rngs[p];
            sample.clear();
            sample.extend((0..pairs.len()).map(|_| pairs[rng.below(pairs.len())]));
            let d = mean_delta(&sample);
            per_project[p].push(d);
            sum += d;
        }
        objective.push(sum / aligned.len() as f64);
    }
    let mut projects = Vec::with_capacity(inputs.len());
    for (p, pairs) in aligned.iter().enumerate() {
        let n = pairs.len();
        let wins = pairs.iter().filter(|&&(b, c)| c && !b).count();
        let losses = pairs.iter().filter(|&&(b, c)| b && !c).count();
        let baseline = pairs.iter().filter(|&&(b, _)| b).count() as f64 / n as f64;
        let candidate = pairs.iter().filter(|&&(_, c)| c).count() as f64 / n as f64;
        let draws_sorted = &mut per_project[p];
        draws_sorted.sort_by(f64::total_cmp);
        projects.push(PairedProject {
            label: inputs[p].label.clone(),
            n,
            baseline,
            candidate,
            delta: mean_delta(pairs),
            wins,
            losses,
            ties: n - wins - losses,
            ci95: (percentile(draws_sorted, 0.025), percentile(draws_sorted, 0.975)),
        });
    }
    objective.sort_by(f64::total_cmp);
    let k = projects.len() as f64;
    let objective_baseline = projects.iter().map(|p| p.baseline).sum::<f64>() / k;
    let objective_candidate = projects.iter().map(|p| p.candidate).sum::<f64>() / k;
    Ok(PairedReport {
        wins: projects.iter().map(|p| p.wins).sum(),
        losses: projects.iter().map(|p| p.losses).sum(),
        objective_delta: projects.iter().map(|p| p.delta).sum::<f64>() / k,
        objective_ci95: (percentile(&objective, 0.025), percentile(&objective, 0.975)),
        projects,
        objective_baseline,
        objective_candidate,
        draws,
        seed,
    })
}

/// One question's archived observation, as much of it as the intervals need.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRow {
    /// `question_id`.
    pub id: String,
    /// Rank (1-based) of the first relevant page, if any.
    #[serde(default)]
    pub rank: Option<usize>,
    /// nDCG@10 as the scorer archived it.
    #[serde(default)]
    pub ndcg_at_10: f64,
}

/// One run's metrics with their bootstrap intervals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunInterval {
    /// The run's name.
    pub run: String,
    /// Questions (the denominator).
    pub n: usize,
    /// success@5.
    pub success_at_5: f64,
    /// Its 2.5th and 97.5th percentiles over the resamples.
    pub success_ci95: (f64, f64),
    /// MRR@5.
    pub mrr_at_5: f64,
    /// Its interval.
    pub mrr_ci95: (f64, f64),
    /// Mean nDCG@10.
    pub ndcg_at_10: f64,
    /// Its interval.
    pub ndcg_ci95: (f64, f64),
    /// Bootstrap draws.
    pub draws: usize,
    /// Bootstrap seed.
    pub seed: u64,
}

#[allow(clippy::cast_precision_loss)]
fn metric_means(rows: &[&RunRow]) -> (f64, f64, f64) {
    let n = rows.len() as f64;
    let hit = rows.iter().filter(|r| r.rank.is_some_and(|k| k <= 5)).count();
    let rr: f64 =
        rows.iter().filter_map(|r| r.rank.filter(|&k| k <= 5)).map(|k| 1.0 / k as f64).sum();
    let ndcg: f64 = rows.iter().map(|r| r.ndcg_at_10).sum();
    (hit as f64 / n, rr / n, ndcg / n)
}

/// One run's success@5, MRR@5 and nDCG@10 with 95% bootstrap intervals over its questions
/// (`draws` resamples with replacement; the generator is keyed by `seed` and the run name,
/// so files can be given in any order).
///
/// # Errors
///
/// When the run has no rows, repeats a question id, or `draws < 2`.
pub fn intervals(
    run: &str,
    rows: &[RunRow],
    draws: usize,
    seed: u64,
) -> Result<RunInterval, PairedError> {
    if rows.is_empty() {
        return Err(PairedError::Empty { label: run.to_owned() });
    }
    if draws < 2 {
        return Err(PairedError::Invalid("at least two bootstrap draws are needed"));
    }
    let mut ids = std::collections::HashSet::with_capacity(rows.len());
    for r in rows {
        if !ids.insert(r.id.as_str()) {
            return Err(PairedError::Duplicate {
                label: run.to_owned(),
                side: "run",
                id: r.id.clone(),
            });
        }
    }
    let all: Vec<&RunRow> = rows.iter().collect();
    let (success_at_5, mrr_at_5, ndcg_at_10) = metric_means(&all);
    let mut rng = SplitMix64::for_label(seed, run);
    let mut successes = Vec::with_capacity(draws);
    let mut mrrs = Vec::with_capacity(draws);
    let mut ndcgs = Vec::with_capacity(draws);
    let mut sample: Vec<&RunRow> = Vec::with_capacity(rows.len());
    for _ in 0..draws {
        sample.clear();
        sample.extend((0..rows.len()).map(|_| &rows[rng.below(rows.len())]));
        let (success, mrr, ndcg) = metric_means(&sample);
        successes.push(success);
        mrrs.push(mrr);
        ndcgs.push(ndcg);
    }
    for v in [&mut successes, &mut mrrs, &mut ndcgs] {
        v.sort_by(f64::total_cmp);
    }
    Ok(RunInterval {
        run: run.to_owned(),
        n: rows.len(),
        success_at_5,
        success_ci95: (percentile(&successes, 0.025), percentile(&successes, 0.975)),
        mrr_at_5,
        mrr_ci95: (percentile(&mrrs, 0.025), percentile(&mrrs, 0.975)),
        ndcg_at_10,
        ndcg_ci95: (percentile(&ndcgs, 0.025), percentile(&ndcgs, 0.975)),
        draws,
        seed,
    })
}

#[derive(Deserialize)]
struct ResultsFile {
    runs: Vec<ResultsRun>,
}

#[derive(Deserialize)]
struct ResultsRun {
    run: String,
    results: Vec<RunRow>,
}

/// Every run of an archived `results.json` with its rows.
///
/// # Errors
///
/// When the file cannot be read or parsed.
pub fn read_runs(path: &Path) -> Result<Vec<(String, Vec<RunRow>)>, PairedError> {
    let display = path.display().to_string();
    let text = std::fs::read_to_string(path)
        .map_err(|source| PairedError::Io { path: display.clone(), source })?;
    let file: ResultsFile = serde_json::from_str(&text)
        .map_err(|source| PairedError::Json { path: display, source })?;
    Ok(file.runs.into_iter().map(|r| (r.run, r.results)).collect())
}

/// The `(question_id, success@5)` rows of one named run in an archived `results.json`
/// (`rank` is the 1-based rank of the first relevant page; success@5 is `rank ≤ 5`).
///
/// # Errors
///
/// When the file cannot be read or parsed, or has no run of that name.
pub fn read_hits(path: &Path, run: &str) -> Result<Vec<(String, bool)>, PairedError> {
    let (_, rows) =
        read_runs(path)?.into_iter().find(|(name, _)| name == run).ok_or_else(|| {
            PairedError::RunNotFound { path: path.display().to_string(), run: run.to_owned() }
        })?;
    Ok(rows.into_iter().map(|r| (r.id, r.rank.is_some_and(|k| k <= 5))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(hits: &[bool]) -> Vec<(String, bool)> {
        hits.iter().enumerate().map(|(i, &h)| (format!("q{i}"), h)).collect()
    }

    fn one(label: &str, base: &[bool], cand: &[bool]) -> PairedInput {
        PairedInput { label: label.into(), baseline: rows(base), candidate: rows(cand) }
    }

    #[test]
    fn identical_runs_have_zero_delta_and_a_degenerate_interval() {
        let r = compare(&[one("p", &[true, false, true], &[true, false, true])], 200, 1).unwrap();
        assert!(r.objective_delta.abs() < 1e-12);
        assert!(r.objective_ci95.0.abs() < 1e-12 && r.objective_ci95.1.abs() < 1e-12);
        assert_eq!((r.wins, r.losses, r.projects[0].ties), (0, 0, 3));
    }

    #[test]
    fn counts_wins_losses_and_the_unrounded_delta() {
        let r =
            compare(&[one("p", &[true, false, false, true], &[false, true, true, true])], 500, 7)
                .unwrap();
        let p = &r.projects[0];
        assert_eq!((p.wins, p.losses, p.ties, p.n), (2, 1, 1, 4));
        assert!((p.delta - 0.25).abs() < 1e-12);
        assert!((p.baseline - 0.5).abs() < 1e-12 && (p.candidate - 0.75).abs() < 1e-12);
        assert!(p.ci95.0 <= p.delta && p.delta <= p.ci95.1);
    }

    #[test]
    fn objective_is_the_mean_over_projects_with_equal_weights() {
        let r = compare(
            &[one("big", &[false; 10], &[true; 10]), one("small", &[true, true], &[true, true])],
            100,
            3,
        )
        .unwrap();
        assert!((r.objective_delta - 0.5).abs() < 1e-12);
        assert!((r.objective_baseline - 0.5).abs() < 1e-12);
        assert!((r.objective_candidate - 1.0).abs() < 1e-12);
        assert!(
            (r.objective_ci95.0 - 0.5).abs() < 1e-12 && (r.objective_ci95.1 - 0.5).abs() < 1e-12
        );
    }

    #[test]
    fn same_seed_gives_the_same_interval_and_another_seed_may_not() {
        let input = [one(
            "p",
            &[true, false, false, true, false, true],
            &[false, true, true, true, true, false],
        )];
        let a = compare(&input, 2000, 20_260_922).unwrap();
        let b = compare(&input, 2000, 20_260_922).unwrap();
        assert_eq!(a, b);
        assert!(a.objective_ci95.0 < a.objective_delta && a.objective_delta < a.objective_ci95.1);
    }

    #[test]
    fn project_order_does_not_change_the_report() {
        let a = one("a", &[true, false, false, true, false], &[false, true, true, true, false]);
        let b = one("b", &[true, true, false], &[true, false, true]);
        let ab = compare(&[a.clone(), b.clone()], 1000, 5).unwrap();
        let ba = compare(&[b, a], 1000, 5).unwrap();
        assert_eq!(ab.objective_ci95, ba.objective_ci95);
        assert_eq!(ab.projects[0], ba.projects[1]);
        assert_eq!(ab.projects[1], ba.projects[0]);
    }

    #[test]
    fn refuses_different_question_sets_duplicates_and_empty_projects() {
        let mut m = one("p", &[true, false], &[true, false]);
        m.candidate.push(("extra".into(), true));
        assert!(matches!(
            compare(&[m], 10, 1),
            Err(PairedError::IdMismatch { only_candidate: 1, only_baseline: 0, .. })
        ));
        let mut d = one("p", &[true, false], &[true, false]);
        d.baseline.push(("q0".into(), false));
        assert!(matches!(
            compare(&[d], 10, 1),
            Err(PairedError::Duplicate { side: "baseline", .. })
        ));
        assert!(matches!(compare(&[one("p", &[], &[])], 10, 1), Err(PairedError::Empty { .. })));
        assert!(matches!(compare(&[], 10, 1), Err(PairedError::Invalid(_))));
        assert!(matches!(
            compare(&[one("p", &[true], &[true])], 1, 1),
            Err(PairedError::Invalid(_))
        ));
    }

    #[test]
    fn reads_success_at_5_from_an_archived_results_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.json");
        std::fs::write(
            &path,
            r#"{"runs":[{"run":"hybrid","results":[{"id":"a","rank":5},{"id":"b","rank":6},{"id":"c","rank":null},{"id":"d"}]}]}"#,
        )
        .unwrap();
        let hits = read_hits(&path, "hybrid").unwrap();
        assert_eq!(
            hits,
            vec![("a".into(), true), ("b".into(), false), ("c".into(), false), ("d".into(), false)]
        );
        assert!(matches!(read_hits(&path, "other"), Err(PairedError::RunNotFound { .. })));
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<f64> = (1..=5000).map(f64::from).collect();
        let at = |q: f64, want: f64| (percentile(&v, q) - want).abs() < 1e-9;
        assert!(at(0.025, 125.0) && at(0.975, 4875.0) && at(0.0, 1.0) && at(1.0, 5000.0));
        assert!((percentile(&[7.0], 0.5) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn intervals_cover_the_point_estimates_and_refuse_bad_input() {
        let row = |i: usize, rank: Option<usize>, ndcg: f64| RunRow {
            id: format!("q{i}"),
            rank,
            ndcg_at_10: ndcg,
        };
        let rows = vec![
            row(0, Some(1), 1.0),
            row(1, Some(3), 0.5),
            row(2, None, 0.0),
            row(3, Some(7), 0.2),
        ];
        let r = intervals("hybrid", &rows, 1000, 20_260_922).unwrap();
        assert_eq!(r.n, 4);
        assert!((r.success_at_5 - 0.5).abs() < 1e-12);
        assert!((r.mrr_at_5 - (1.0 + 1.0 / 3.0) / 4.0).abs() < 1e-12);
        assert!((r.ndcg_at_10 - 0.425).abs() < 1e-12);
        assert!(r.success_ci95.0 <= r.success_at_5 && r.success_at_5 <= r.success_ci95.1);
        assert!(r.mrr_ci95.0 <= r.mrr_at_5 && r.mrr_at_5 <= r.mrr_ci95.1);
        assert!(r.ndcg_ci95.0 <= r.ndcg_at_10 && r.ndcg_at_10 <= r.ndcg_ci95.1);
        assert_eq!(r, intervals("hybrid", &rows, 1000, 20_260_922).unwrap());
        assert!(matches!(intervals("x", &[], 10, 1), Err(PairedError::Empty { .. })));
        let dup = vec![row(0, None, 0.0), row(0, None, 0.0)];
        assert!(matches!(
            intervals("x", &dup, 10, 1),
            Err(PairedError::Duplicate { side: "run", .. })
        ));
    }

    #[test]
    fn reads_every_run_of_a_results_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.json");
        std::fs::write(
            &path,
            r#"{"runs":[{"run":"a","results":[{"id":"x","rank":2,"ndcg_at_10":0.5}]},{"run":"b","results":[{"id":"x"}]}]}"#,
        )
        .unwrap();
        let runs = read_runs(&path).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].1[0], RunRow { id: "x".into(), rank: Some(2), ndcg_at_10: 0.5 });
        assert_eq!(runs[1].1[0], RunRow { id: "x".into(), rank: None, ndcg_at_10: 0.0 });
    }

    #[test]
    fn below_is_uniform_enough_and_in_range() {
        let mut rng = SplitMix64(42);
        let mut counts = [0usize; 3];
        for _ in 0..30_000 {
            counts[rng.below(3)] += 1;
        }
        for c in counts {
            assert!((9_000..11_000).contains(&c), "{counts:?}");
        }
    }
}
