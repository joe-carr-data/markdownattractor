//! The T5 analysis (execution plan §4 T5): correctness and effort at equal evidence.
//!
//! Input: `grades.jsonl` as `t5.sh grade` writes it, one row per question × arm × run with a
//! deterministic `correct` (0/1), the run's effort (`tool_calls`, `source_tokens`) and the
//! runner's failure flag; plus the runner's manifest (the expected run set). Per question and
//! arm the median over runs is taken (a failed or missing run is 0 correct; its effort counts
//! as recorded, a missing run has none). Then, for mda against each comparator, a paired
//! bootstrap over the questions of three differences `mda − comparator` — correctness, tool
//! calls, source tokens — with one `SplitMix64` stream per (pair, metric) and nearest-rank
//! percentiles, as [`super::paired`] does. The claim "correct with less effort at equal
//! evidence" is made for a pair only when the correctness difference's lower bound is
//! ≥ `min_correct_lower` (−0.05) **and both** effort differences' upper bounds are < 0.
//! Every pair is reported with or without the claim.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::analysis::{AnalysisError, Manifest};
use super::paired::{SplitMix64, percentile};

/// One graded replay run, as `t5.sh grade` writes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayRow {
    /// The question id.
    pub id: String,
    /// The arm.
    pub arm: String,
    /// Run number, 1-based.
    pub run: usize,
    /// The question type (`which`, `when`, `added`).
    #[serde(default, rename = "type")]
    pub kind: String,
    /// The runner's failure flag.
    #[serde(default)]
    pub error: bool,
    /// 1 when the deterministic scorer accepted the answer, else 0.
    #[serde(default)]
    pub correct: u8,
    /// Tool calls made (effort measure one).
    #[serde(default)]
    pub tool_calls: Option<u64>,
    /// Source tokens read (effort measure two, rule 0.7).
    #[serde(default)]
    pub source_tokens: Option<u64>,
    /// Wall-clock seconds.
    #[serde(default)]
    pub wall_s: Option<f64>,
    /// List-price cost as the transcript reports it.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

/// Parameters of a replay analysis.
#[derive(Debug, Clone)]
pub struct ReplayOptions {
    /// The arm the claim is about.
    pub mda_arm: String,
    /// The comparators, in order (the git baseline first by convention).
    pub comparators: Vec<String>,
    /// Bootstrap draws.
    pub draws: usize,
    /// Bootstrap seed.
    pub seed: u64,
    /// The correctness difference's lower bound must be at least this for a claim.
    pub min_correct_lower: f64,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            mda_arm: "mda".into(),
            comparators: vec!["git".into(), "qmd".into(), "graphify".into()],
            draws: 10_000,
            seed: 20_260_922,
            min_correct_lower: -0.05,
        }
    }
}

/// One question's medians for one arm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayQuestion {
    /// The question.
    pub id: String,
    /// The arm.
    pub arm: String,
    /// The question type.
    pub kind: String,
    /// Expected runs.
    pub runs: usize,
    /// Runs with a row that did not fail.
    pub completed: usize,
    /// Median correctness over the expected runs (missing and failed runs are 0).
    pub correct: f64,
    /// Median tool calls over the runs that have a value.
    pub tool_calls: Option<f64>,
    /// Median source tokens over the runs that have a value.
    pub source_tokens: Option<f64>,
    /// Median wall-clock seconds over the runs that have a value.
    pub wall_s: Option<f64>,
    /// Median cost over the runs that have a value.
    pub cost_usd: Option<f64>,
}

/// One arm over the sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayArm {
    /// The arm.
    pub arm: String,
    /// Questions in the sample.
    pub questions: usize,
    /// Share of questions whose median run is correct.
    pub correct_rate: f64,
    /// The same, per question type.
    pub correct_by_type: BTreeMap<String, f64>,
    /// Expected runs.
    pub runs: usize,
    /// Failed or missing runs.
    pub failed: usize,
    /// Median over questions of the median tool calls.
    pub tool_calls: Option<f64>,
    /// Median over questions of the median source tokens.
    pub source_tokens: Option<f64>,
    /// Median over questions of the median wall-clock.
    pub wall_s: Option<f64>,
    /// Median over questions of the median cost.
    pub cost_usd: Option<f64>,
}

