//! `mda explain <query>` — the three ranked lists behind a search and the fused result.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;
use mda_core::search::{self, SearchOptions};

use crate::output::{self, Style};

/// Arguments for `mda explain`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The query to explain.
    pub query: Vec<String>,
    /// Entries per list.
    #[arg(short, long, default_value_t = 8)]
    pub k: usize,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let query = args.query.join(" ");
    let embedder = super::embedder_for(engine.config());
    let opts = SearchOptions { k: args.k, ..SearchOptions::default() };
    let ex = search::explain(engine.store(), &query, &opts, embedder.as_deref())?;

    if json {
        output::json(&serde_json::json!({ "query": query, "explain": ex }));
        return Ok(ExitCode::SUCCESS);
    }
    let st = Style::auto();
    let label = |id: &str| -> String {
        engine.store().section(id).ok().flatten().map_or_else(
            || id.to_owned(),
            |s| format!("{} › {}", s.rel_path, s.heading_path.join(" › ")),
        )
    };
    println!("{} {:?}", st.bold("explain"), query);
    if ex.via_or_fallback {
        println!("{}", st.dim("(no section matched every term; lexical lists use the OR form)"));
    }
    println!("{} (bm25 over tldr/summary/keywords/questions)", st.accent("cards"));
    for (i, h) in ex.cards.iter().enumerate() {
        println!("  {:>2}. {:>7.3}  {}", i + 1, h.bm25, label(&h.section_id));
    }
    println!("{} (bm25 over section text)", st.accent("raw"));
    for (i, h) in ex.raw.iter().enumerate() {
        println!("  {:>2}. {:>7.3}  {}", i + 1, h.bm25, label(&h.section_id));
    }
    match &ex.vector_note {
        Some(note) => println!("{} ({note})", st.accent("vector")),
        None => println!("{} (cosine over card embeddings)", st.accent("vector")),
    }
    for (i, (id, cos)) in ex.vector.iter().enumerate() {
        println!("  {:>2}. {:>7.3}  {}", i + 1, cos, label(id));
    }
    println!("{} (reciprocal rank fusion × recency, filtered)", st.accent("fused"));
    for (i, h) in ex.fused.iter().enumerate() {
        println!(
            "  {:>2}. {:>7.4}  {} › {}  {:?}{}",
            i + 1,
            h.score,
            h.rel_path,
            h.heading_path.join(" › "),
            h.matched,
            if h.vector { " +vec" } else { "" }
        );
    }
    Ok(ExitCode::SUCCESS)
}
