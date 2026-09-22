//! `mda eval --golden <dir>` — retrieval metrics on a golden set, offline.
//!
//! The golden directory holds `docs/` (a markdown corpus) and `queries.jsonl`, one query
//! per line: `{"q": "...", "expect": [{"path": "a.md", "heading": "Rollback"}], "temporal":
//! false}`. A hit counts when its path matches and, if `heading` is given, that text occurs
//! in the hit's heading path. Optional `cards.json` (`{"<section_hash>": <SectionSummary>}`)
//! turns on the hybrid run: cards are attached without any model call and, when the
//! embedding model is on disk, vectors are built too.
//!
//! Metrics: a query's `expect` entries are *alternatives* (any of them answers it), so the
//! headline number is **success@k** (an expected section is in the top k), and **MRR@k** is
//! the mean reciprocal rank of the first expected section within the top k (0 beyond k).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use mda_core::card::{Provenance, SCHEMA_VERSION, SectionSummary};
use mda_core::config::Config;
use mda_core::pipeline::Engine;
use mda_core::search::{self, SearchOptions};
use mda_core::store::{Store, Usage};
use serde::{Deserialize, Serialize};

use crate::output::{self, Style};

/// Arguments for `mda eval`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory with `docs/`, `queries.jsonl` and optionally `cards.json`.
    #[arg(long, default_value = "evals/golden")]
    pub golden: PathBuf,
    /// Cutoff for recall@k.
    #[arg(short, long, default_value_t = 5)]
    pub k: usize,
    /// Produce `cards.json` by summarizing the corpus with the configured backend (spends
    /// money on the `api` backend), then evaluate.
    #[arg(long)]
    pub record: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Expect {
    path: String,
    #[serde(default)]
    heading: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Query {
    q: String,
    expect: Vec<Expect>,
    #[serde(default)]
    temporal: bool,
    #[serde(default)]
    since: Option<String>,
}

/// One run's numbers.
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    /// Which configuration.
    pub run: String,
    /// Queries evaluated.
    pub queries: usize,
    /// Fraction of queries with an expected section in the top k (success@k).
    pub success_at_k: f64,
    /// Mean reciprocal rank of the first expected section within the top k (MRR@k).
    pub mrr_at_k: f64,
    /// Queries that missed, with the top hit for each.
    pub misses: Vec<Miss>,
    /// Mean query latency in milliseconds.
    pub mean_ms: f64,
}

