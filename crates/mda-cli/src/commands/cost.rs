//! `mda cost [--since 7d]` — what summarization cost, from the usage ledger.
//!
//! The ledger records every model attempt whatever its outcome, so retries and failures are
//! part of the bill. Tokens *saved* on reads are not measured yet (that needs a read ledger;
//! see the Phase 4 plan), and the command says so rather than guessing.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;
use mda_core::store::DailyUsage;

use crate::output::{self, Style};

/// Arguments for `mda cost`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Only count attempts since this time: `7d`, `24h`, `2026-09-01`. Default: everything.
    #[arg(long)]
    pub since: Option<String>,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Totals over a set of ledger rows.
#[derive(Debug, Default, serde::Serialize)]
struct Totals {
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
}

impl Totals {
    fn add(&mut self, r: &DailyUsage) {
        self.calls += r.calls;
        self.input_tokens += r.input_tokens;
        self.output_tokens += r.output_tokens;
        self.cost_usd += r.cost_usd;
    }
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let since = args.since.as_deref().map(super::parse_time).transpose()?;
    let rows = engine.store().usage_by_day(since)?;
    let counts = engine.store().counts()?;

    let mut window = Totals::default();
    let mut by_day: Vec<(String, Totals)> = Vec::new();
    let mut by_model: Vec<(String, String, Totals)> = Vec::new();
    for r in &rows {
        window.add(r);
        if let Some((_, t)) = by_day.iter_mut().find(|(d, _)| *d == r.day) {
            t.add(r);
        } else {
            let mut t = Totals::default();
            t.add(r);
            by_day.push((r.day.clone(), t));
        }
        if let Some((_, _, t)) =
            by_model.iter_mut().find(|(m, o, _)| *m == r.model && *o == r.outcome)
        {
            t.add(r);
        } else {
            let mut t = Totals::default();
            t.add(r);
            by_model.push((r.model.clone(), r.outcome.clone(), t));
        }
    }

    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "since": since,
            "window": window,
            "rows": rows,
            "cards_all_time": {
                "input_tokens": counts.total_input_tokens,
                "output_tokens": counts.total_output_tokens,
                "cost_usd": counts.total_cost_usd,
                "summarized": counts.summarized,
            },
            "tokens_saved": serde_json::Value::Null,
        }));
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    let scope = match &args.since {
        Some(s) => format!("since {s}"),
        None => "all time".to_owned(),
    };
    println!(
        "{} {} · {} · {} call(s) · {} in / {} out tokens · ${:.4}",
        st.bold("cost"),
        st.dim(&engine.root().display().to_string()),
        scope,
        window.calls,
        window.input_tokens,
        window.output_tokens,
        window.cost_usd,
    );
    if rows.is_empty() {
        println!("  {} nothing in the ledger for this window", st.dim("note:"));
    } else {
        println!("  {:<12} {:>6} {:>10} {:>9} {:>9}", "day", "calls", "in", "out", "usd");
        for (day, t) in &by_day {
            println!(
                "  {day:<12} {:>6} {:>10} {:>9} {:>9.4}",
                t.calls, t.input_tokens, t.output_tokens, t.cost_usd
            );
        }
        println!("  {:<24} {:<12} {:>6} {:>9}", "model", "outcome", "calls", "usd");
        for (model, outcome, t) in &by_model {
            let outcome = if outcome == "ok" { st.ok(outcome) } else { st.warn(outcome) };
            println!(
                "  {:<24} {:<12} {:>6} {:>9.4}",
                st.accent(model),
                outcome,
                t.calls,
                t.cost_usd
            );
        }
    }
    #[allow(clippy::cast_precision_loss)] // section counts are far below 2^52
    let per_card =
        if counts.summarized == 0 { 0.0 } else { counts.total_cost_usd / counts.summarized as f64 };
    println!(
        "  {} {} card(s) attached all time for ${:.4} (${per_card:.4} per card)",
        st.dim("cards:"),
        counts.summarized,
        counts.total_cost_usd,
    );
    println!(
        "  {} tokens saved on reads are not measured yet; `mda open` line ranges are the lever",
        st.dim("note:")
    );
    Ok(ExitCode::SUCCESS)
}