/// A paired difference with its descriptive 95% interval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Difference {
    /// Questions with a value for both arms.
    pub n: usize,
    /// Mean of `mda − comparator`.
    pub diff: f64,
    /// 2.5th percentile of the bootstrap means (nearest rank).
    pub lower: f64,
    /// 97.5th percentile.
    pub upper: f64,
    /// Questions where mda is higher.
    pub higher: usize,
    /// Questions where mda is lower.
    pub lower_count: usize,
}

/// mda against one comparator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayPair {
    /// The comparator.
    pub comparator: String,
    /// Correctness difference (positive: mda more often correct).
    pub correct: Difference,
    /// Tool-call difference (negative: mda made fewer calls).
    pub tool_calls: Difference,
    /// Source-token difference (negative: mda read less).
    pub source_tokens: Difference,
    /// Whether the preregistered claim holds.
    pub claim: bool,
    /// The criterion, spelled out.
    pub criterion: String,
}

/// The whole analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayAnalysis {
    /// Per arm.
    pub arms: Vec<ReplayArm>,
    /// Per comparator.
    pub pairs: Vec<ReplayPair>,
    /// Comparators named but absent from the manifest's arms.
    pub missing_comparators: Vec<String>,
    /// Per question and arm.
    pub questions: Vec<ReplayQuestion>,
    /// Bootstrap draws.
    pub draws: usize,
    /// Bootstrap seed.
    pub seed: u64,
}

/// Read a `grades.jsonl`.
///
/// # Errors
///
/// I/O or a malformed row.
pub fn read_rows(path: &Path) -> Result<Vec<ReplayRow>, AnalysisError> {
    let display = path.display().to_string();
    let text = std::fs::read_to_string(path)
        .map_err(|source| AnalysisError::Io { path: display.clone(), source })?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|source| AnalysisError::Json {
                path: display.clone(),
                line: i + 1,
                source,
            })
        })
        .collect()
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 { values[n / 2] } else { f64::midpoint(values[n / 2 - 1], values[n / 2]) })
}

#[allow(clippy::cast_precision_loss)]
fn question(id: &str, arm: &str, runs: usize, rows: &[&ReplayRow]) -> ReplayQuestion {
    let mut correct: Vec<f64> = (1..=runs)
        .map(|r| {
            rows.iter()
                .find(|x| x.run == r && !x.error)
                .map_or(0.0, |x| f64::from(x.correct.min(1)))
        })
        .collect();
    let mut tc: Vec<f64> = rows.iter().filter_map(|x| x.tool_calls).map(|v| v as f64).collect();
    let mut st: Vec<f64> = rows.iter().filter_map(|x| x.source_tokens).map(|v| v as f64).collect();
    let mut wall: Vec<f64> = rows.iter().filter_map(|x| x.wall_s).collect();
    let mut cost: Vec<f64> = rows.iter().filter_map(|x| x.cost_usd).collect();
    ReplayQuestion {
        id: id.to_owned(),
        arm: arm.to_owned(),
        kind: rows.first().map(|r| r.kind.clone()).unwrap_or_default(),
        runs,
        completed: rows.iter().filter(|x| !x.error).count(),
        correct: median(&mut correct).unwrap_or(0.0),
        tool_calls: median(&mut tc),
        source_tokens: median(&mut st),
        wall_s: median(&mut wall),
        cost_usd: median(&mut cost),
    }
}

