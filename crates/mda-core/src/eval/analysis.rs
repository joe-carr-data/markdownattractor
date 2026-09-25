//! The T2 analysis (execution plan §2.5, strategy rule 0.4): from graded answer runs to the
//! per-question scores, the paired comparison of mda with every comparator arm, the three
//! gates that a savings claim needs, and the savings themselves — claimed only where the
//! gates pass. Failures are results (rule 0.3): a run that errored, produced no answer or
//! no grade scores 0 and fails grounding; a run the manifest expected but the file lacks is
//! the same; such runs contribute no token, call or cost value to any saving and are
//! counted per arm. A secondary "completed runs only" view is computed for information and
//! never gates.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paired::{SplitMix64, percentile};

/// Everything that can go wrong in the analysis.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    /// A file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// A line is not a grade row.
    #[error("{path} line {line}: {source}")]
    Json {
        /// The file.
        path: String,
        /// 1-based line.
        line: usize,
        /// The error.
        #[source]
        source: serde_json::Error,
    },
    /// The same (question, arm, run) appears twice: the runner double-counted.
    #[error("duplicate observation for question {id:?}, arm {arm:?}, run {run}")]
    Duplicate {
        /// The question.
        id: String,
        /// The arm.
        arm: String,
        /// The run number.
        run: usize,
    },
    /// The mda arm has no rows.
    #[error("no rows for the mda arm {0:?}")]
    NoMdaRows(String),
    /// Nothing to analyse or a bad parameter.
    #[error("{0}")]
    Invalid(String),
}

/// One graded run, as `t2.sh grade` writes it (a row per question × arm × run).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GradeRow {
    /// `question_id`.
    pub id: String,
    /// The arm (`grep`, `mda`, `qmd`, `graphify`, …).
    pub arm: String,
    /// Run number, 1-based.
    pub run: usize,
    /// The runner's failure flag (errored, timed out, no answer).
    #[serde(default)]
    pub error: bool,
    /// The grader's score on the 0–6 scale; `None` when the run failed or the grader gave
    /// no structured output.
    #[serde(default)]
    pub score: Option<f64>,
    /// The grounding verdict: `Some(true)` when every claim is supported by a cited page;
    /// `None` when not checked (a failed run, or no grader output).
    #[serde(default)]
    pub grounded: Option<bool>,
    /// Source tokens read (rule 0.7: from consecutive turns' usage).
    #[serde(default)]
    pub source_tokens: Option<u64>,
    /// Total input tokens of the transcript.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Tool calls made.
    #[serde(default)]
    pub tool_calls: Option<u64>,
    /// List-price cost as the transcript reports it.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Wall-clock seconds.
    #[serde(default)]
    pub wall_s: Option<f64>,
}

/// What a complete run set looks like (`manifest.json` of the runner): every expected
/// (question, arm, run) that has no row is a failed run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Manifest {
    /// The questions.
    pub question_ids: Vec<String>,
    /// The arms.
    pub arms: Vec<String>,
    /// Runs per question and arm.
    pub runs: usize,
}

/// The parameters of one analysis.
#[derive(Debug, Clone)]
pub struct AnalysisOptions {
    /// The arm the claims are about.
    pub mda_arm: String,
    /// The arms mda is compared with (each pair gated on its own).
    pub comparators: Vec<String>,
    /// Bootstrap draws (the plan says 10,000).
    pub draws: usize,
    /// Bootstrap seed.
    pub seed: u64,
    /// Gate (a): lower bound of the paired interval of `mda − comparator` mean score.
    pub min_lower_bound: f64,
    /// Gate (b): mda's mean score over the whole sample.
    pub min_mean: f64,
    /// Gate (c): mda's grounding pass rate over all graded-or-failed runs.
    pub min_grounding: f64,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            mda_arm: "mda".into(),
            comparators: vec!["grep".into(), "qmd".into(), "graphify".into()],
            draws: 10_000,
            seed: 20_260_922,
            min_lower_bound: -0.25,
            min_mean: 4.0,
            min_grounding: 0.95,
        }
    }
}