/// A query whose expected section was not in the top k.
#[derive(Debug, Clone, Serialize)]
pub struct Miss {
    /// The query.
    pub q: String,
    /// What was expected.
    pub expected: Vec<String>,
    /// The top hit, if any.
    pub got: Option<String>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let golden = args
        .golden
        .canonicalize()
        .with_context(|| format!("golden set {}", args.golden.display()))?;
    let queries = load_queries(&golden.join("queries.jsonl"))?;
    let cards: Option<HashMap<String, SectionSummary>> =
        match std::fs::read_to_string(golden.join("cards.json")) {
            Ok(text) => Some(serde_json::from_str(&text).context("cards.json")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };

    // A private root: the corpus is copied so nothing under evals/ is ever written.
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    copy_tree(&golden.join("docs"), &root)?;
    let cfg = Config::load(&golden).unwrap_or_default();
    cfg.save(&root)?;
    let mut engine = Engine::open(&root)?;
    let report = engine.index_root()?;
    let cards = if args.record { Some(record_cards(&mut engine, &golden, json)?) } else { cards };

    let mut runs = Vec::new();
    runs.push(evaluate("lexical (raw only)", &engine, &queries, args.k, None, true)?);
    if let Some(cards) = &cards {
        let attached = attach_cards(engine.store_mut(), cards)?;
        let embedder = super::embedder_for(engine.config()).filter(|e| e.ready());
        let mut vectors = 0;
        if let Some(e) = &embedder {
            vectors = engine.embed_pending(&**e, usize::MAX)?.embedded;
        }
        runs.push(evaluate("lexical (cards + raw)", &engine, &queries, args.k, None, false)?);
        if embedder.is_some() {
            runs.push(evaluate(
                "hybrid (cards + raw + vectors)",
                &engine,
                &queries,
                args.k,
                embedder.as_deref(),
                false,
            )?);
        }
        tracing::info!(attached, vectors, "golden cards attached");
    }

    if json {
        output::json(&serde_json::json!({
            "golden": golden,
            "files": report.files,
            "sections": engine.store().counts()?.sections,
            "queries": queries.len(),
            "temporal_queries": queries.iter().filter(|q| q.temporal).count(),
            "k": args.k,
            "runs": runs,
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    println!(
        "{} {} · {} files · {} sections · {} queries ({} temporal) · k={}",
        st.bold("eval"),
        st.dim(&golden.display().to_string()),
        report.files,
        engine.store().counts()?.sections,
        queries.len(),
        queries.iter().filter(|q| q.temporal).count(),
        args.k
    );
    println!("{:<34} {:>9} {:>7} {:>8}", "run", "success@k", "MRR@k", "mean ms");
    for r in &runs {
        println!("{:<34} {:>9.3} {:>7.3} {:>8.1}", r.run, r.success_at_k, r.mrr_at_k, r.mean_ms);
    }
    if let Some(last) = runs.last()
        && !last.misses.is_empty()
    {
        println!("{} ({}):", st.warn("misses in the last run"), last.misses.len());
        for m in &last.misses {
            println!(
                "  {:?} → expected {} · got {}",
                m.q,
                m.expected.join(" | "),
                m.got.as_deref().unwrap_or("nothing")
            );
        }
    }
    if cards.is_none() {
        println!(
            "  {} no cards.json in the golden set: only the lexical run was measured",
            st.dim("note:")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Summarize the corpus with the configured backend and write `cards.json` next to it.
fn record_cards(
    engine: &mut Engine,
    golden: &Path,
    json: bool,
) -> anyhow::Result<HashMap<String, SectionSummary>> {
    let backend = mda_core::worker::backend_for(engine.config())
        .context("preparing the backend for --record (run `mda doctor`)")?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let report = rt.block_on(engine.summarize_pending(
        backend,
        tokio_util::sync::CancellationToken::new(),
        mda_core::pipeline::SummarizeOptions::default(),
        |_| {},
    ))?;
    let mut cards = HashMap::new();
    for doc in engine.store().documents()? {
        for s in engine.store().sections_of(&doc.doc_id)? {
            if let Some(summary) = s.summary {
                cards.insert(s.section_hash, summary);
            }
        }
    }
    let path = golden.join("cards.json");
    std::fs::write(&path, serde_json::to_string_pretty(&cards)?)?;
    if !json {
        println!(
            "recorded {} card(s) to {} · {} failed · ${:.4}",
            cards.len(),
            path.display(),
            report.failed,
            report.pool.usage.cost_usd
        );
    }
    // The store already has these cards attached; the caller re-attaches harmlessly.
    Ok(cards)
}

fn load_queries(path: &Path) -> anyhow::Result<Vec<Query>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let q: Query =
            serde_json::from_str(line).with_context(|| format!("{}:{}", path.display(), n + 1))?;
        out.push(q);
    }
    anyhow::ensure!(!out.is_empty(), "no queries in {}", path.display());
    Ok(out)
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    for entry in walkdir(from)? {
        let rel = entry.strip_prefix(from)?;
        let dest = to.join(rel);
        if std::fs::symlink_metadata(&entry)?.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

/// Regular files and directories under `dir`. Symlinks and anything else are skipped, so a
/// link in the fixture can neither pull outside files into the corpus nor loop.
fn walkdir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| d.display().to_string())? {
            let p = e?.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if meta.is_dir() {
                stack.push(p.clone());
                out.push(p);
            } else if meta.is_file() {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn attach_cards(
    store: &mut Store,
    cards: &HashMap<String, SectionSummary>,
) -> anyhow::Result<usize> {
    let mut n = 0;
    let prov = Provenance {
        model: "golden".to_owned(),
        prompt_version: "recorded".to_owned(),
        schema_version: SCHEMA_VERSION,
        backend: "golden".to_owned(),
        summarized_at: jiff::Timestamp::now(),
        truncated: false,
    };
    for p in store.pending_hashes(usize::MAX)? {
        if let Some(card) = cards.get(&p.section_hash) {
            store.attach_summary(&p.section_hash, card, &prov, &Usage::default())?;
            n += 1;
        }
    }
    Ok(n)
}

fn evaluate(
    name: &str,
    engine: &Engine,
    queries: &[Query],
    k: usize,
    embedder: Option<&dyn mda_core::embed::Embedder>,
    raw_only: bool,
) -> anyhow::Result<Metrics> {
    let mut hits_at_k = 0usize;
    let mut rr_sum = 0.0f64;
    let mut ms_sum = 0.0f64;
    let mut misses = Vec::new();
    for q in queries {
        let opts = SearchOptions {
            k,
            raw_only,
            since: q.since.as_deref().map(super::parse_time).transpose()?,
            ..SearchOptions::default()
        };
        let started = std::time::Instant::now();
        let hits = search::search_with(engine.store(), &q.q, &opts, embedder)?;
        ms_sum += started.elapsed().as_secs_f64() * 1000.0;
        let rank = hits.iter().position(|h| {
            q.expect.iter().any(|e| {
                h.rel_path == e.path
                    && e.heading.as_deref().is_none_or(|needle| {
                        h.heading_path.join(" › ").to_lowercase().contains(&needle.to_lowercase())
                    })
            })
        });
        match rank {
            Some(r) => {
                hits_at_k += 1;
                #[allow(clippy::cast_precision_loss)]
                {
                    rr_sum += 1.0 / (r as f64 + 1.0);
                }
            }
            None => misses.push(Miss {
                q: q.q.clone(),
                expected: q
                    .expect
                    .iter()
                    .map(|e| match &e.heading {
                        Some(h) => format!("{} › {h}", e.path),
                        None => e.path.clone(),
                    })
                    .collect(),
                got: hits
                    .first()
                    .map(|h| format!("{} › {}", h.rel_path, h.heading_path.join(" › "))),
            }),
        }
    }
    #[allow(clippy::cast_precision_loss)] // query counts are tiny
    let (n, found) = (queries.len() as f64, hits_at_k as f64);
    Ok(Metrics {
        run: name.to_owned(),
        queries: queries.len(),
        success_at_k: found / n,
        mrr_at_k: rr_sum / n,
        misses,
        mean_ms: ms_sum / n,
    })
}
