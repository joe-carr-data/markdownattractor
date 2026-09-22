//! `mda index [path]` — parse and index, then summarize what is pending.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use mda_core::pipeline::{Engine, IndexReport, SummarizeOptions, SummarizeReport};
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
    /// Put sections that failed summarization back in the queue before summarizing.
    #[arg(long)]
    pub retry_failed: bool,
    /// Summarize at most this many sections this run (smallest first); the rest stay pending.
    #[arg(long)]
    pub limit: Option<usize>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let mut engine = Engine::open(&root).with_context(|| format!("opening {}", root.display()))?;
    let st = Style::auto();

    if super::block_on(mda_core::daemon::is_running(engine.root()))? {
        return delegate(args, &mut engine, json, &st);
    }

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

    if args.retry_failed {
        let n = engine.store_mut().retry_failed()?;
        if !json {
            println!("{} {n} failed section(s) queued again", st.ok("retry"));
        }
    }
    // Pending work includes sections left over from earlier runs (failures, interrupted
    // runs), not only what this run discovered.
    let pending_total = engine.store().counts()?.pending;
    let summarize = if args.no_summarize || pending_total == 0 {
        None
    } else {
        Some(summarize(
            &mut engine,
            json,
            &st,
            SummarizeOptions { limit: args.limit, hot_paths: Vec::new() },
        )?)
    };

    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "index": report,
            "parse_ms": parse_ms,
            "summarize": summarize,
        }));
    } else {
        print_summary(summarize.as_ref(), pending_total, &st);
    }

    let failed =
        !report.errors.is_empty() || summarize.as_ref().is_some_and(|s| s.failed > 0 && s.ok == 0);
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn print_summary(summarize: Option<&SummarizeReport>, pending_total: u64, st: &Style) {
    if let Some(s) = summarize {
        let usage = &s.pool.usage;
        println!(
            "{} {} card(s){} · {} failed · {} clean · {} date(s) and {} entit{} dropped by grounding · {} in / {} out tokens · ${:.4}",
            st.ok("summarized"),
            s.ok,
            if s.deterministic > 0 {
                format!(" (+{} heading-only)", s.deterministic)
            } else {
                String::new()
            },
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
                "  {} {} failed; `mda index --retry-failed` queues them again",
                st.dim("hint:"),
                s.failed
            );
        }
        if s.deferred > 0 {
            let why = if s.budget_exhausted { "daily token budget reached" } else { "--limit" };
            println!(
                "  {} {} section(s) still pending ({why}); run `mda index` again later",
                st.dim("hint:"),
                s.deferred
            );
        }
    } else if pending_total > 0 {
        println!(
            "  {} {pending_total} section(s) pending; run without --no-summarize to produce cards",
            st.dim("hint:")
        );
    }
}

/// A running daemon is the single writer for cards: hand the request to it and return once the
/// raw index is updated. Cards follow in the background.
fn delegate(args: &Args, engine: &mut Engine, json: bool, st: &Style) -> anyhow::Result<ExitCode> {
    use mda_core::daemon::{Client, Request, Response};
    if args.retry_failed {
        let n = engine.store_mut().retry_failed()?;
        if !json {
            println!("{} {n} failed section(s) queued again", st.ok("retry"));
        }
    }
    let path = match &args.path {
        Some(p) => {
            let abs = if p.is_absolute() { p.clone() } else { std::env::current_dir()?.join(p) };
            Some(abs.display().to_string())
        }
        None => None,
    };
    let started = std::time::Instant::now();
    let resp = super::block_on(async {
        let mut c = Client::connect(engine.root()).await?;
        c.request(&Request::Index { path }).await
    })??;
    let report = match resp {
        Response::Indexed(r) => *r,
        Response::Error { message } => anyhow::bail!("daemon could not index: {message}"),
        other => anyhow::bail!("unexpected answer from the daemon: {other:?}"),
    };
    let parse_ms = started.elapsed().as_millis();
    if json {
        output::json(&serde_json::json!({
            "root": engine.root(),
            "daemon": true,
            "index": report,
            "parse_ms": parse_ms,
            "summarize": null,
        }));
    } else {
        println!(
            "{} {} file(s) · {} changed · {} section(s) queued for cards · {} tombstoned · {} ms · via the daemon",
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
        if args.limit.is_some() {
            println!(
                "  {} --limit is ignored while the daemon runs; it paces itself",
                st.dim("note:")
            );
        }
        println!("  {} mda watch · mda status", st.dim("follow:"));
    }
    Ok(if report.errors.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn summarize(
    engine: &mut Engine,
    json: bool,
    st: &Style,
    opts: SummarizeOptions,
) -> anyhow::Result<SummarizeReport> {
    let backend = mda_core::worker::backend_for(engine.config()).with_context(|| {
        format!(
            "preparing the {} backend (run `mda doctor`, or `mda backend local` for a local model)",
            engine.config().backend.as_str()
        )
    })?;
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
    let report = rt.block_on(engine.summarize_pending(backend, cancel, opts, move |p| {
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
