//! `mda search <query>` — hybrid search with time and path filters.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;
use mda_core::search::{self, Matched, SearchOptions};

use crate::output::{self, Style};

/// Arguments for `mda search`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The question, as you would type it into a search box.
    pub query: Vec<String>,
    /// Number of hits.
    #[arg(short, long, default_value_t = 8)]
    pub k: usize,
    /// Only search the raw section text (skip cards). Use when a summary might have
    /// dropped the exact identifier you need.
    #[arg(long)]
    pub raw: bool,
    /// Only sections updated since this time: `7d`, `24h`, `2026-09-01`.
    #[arg(long)]
    pub since: Option<String>,
    /// Only sections updated before this time.
    #[arg(long)]
    pub until: Option<String>,
    /// Only documents under this path (relative to the root).
    #[arg(long = "in")]
    pub path_prefix: Option<String>,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let query = args.query.join(" ");
    let opts = SearchOptions {
        k: args.k,
        raw_only: args.raw,
        since: args.since.as_deref().map(super::parse_time).transpose()?,
        until: args.until.as_deref().map(super::parse_time).transpose()?,
        path_prefix: args.path_prefix.clone(),
        ..SearchOptions::for_config(engine.config())
    };
    let embedder = super::embedder_for(engine.config());
    let hits = search::search_with(engine.store(), &query, &opts, embedder.as_deref())?;

    if json {
        output::json(&serde_json::json!({ "query": query, "hits": hits }));
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    if hits.is_empty() {
        println!("{} for {:?}", st.warn("no hits"), query);
        println!("  {} try fewer words, `--raw` for exact identifiers, or `grep`", st.dim("hint:"));
        return Ok(ExitCode::SUCCESS);
    }
    if hits[0].via_or_fallback {
        println!("{}", st.dim("(no section matched every term; showing partial matches)"));
    }
    for (i, h) in hits.iter().enumerate() {
        let heading = if h.heading_path.is_empty() {
            st.dim("(preamble)")
        } else {
            h.heading_path.join(" › ")
        };
        let matched = match (h.matched, h.vector) {
            (Matched::Both, false) => "",
            (Matched::Both, true) => " · +vec",
            (Matched::Cards, false) => " · card",
            (Matched::Cards, true) => " · card+vec",
            (Matched::Raw, false) => " · raw",
            (Matched::Raw, true) => " · raw+vec",
            (Matched::Vector, _) => " · vec",
        };
        let pending = if h.pending { st.warn(" (pending)") } else { String::new() };
        println!(
            "{:>2}. {} › {}   {} · ~{} tok · {}{}{}",
            i + 1,
            st.bold(&h.rel_path),
            heading,
            st.accent(&format!("L{}–{}", h.line_start, h.line_end)),
            h.token_estimate,
            super::humanize_age(h.updated_at),
            st.dim(matched),
            pending,
        );
        let line = h.tldr.as_deref().unwrap_or(&h.snippet);
        println!("    {line}");
        println!("    {}", st.dim(&format!("mda open {}", h.section_id)));
    }
    Ok(ExitCode::SUCCESS)
}