/// One question's numbers for one arm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuestionArm {
    /// The question.
    pub id: String,
    /// The arm.
    pub arm: String,
    /// Runs expected (from the manifest, or the rows).
    pub runs: usize,
    /// Runs that completed with a grade.
    pub completed: usize,
    /// Runs counted as failures (errored, ungraded, or missing).
    pub failed: usize,
    /// The question-level score: conventional median of the runs' scores, failed runs as 0.
    pub score: f64,
    /// The same median over completed runs only (`None` when none completed): the
    /// secondary view, never a gate.
    pub score_completed: Option<f64>,
    /// Grounding passes among the runs (a failed run is a fail).
    pub grounded: usize,
    /// Median source tokens over completed runs (failed runs contribute nothing).
    pub source_tokens: Option<f64>,
    /// Median tool calls over completed runs.
    pub tool_calls: Option<f64>,
    /// Median cost over completed runs.
    pub cost_usd: Option<f64>,
}

/// One arm over the whole sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArmSummary {
    /// The arm.
    pub arm: String,
    /// Questions.
    pub questions: usize,
    /// Mean of the question-level scores (failed runs as 0).
    pub mean_score: f64,
    /// Mean over questions of the completed-only medians, over questions with at least one
    /// completed run (secondary view).
    pub mean_score_completed: Option<f64>,
    /// Questions with at least one completed run.
    pub questions_with_completed: usize,
    /// Runs expected.
    pub runs: usize,
    /// Runs that completed with a grade.
    pub completed: usize,
    /// Runs counted as failures.
    pub failed: usize,
    /// Grounding pass rate over all runs (failed runs fail).
    pub grounding_rate: f64,
    /// Median over questions of the median source tokens of completed runs.
    pub source_tokens: Option<f64>,
    /// Median over questions of the median tool calls of completed runs.
    pub tool_calls: Option<f64>,
    /// Median over questions of the median cost of completed runs.
    pub cost_usd: Option<f64>,
}

/// The gates of rule 0.4 for one pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gates {
    /// (a) the interval's lower bound ≥ `min_lower_bound`.
    pub lower_bound_ok: bool,
    /// (b) mda's mean ≥ `min_mean`.
    pub mean_ok: bool,
    /// (c) mda's grounding rate ≥ `min_grounding`.
    pub grounding_ok: bool,
    /// All three.
    pub pass: bool,
}

/// mda against one comparator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairReport {
    /// The comparator arm.
    pub comparator: String,
    /// Questions paired.
    pub n: usize,
    /// Mean of `mda − comparator` question scores.
    pub mean_delta: f64,
    /// 2.5th and 97.5th percentiles of the resampled mean delta.
    pub ci95: (f64, f64),
    /// Questions mda scores higher / lower / equal.
    pub wins: usize,
    /// Losses.
    pub losses: usize,
    /// Ties.
    pub ties: usize,
    /// The gates.
    pub gates: Gates,
    /// Savings, present only when the gates pass: ratio `comparator / mda` of the median
    /// source tokens, tool calls and cost (completed runs only, labelled with their
    /// denominators), and the number of questions with a completed run on both sides.
    pub savings: Option<Savings>,
}

/// The savings of a passing pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Savings {
    /// Questions with at least one completed run on both arms (the denominator).
    pub questions: usize,
    /// Median over those questions of the comparator's source tokens / mda's.
    pub source_tokens_ratio: Option<f64>,
    /// Median ratio of tool calls.
    pub tool_calls_ratio: Option<f64>,
    /// Median ratio of cost.
    pub cost_ratio: Option<f64>,
}

/// The whole analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Analysis {
    /// Per arm.
    pub arms: Vec<ArmSummary>,
    /// mda against each comparator, in the order requested (a comparator with no rows is
    /// listed in `missing_comparators`).
    pub pairs: Vec<PairReport>,
    /// Comparators that have no rows at all.
    pub missing_comparators: Vec<String>,
    /// Per question and arm.
    pub questions: Vec<QuestionArm>,
    /// Rows whose arm is neither mda nor a comparator (counted, not analysed).
    pub other_arms: Vec<String>,
    /// Bootstrap draws.
    pub draws: usize,
    /// Seed.
    pub seed: u64,
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 { values[n / 2] } else { f64::midpoint(values[n / 2 - 1], values[n / 2]) })
}

