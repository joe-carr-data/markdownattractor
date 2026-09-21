//! `mda index [path]` — parse and index, then summarize what is pending.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use mda_core::pipeline::{Engine, IndexReport, SummarizeReport};
use mda_core::worker::ClaudeCli;
use tokio_util::sync::CancellationToken;

use crate::output::{self, Style};

/// Arguments for `mda index`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// A single file to (re)index. Defaults to the whole root.
    pub path: Option<PathBuf>,
    /// Watched root. Defaults to the nearest ancestor with a `.markdownattractor/` directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Only parse and index raw text; do not call the model.
    #[arg(long)]
    pub no_summarize: bool,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut engine = Engine::open(&root).with_context(|| format!("opening {}", root.display()))?;
    let st = Style::auto();

    let started = std::time::Instant::now();
    let report = match &args.path {
        Some(p) => {
            let abs = if p.is_absolute() { p.clone() } else { std::env::current_dir()?.join(p) };
            let abs = abs.canonicalize().with_context(|| format!("resolving {}", p.display()))?;
            let out = engine.index_file(&abs)?;
            IndexReport {
                files: 1,
                changed: usize::from(out.upsert.created || out.upsert.changed),
                pending: out.upsert.new_hashes.len(),
                tombstoned: 0,
                errors: Vec::new(),
            }
        }
        None => engine.index_root()?,
    };
    let parse_ms = started.elapsed().as_millis();

    if !json {
        println!(
            "{} {} file(s) · {} changed · {} section(s) need a card · {} tombstoned · {} ms",
            st.ok("indexed"),
            report.files,
            report.changed,
            report.pending,
            report.tombstoned,
            parse_ms,
        );
        for (path, err) in &report.errors {
            println!("  {} {path}: {err}", st.warn("skipped"));
        }
    }

    let summarize = if args.no_summarize || report.pending == 0 {
        None
    } else {
        Some(summarize(&mut engine, json, &st)?)
    };

    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "index": report,
            "parse_ms": parse_ms,
            "summarize": summarize,
        }));
    } else if let Some(s) = &summarize {
        let usage = &s.pool.usage;
        println!(
            "{} {} card(s) · {} failed · {} clean · {} date(s) and {} entit{} dropped by grounding · {} in / {} out tokens · ${:.4}",
            st.ok("summarized"),
            s.ok,
            s.failed,
            s.clean,
            s.dropped_dates,
            s.dropped_entities,
            if s.dropped_entities == 1 { "y" } else { "ies" },
            usage.input_tokens,
            usage.output_tokens,
            usage.cost_usd,
        );
        if s.failed > 0 {
            println!(
                "  {} `mda status` lists failed sections; `mda index` retries them",
                st.dim("hint:")
            );
        }
    } else if report.pending > 0 {
        println!("  {} run without --no-summarize to produce cards", st.dim("hint:"));
    }

    let failed =
        !report.errors.is_empty() || summarize.as_ref().is_some_and(|s| s.failed > 0 && s.ok == 0);
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn summarize(engine: &mut Engine, json: bool, st: &Style) -> anyhow::Result<SummarizeReport> {
    let backend = Arc::new(ClaudeCli::new(engine.config()).context("preparing the claude worker")?);
    let cancel = CancellationToken::new();
    let ctrl_c = cancel.clone();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            ctrl_c.cancel();
        }
    });

    let quiet = json;
    let model = engine.config().summarization_model.clone();
    let report = rt.block_on(engine.summarize_pending(backend, cancel, move |p| {
        if quiet {
            return;
        }
        let last = p.last.as_deref().unwrap_or("");
        let last: String = last.chars().take(60).collect();
        eprint!(
            "\r\x1b[2K  {} {}/{} · {} ok · {} failed · {last}",
            st.accent(&model),
            p.done,
            p.total,
            p.ok,
            p.failed
        );
        let _ = std::io::stderr().flush();
    }))?;
    if !json {
        eprint!("\r\x1b[2K");
    }
    Ok(report)
}