#[allow(clippy::cast_precision_loss)]
fn difference(pairs: &[(f64, f64)], draws: usize, seed: u64, label: &str) -> Difference {
    let deltas: Vec<f64> = pairs.iter().map(|(m, c)| m - c).collect();
    let n = deltas.len();
    if n == 0 {
        return Difference { n, diff: 0.0, lower: 0.0, upper: 0.0, higher: 0, lower_count: 0 };
    }
    let mean = deltas.iter().sum::<f64>() / n as f64;
    let mut rng = SplitMix64::for_label(seed, label);
    let mut means: Vec<f64> = Vec::with_capacity(draws);
    for _ in 0..draws {
        let s: f64 = (0..n).map(|_| deltas[rng.below(n)]).sum();
        means.push(s / n as f64);
    }
    means.sort_by(f64::total_cmp);
    Difference {
        n,
        diff: mean,
        lower: percentile(&means, 0.025),
        upper: percentile(&means, 0.975),
        higher: deltas.iter().filter(|d| **d > 0.0).count(),
        lower_count: deltas.iter().filter(|d| **d < 0.0).count(),
    }
}

/// Run the analysis.
///
/// # Errors
///
/// Duplicate rows, rows outside the manifest's run set, no mda rows, or fewer than two
/// draws.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn analyse(
    rows: &[ReplayRow],
    manifest: &Manifest,
    opts: &ReplayOptions,
) -> Result<ReplayAnalysis, AnalysisError> {
    if opts.draws < 2 {
        return Err(AnalysisError::Invalid("at least two bootstrap draws are needed".into()));
    }
    let mut by_qa: BTreeMap<(String, String), Vec<&ReplayRow>> = BTreeMap::new();
    let mut seen: std::collections::HashSet<(&str, &str, usize)> = std::collections::HashSet::new();
    for r in rows {
        if !seen.insert((&r.id, &r.arm, r.run)) {
            return Err(AnalysisError::Duplicate {
                id: r.id.clone(),
                arm: r.arm.clone(),
                run: r.run,
            });
        }
        let why = if !manifest.question_ids.contains(&r.id) {
            Some("unknown question")
        } else if !manifest.arms.contains(&r.arm) {
            Some("unknown arm")
        } else if r.run == 0 || r.run > manifest.runs {
            Some("run number outside 1..=runs")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(AnalysisError::Unexpected {
                id: r.id.clone(),
                arm: r.arm.clone(),
                run: r.run,
                why,
            });
        }
        by_qa.entry((r.id.clone(), r.arm.clone())).or_default().push(r);
    }
    if !manifest.arms.contains(&opts.mda_arm) {
        return Err(AnalysisError::NoMdaRows(opts.mda_arm.clone()));
    }
    let analysed: BTreeSet<&String> =
        std::iter::once(&opts.mda_arm).chain(opts.comparators.iter()).collect();
    let mut per_arm: BTreeMap<String, Vec<ReplayQuestion>> = BTreeMap::new();
    let mut questions = Vec::new();
    for arm in manifest.arms.iter().filter(|a| analysed.contains(a)) {
        for id in &manifest.question_ids {
            let empty = Vec::new();
            let rs = by_qa.get(&(id.clone(), arm.clone())).unwrap_or(&empty);
            let q = question(id, arm, manifest.runs, rs);
            per_arm.entry(arm.clone()).or_default().push(q.clone());
            questions.push(q);
        }
    }
    let summary = |arm: &str| -> ReplayArm {
        let qs = &per_arm[arm];
        let n = qs.len() as f64;
        let mut by_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for q in qs {
            let e = by_type.entry(q.kind.clone()).or_default();
            e.1 += 1;
            if q.correct >= 0.5 {
                e.0 += 1;
            }
        }
        let mut tc: Vec<f64> = qs.iter().filter_map(|q| q.tool_calls).collect();
        let mut st: Vec<f64> = qs.iter().filter_map(|q| q.source_tokens).collect();
        let mut wall: Vec<f64> = qs.iter().filter_map(|q| q.wall_s).collect();
        let mut cost: Vec<f64> = qs.iter().filter_map(|q| q.cost_usd).collect();
        let runs: usize = qs.iter().map(|q| q.runs).sum();
        let completed: usize = qs.iter().map(|q| q.completed).sum();
        ReplayArm {
            arm: arm.to_owned(),
            questions: qs.len(),
            correct_rate: if qs.is_empty() {
                0.0
            } else {
                qs.iter().filter(|q| q.correct >= 0.5).count() as f64 / n
            },
            correct_by_type: by_type
                .into_iter()
                .map(|(k, (c, t))| (k, if t == 0 { 0.0 } else { c as f64 / t as f64 }))
                .collect(),
            runs,
            failed: runs - completed,
            tool_calls: median(&mut tc),
            source_tokens: median(&mut st),
            wall_s: median(&mut wall),
            cost_usd: median(&mut cost),
        }
    };
    let arms: Vec<ReplayArm> = per_arm.keys().map(|a| summary(a)).collect();
    let mq = &per_arm[&opts.mda_arm];
    let mut pairs = Vec::new();
    let mut missing_comparators = Vec::new();
    for comp in &opts.comparators {
        let Some(cq) = per_arm.get(comp) else {
            missing_comparators.push(comp.clone());
            continue;
        };
        let label = |metric: &str| format!("{} vs {comp}: {metric}", opts.mda_arm);
        let correct: Vec<(f64, f64)> =
            mq.iter().zip(cq).map(|(m, c)| (m.correct, c.correct)).collect();
        let tool_calls: Vec<(f64, f64)> =
            mq.iter().zip(cq).filter_map(|(m, c)| Some((m.tool_calls?, c.tool_calls?))).collect();
        let source_tokens: Vec<(f64, f64)> = mq
            .iter()
            .zip(cq)
            .filter_map(|(m, c)| Some((m.source_tokens?, c.source_tokens?)))
            .collect();
        let correct = difference(&correct, opts.draws, opts.seed, &label("correct"));
        let tool_calls = difference(&tool_calls, opts.draws, opts.seed, &label("tool_calls"));
        let source_tokens =
            difference(&source_tokens, opts.draws, opts.seed, &label("source_tokens"));
        // Eligibility (Codex M8 F10): the three intervals must be over the same questions (an
        // effort value for every question of both arms), and mda must have answered something
        // correctly — two arms that fail everything have a zero correctness difference and
        // must not earn an effort claim (rule 0.3).
        let mda_correct = mq.iter().filter(|q| q.correct >= 0.5).count();
        let same_sample = tool_calls.n == correct.n && source_tokens.n == correct.n;
        let claim = correct.n > 0
            && same_sample
            && mda_correct > 0
            && correct.lower >= opts.min_correct_lower
            && tool_calls.upper < 0.0
            && source_tokens.upper < 0.0;
        pairs.push(ReplayPair {
            comparator: comp.clone(),
            correct,
            tool_calls,
            source_tokens,
            claim,
            criterion: format!(
                "lower bound of Δcorrect ≥ {} and upper bounds of Δtool_calls and Δsource_tokens < 0; eligible only with an effort value for every question of both arms and at least one correct mda answer",
                opts.min_correct_lower
            ),
        });
    }
    Ok(ReplayAnalysis {
        arms,
        pairs,
        missing_comparators,
        questions,
        draws: opts.draws,
        seed: opts.seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        id: &str,
        arm: &str,
        run: usize,
        correct: u8,
        calls: u64,
        toks: u64,
        error: bool,
    ) -> ReplayRow {
        ReplayRow {
            id: id.into(),
            arm: arm.into(),
            run,
            kind: if id.ends_with('w') { "when".into() } else { "which".into() },
            error,
            correct,
            tool_calls: Some(calls),
            source_tokens: Some(toks),
            wall_s: Some(10.0),
            cost_usd: Some(0.01),
        }
    }

    fn manifest(ids: &[&str]) -> Manifest {
        Manifest {
            question_ids: ids.iter().map(|s| (*s).to_owned()).collect(),
            arms: vec!["git".into(), "mda".into()],
            runs: 3,
        }
    }

    fn opts() -> ReplayOptions {
        ReplayOptions { comparators: vec!["git".into()], draws: 2000, ..ReplayOptions::default() }
    }

    #[test]
    fn failed_and_missing_runs_are_incorrect_and_the_claim_needs_both_efforts() {
        let ids = ["q1", "q2w", "q3"];
        let mut rows = Vec::new();
        for id in ids {
            for r in 1..=3 {
                rows.push(row(id, "mda", r, 1, 2, 100, false));
                rows.push(row(id, "git", r, 1, 6, 900, false));
            }
        }
        // mda fails two of three runs on q3: its median is 0 there
        rows.retain(|r| !(r.id == "q3" && r.arm == "mda" && r.run == 2));
        if let Some(r) = rows.iter_mut().find(|r| r.id == "q3" && r.arm == "mda" && r.run == 3) {
            r.error = true;
        }
        let a = analyse(&rows, &manifest(&ids), &opts()).unwrap();
        let mda = a.arms.iter().find(|x| x.arm == "mda").unwrap();
        assert!((mda.correct_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(mda.failed, 2);
        assert!((mda.correct_by_type["when"] - 1.0).abs() < 1e-12);
        let p = &a.pairs[0];
        assert!(p.tool_calls.upper < 0.0 && p.source_tokens.upper < 0.0);
        // one question lost of three: the lower bound of Δcorrect is well below −0.05
        assert!(p.correct.lower < -0.05);
        assert!(!p.claim);
    }

    #[test]
    fn equal_correctness_with_less_effort_makes_the_claim() {
        let ids: Vec<String> = (1..=12).map(|i| format!("q{i}")).collect();
        let idr: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut rows = Vec::new();
        for id in &idr {
            for r in 1..=3 {
                rows.push(row(id, "mda", r, 1, 2, 100, false));
                rows.push(row(id, "git", r, 1, 6, 900, false));
            }
        }
        let a = analyse(&rows, &manifest(&idr), &opts()).unwrap();
        let p = &a.pairs[0];
        assert!(p.correct.diff.abs() < 1e-12);
        assert!(p.claim, "{p:?}");
    }

    #[test]
    fn two_arms_that_fail_everything_earn_no_claim_and_missing_effort_blocks_it() {
        let ids: Vec<String> = (1..=12).map(|i| format!("q{i}")).collect();
        let idr: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut rows = Vec::new();
        for id in &idr {
            for r in 1..=3 {
                rows.push(row(id, "mda", r, 0, 2, 100, false));
                rows.push(row(id, "git", r, 0, 6, 900, false));
            }
        }
        let a = analyse(&rows, &manifest(&idr), &opts()).unwrap();
        assert!(!a.pairs[0].claim, "all-failed arms must not claim");
        // correct answers, but one question without an effort value for git
        for r in &mut rows {
            r.correct = 1;
        }
        for r in rows.iter_mut().filter(|r| r.id == "q1" && r.arm == "git") {
            r.tool_calls = None;
        }
        let a = analyse(&rows, &manifest(&idr), &opts()).unwrap();
        assert_eq!(a.pairs[0].tool_calls.n, 11);
        assert!(!a.pairs[0].claim, "effort missing for a question blocks the claim");
    }

    #[test]
    fn rows_outside_the_manifest_are_refused() {
        let rows = vec![row("q1", "mda", 1, 1, 1, 1, false), row("q9", "mda", 1, 1, 1, 1, false)];
        assert!(matches!(
            analyse(&rows, &manifest(&["q1"]), &opts()),
            Err(AnalysisError::Unexpected { why: "unknown question", .. })
        ));
        let rows = vec![row("q1", "mda", 1, 1, 1, 1, false), row("q1", "mda", 1, 0, 1, 1, false)];
        assert!(matches!(
            analyse(&rows, &manifest(&["q1"]), &opts()),
            Err(AnalysisError::Duplicate { .. })
        ));
    }
}