/// Read a `grades.jsonl`.
///
/// # Errors
///
/// When the file cannot be read or a line is not a grade row.
pub fn read_grades(path: &Path) -> Result<Vec<GradeRow>, AnalysisError> {
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

/// Read a runner manifest (`{question_ids, arms, runs, …}`; other fields ignored).
///
/// # Errors
///
/// When the file cannot be read or parsed.
pub fn read_manifest(path: &Path) -> Result<Manifest, AnalysisError> {
    let display = path.display().to_string();
    let text = std::fs::read_to_string(path)
        .map_err(|source| AnalysisError::Io { path: display.clone(), source })?;
    serde_json::from_str(&text).map_err(|source| AnalysisError::Json {
        path: display,
        line: 1,
        source,
    })
}

/// A run's contribution: `Some((score, grounded, tokens…))` when it completed with a
/// grade, `None` when it is a failure (rule 0.3).
fn completed(row: &GradeRow) -> Option<(f64, bool)> {
    if row.error {
        return None;
    }
    row.score.map(|s| (s, row.grounded.unwrap_or(false)))
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn question_arm(id: &str, arm: &str, expected_runs: usize, rows: &[&GradeRow]) -> QuestionArm {
    let runs = expected_runs.max(rows.len());
    let mut scores: Vec<f64> = Vec::with_capacity(runs);
    let mut completed_scores: Vec<f64> = Vec::new();
    let mut grounded = 0;
    let mut src: Vec<f64> = Vec::new();
    let mut calls: Vec<f64> = Vec::new();
    let mut cost: Vec<f64> = Vec::new();
    for r in rows {
        match completed(r) {
            Some((s, g)) => {
                scores.push(s);
                completed_scores.push(s);
                if g {
                    grounded += 1;
                }
                if let Some(v) = r.source_tokens {
                    src.push(v as f64);
                }
                if let Some(v) = r.tool_calls {
                    calls.push(v as f64);
                }
                if let Some(v) = r.cost_usd {
                    cost.push(v);
                }
            }
            None => scores.push(0.0),
        }
    }
    // runs the manifest expected but the file lacks: failures
    while scores.len() < runs {
        scores.push(0.0);
    }
    let n_completed = completed_scores.len();
    QuestionArm {
        id: id.to_owned(),
        arm: arm.to_owned(),
        runs,
        completed: n_completed,
        failed: runs - n_completed,
        score: median(&mut scores).unwrap_or(0.0),
        score_completed: median(&mut completed_scores),
        grounded,
        source_tokens: median(&mut src),
        tool_calls: median(&mut calls),
        cost_usd: median(&mut cost),
    }
}

/// Run the analysis.
///
/// # Errors
///
/// On duplicate observations, no mda rows, or fewer than two draws.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn analyse(
    rows: &[GradeRow],
    manifest: Option<&Manifest>,
    opts: &AnalysisOptions,
) -> Result<Analysis, AnalysisError> {
    if opts.draws < 2 {
        return Err(AnalysisError::Invalid("at least two bootstrap draws are needed".into()));
    }
    // index rows by (question, arm); refuse duplicates
    let mut by_qa: BTreeMap<(String, String), Vec<&GradeRow>> = BTreeMap::new();
    let mut seen: std::collections::HashSet<(&str, &str, usize)> = std::collections::HashSet::new();
    for r in rows {
        if !seen.insert((&r.id, &r.arm, r.run)) {
            return Err(AnalysisError::Duplicate {
                id: r.id.clone(),
                arm: r.arm.clone(),
                run: r.run,
            });
        }
        by_qa.entry((r.id.clone(), r.arm.clone())).or_default().push(r);
    }
    // the universe: the manifest's questions × arms × runs when given, else the rows'
    let (question_ids, arms, expected_runs): (Vec<String>, Vec<String>, usize) =
        if let Some(m) = manifest {
            (m.question_ids.clone(), m.arms.clone(), m.runs)
        } else {
            let q: BTreeSet<String> = rows.iter().map(|r| r.id.clone()).collect();
            let a: BTreeSet<String> = rows.iter().map(|r| r.arm.clone()).collect();
            let runs = by_qa.values().map(Vec::len).max().unwrap_or(0);
            (q.into_iter().collect(), a.into_iter().collect(), runs)
        };
    if !arms.contains(&opts.mda_arm) {
        return Err(AnalysisError::NoMdaRows(opts.mda_arm.clone()));
    }
    let analysed: BTreeSet<&String> =
        std::iter::once(&opts.mda_arm).chain(opts.comparators.iter()).collect();
    let other_arms: Vec<String> = arms.iter().filter(|a| !analysed.contains(a)).cloned().collect();
    let mut questions: Vec<QuestionArm> = Vec::new();
    let mut per_arm: BTreeMap<String, Vec<QuestionArm>> = BTreeMap::new();
    for arm in arms.iter().filter(|a| analysed.contains(a)) {
        for id in &question_ids {
            let empty = Vec::new();
            let rs = by_qa.get(&(id.clone(), arm.clone())).unwrap_or(&empty);
            let qa = question_arm(id, arm, expected_runs, rs);
            per_arm.entry(arm.clone()).or_default().push(qa.clone());
            questions.push(qa);
        }
    }
    let summary = |arm: &str| -> ArmSummary {
        let qs = &per_arm[arm];
        let n = qs.len() as f64;
        let completed: usize = qs.iter().map(|q| q.completed).sum();
        let runs: usize = qs.iter().map(|q| q.runs).sum();
        let grounded: usize = qs.iter().map(|q| q.grounded).sum();
        let with_completed: Vec<f64> = qs.iter().filter_map(|q| q.score_completed).collect();
        let mut src: Vec<f64> = qs.iter().filter_map(|q| q.source_tokens).collect();
        let mut calls: Vec<f64> = qs.iter().filter_map(|q| q.tool_calls).collect();
        let mut cost: Vec<f64> = qs.iter().filter_map(|q| q.cost_usd).collect();
        ArmSummary {
            arm: arm.to_owned(),
            questions: qs.len(),
            mean_score: qs.iter().map(|q| q.score).sum::<f64>() / n,
            mean_score_completed: if with_completed.is_empty() {
                None
            } else {
                Some(with_completed.iter().sum::<f64>() / with_completed.len() as f64)
            },
            questions_with_completed: with_completed.len(),
            runs,
            completed,
            failed: runs - completed,
            grounding_rate: if runs == 0 { 0.0 } else { grounded as f64 / runs as f64 },
            source_tokens: median(&mut src),
            tool_calls: median(&mut calls),
            cost_usd: median(&mut cost),
        }
    };
    let arm_summaries: Vec<ArmSummary> = per_arm.keys().map(|a| summary(a)).collect();
    let mda = summary(&opts.mda_arm);
    let mut pairs = Vec::new();
    let mut missing_comparators = Vec::new();
    for comp in &opts.comparators {
        let Some(cq) = per_arm.get(comp) else {
            missing_comparators.push(comp.clone());
            continue;
        };
        let mq = &per_arm[&opts.mda_arm];
        let deltas: Vec<f64> = mq.iter().zip(cq).map(|(m, c)| m.score - c.score).collect();
        let n = deltas.len();
        let mean_delta = deltas.iter().sum::<f64>() / n as f64;
        let mut rng = SplitMix64::for_label(opts.seed, &format!("{} vs {}", opts.mda_arm, comp));
        let mut draws: Vec<f64> = Vec::with_capacity(opts.draws);
        for _ in 0..opts.draws {
            let s: f64 = (0..n).map(|_| deltas[rng.below(n)]).sum();
            draws.push(s / n as f64);
        }
        draws.sort_by(f64::total_cmp);
        let ci95 = (percentile(&draws, 0.025), percentile(&draws, 0.975));
        let gates = Gates {
            lower_bound_ok: ci95.0 >= opts.min_lower_bound,
            mean_ok: mda.mean_score >= opts.min_mean,
            grounding_ok: mda.grounding_rate >= opts.min_grounding,
            pass: ci95.0 >= opts.min_lower_bound
                && mda.mean_score >= opts.min_mean
                && mda.grounding_rate >= opts.min_grounding,
        };
        let savings = if gates.pass {
            let both: Vec<(&QuestionArm, &QuestionArm)> =
                mq.iter().zip(cq).filter(|(m, c)| m.completed > 0 && c.completed > 0).collect();
            let ratio = |f: fn(&QuestionArm) -> Option<f64>| -> Option<f64> {
                let mut v: Vec<f64> = both
                    .iter()
                    .filter_map(|(m, c)| match (f(m), f(c)) {
                        (Some(a), Some(b)) if a > 0.0 => Some(b / a),
                        _ => None,
                    })
                    .collect();
                median(&mut v)
            };
            Some(Savings {
                questions: both.len(),
                source_tokens_ratio: ratio(|q| q.source_tokens),
                tool_calls_ratio: ratio(|q| q.tool_calls),
                cost_ratio: ratio(|q| q.cost_usd),
            })
        } else {
            None
        };
        pairs.push(PairReport {
            comparator: comp.clone(),
            n,
            mean_delta,
            ci95,
            wins: deltas.iter().filter(|d| **d > 0.0).count(),
            losses: deltas.iter().filter(|d| **d < 0.0).count(),
            ties: deltas.iter().filter(|d| **d == 0.0).count(),
            gates,
            savings,
        });
    }
    Ok(Analysis {
        arms: arm_summaries,
        pairs,
        missing_comparators,
        questions,
        other_arms,
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
        score: Option<f64>,
        grounded: Option<bool>,
        error: bool,
    ) -> GradeRow {
        GradeRow {
            id: id.into(),
            arm: arm.into(),
            run,
            error,
            score,
            grounded,
            source_tokens: Some(if arm == "mda" { 100 } else { 500 }),
            input_tokens: None,
            output_tokens: None,
            tool_calls: Some(if arm == "mda" { 2 } else { 6 }),
            cost_usd: Some(if arm == "mda" { 0.01 } else { 0.05 }),
            wall_s: None,
        }
    }

    fn opts() -> AnalysisOptions {
        AnalysisOptions {
            comparators: vec!["grep".into()],
            draws: 2000,
            ..AnalysisOptions::default()
        }
    }

    #[test]
    fn failed_ungraded_and_missing_runs_score_zero_fail_grounding_and_count() {
        let rows = vec![
            row("q1", "mda", 1, Some(6.0), Some(true), false),
            row("q1", "mda", 2, None, None, true),  // errored
            row("q1", "mda", 3, None, None, false), // ungraded
            row("q1", "grep", 1, Some(6.0), Some(true), false),
            row("q1", "grep", 2, Some(6.0), Some(true), false),
            // grep run 3 missing from the file
        ];
        let m = Manifest {
            question_ids: vec!["q1".into()],
            arms: vec!["mda".into(), "grep".into()],
            runs: 3,
        };
        let a = analyse(&rows, Some(&m), &opts()).unwrap();
        let mda = a.questions.iter().find(|q| q.arm == "mda").unwrap();
        assert_eq!((mda.runs, mda.completed, mda.failed, mda.grounded), (3, 1, 2, 1));
        assert!((mda.score - 0.0).abs() < 1e-12, "median of [6, 0, 0] is 0");
        assert_eq!(mda.score_completed, Some(6.0));
        let grep = a.questions.iter().find(|q| q.arm == "grep").unwrap();
        assert_eq!((grep.runs, grep.completed, grep.failed), (3, 2, 1));
        assert!((grep.score - 6.0).abs() < 1e-12, "median of [6, 6, 0] is 6");
        let s = a.arms.iter().find(|s| s.arm == "mda").unwrap();
        assert!((s.grounding_rate - 1.0 / 3.0).abs() < 1e-12);
        assert!(!a.pairs[0].gates.pass && a.pairs[0].savings.is_none());
        // failed runs contribute no token value: the mda question median is from the one completed run
        assert_eq!(mda.source_tokens, Some(100.0));
    }

    #[test]
    fn zero_versus_zero_is_never_parity_or_a_saving() {
        let rows =
            vec![row("q1", "mda", 1, None, None, true), row("q1", "grep", 1, None, None, true)];
        let a = analyse(&rows, None, &opts()).unwrap();
        let p = &a.pairs[0];
        assert_eq!((p.wins, p.losses, p.ties), (0, 0, 1));
        assert!(!p.gates.mean_ok && !p.gates.grounding_ok && !p.gates.pass);
        assert!(p.savings.is_none());
    }

    #[test]
    fn gates_pass_and_savings_are_computed_over_completed_pairs() {
        let mut rows = Vec::new();
        for i in 0..20 {
            let id = format!("q{i}");
            rows.push(row(&id, "mda", 1, Some(5.0), Some(true), false));
            rows.push(row(&id, "grep", 1, Some(4.0), Some(true), false));
        }
        // one grep failure: its question is excluded from the savings denominator, not from the gates
        rows[1] = row("q0", "grep", 1, None, None, true);
        let a = analyse(&rows, None, &opts()).unwrap();
        let p = &a.pairs[0];
        assert!(p.gates.pass, "{p:?}");
        assert!(p.ci95.0 > 0.0 && p.mean_delta > 1.0);
        let s = p.savings.as_ref().unwrap();
        assert_eq!(s.questions, 19);
        assert!((s.source_tokens_ratio.unwrap() - 5.0).abs() < 1e-12);
        assert!((s.tool_calls_ratio.unwrap() - 3.0).abs() < 1e-12);
        assert!((s.cost_ratio.unwrap() - 5.0).abs() < 1e-12);
        assert_eq!(a.other_arms, Vec::<String>::new());
    }

    #[test]
    fn interval_is_deterministic_and_the_lower_bound_gate_bites() {
        let mut rows = Vec::new();
        for i in 0..10 {
            let id = format!("q{i}");
            let (m, g) = if i % 2 == 0 { (6.0, 2.0) } else { (2.0, 6.0) };
            rows.push(row(&id, "mda", 1, Some(m), Some(true), false));
            rows.push(row(&id, "grep", 1, Some(g), Some(true), false));
        }
        let a = analyse(&rows, None, &opts()).unwrap();
        let b = analyse(&rows, None, &opts()).unwrap();
        assert_eq!(a, b);
        let p = &a.pairs[0];
        assert!((p.mean_delta).abs() < 1e-12);
        assert!(p.ci95.0 < -0.25, "{:?}", p.ci95);
        assert!(!p.gates.lower_bound_ok && !p.gates.pass);
    }

    #[test]
    fn refuses_duplicates_and_a_missing_mda_arm() {
        let rows = vec![row("q1", "grep", 1, Some(1.0), Some(true), false)];
        assert!(matches!(analyse(&rows, None, &opts()), Err(AnalysisError::NoMdaRows(_))));
        let rows = vec![
            row("q1", "mda", 1, Some(1.0), Some(true), false),
            row("q1", "mda", 1, Some(2.0), Some(true), false),
        ];
        assert!(matches!(analyse(&rows, None, &opts()), Err(AnalysisError::Duplicate { .. })));
    }

    #[test]
    fn reads_grade_rows_and_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let g = dir.path().join("grades.jsonl");
        std::fs::write(&g, "{\"id\":\"q1\",\"arm\":\"mda\",\"run\":1,\"score\":5,\"grounded\":true}\n\n{\"id\":\"q1\",\"arm\":\"grep\",\"run\":1,\"error\":true}\n").unwrap();
        let rows = read_grades(&g).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].score, Some(5.0));
        assert!(rows[1].error && rows[1].score.is_none());
        let m = dir.path().join("manifest.json");
        std::fs::write(&m, "{\"question_ids\":[\"q1\"],\"arms\":[\"mda\",\"grep\"],\"runs\":3,\"model\":\"sonnet\"}").unwrap();
        assert_eq!(read_manifest(&m).unwrap().runs, 3);
    }
}
