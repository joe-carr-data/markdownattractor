//! The MCP server behind `mda mcp` (ADR-0004): the same engine the CLI uses, exposed to
//! Claude over stdio as tools. Results are the CLI's own `--json` types, so a skill and a
//! tool call see one format, with one exception: `mda_search` returns [`SearchView`], the
//! CLI hit with the same field names minus the diagnostics (`score`, `vector`,
//! `vector_score`, `title`), a `snippet` only when there is no `tldr`, `pending` only when
//! true, and the OR-fallback flag hoisted to one `partial` field. Every token in a search
//! result is paid on every question, so the view carries what an answer needs and nothing
//! else (`docs/design/mcp.md`).
//!
//! Search never waits for a model download here either: without a ready embedder the hits
//! are lexical, and `mda_status` says so.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::config::Config;
use crate::embed::{self, Embedder};
use crate::pipeline::Engine;
use crate::search::{self, SearchOptions};

/// What `initialize` tells the client about how to use the tools.
pub const INSTRUCTIONS: &str = "markdownattractor indexes this project's markdown into sections \
with summaries (cards). Search first: call mda_search with the user's question (5 hits by \
default; ask for more only when the first five miss), read the tldr of each hit, then call \
mda_open on the one or two section ids you need to quote exact lines, or mda_card for the full \
card. Only read a whole file when the user asks for it or the hit says the section is small. If \
the top hits do not contain the answer, retry mda_search with raw=true (raw text only), and if \
that fails too, fall back to grep and say so. Every hit carries a line range, a token estimate \
and when the section last changed; a hit with pending=true has no card yet, so it shows a raw \
snippet instead of a tldr.";

/// Arguments of `mda_search`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// The question, as the user would type it into a search box.
    pub query: String,
    /// Number of hits (default 5, at most 50).
    #[serde(default)]
    pub k: Option<usize>,
    /// Only sections updated since this time: `7d`, `24h`, `2026-09-01`.
    #[serde(default)]
    pub since: Option<String>,
    /// Only sections updated before this time.
    #[serde(default)]
    pub until: Option<String>,
    /// Only documents whose path (relative to the root) starts with this prefix.
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// Search only the raw section text (skip cards and vectors): the recovery path when a
    /// summary may have dropped the exact identifier you need.
    #[serde(default)]
    pub raw: Option<bool>,
}

/// Arguments of `mda_card` and `mda_open`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SectionParams {
    /// A section id from a search hit, like `3f2a…#4`.
    pub section_id: String,
}

/// Arguments of `mda_timeline`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TimelineParams {
    /// Start of the window: `30d`, `2026-09-01` (default: no lower bound).
    #[serde(default)]
    pub since: Option<String>,
    /// End of the window (default: now).
    #[serde(default)]
    pub until: Option<String>,
    /// Only documents whose path starts with this prefix.
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// Maximum entries (default 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Arguments of `mda_recent`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RecentParams {
    /// How many documents (default 10).
    #[serde(default)]
    pub n: Option<usize>,
}

/// Default number of hits for `mda_search`. Five cards are about 400 tokens; the CLI keeps
/// its own default of 8 because a terminal has room and pays nothing per token.
pub const DEFAULT_K: usize = 5;

/// What `mda_search` returns.
#[derive(Debug, Clone, Serialize)]
pub struct SearchView {
    /// The query as received.
    pub query: String,
    /// `true` when no section matched every term and the hits are OR matches.
    pub partial: bool,
    /// The hits, best first.
    pub hits: Vec<HitView>,
}

/// One search hit as the MCP client sees it: the CLI's [`search::Hit`] field names, without
/// the ranking diagnostics. Built by [`HitView::from`].
#[derive(Debug, Clone, Serialize)]
pub struct HitView {
    /// `<doc_id>#<index>`, for `mda_open` and `mda_card`.
    pub section_id: String,
    /// Path relative to the root.
    pub rel_path: String,
    /// Headings down to the section.
    pub heading_path: Vec<String>,
    /// First line, 1-based.
    pub line_start: u32,
    /// Last line, 1-based, inclusive.
    pub line_end: u32,
    /// Rough token count of the section, so the cost of `mda_open` is known before calling it.
    pub token_estimate: u32,
    /// The card's one-liner, when the section has a card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<String>,
    /// The first ~200 characters of the body; only when there is no card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Which index matched: cards, raw, both, or vector.
    pub matched: search::Matched,
    /// `true` when the section has no card yet; omitted otherwise.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
    /// When the section content last changed, to the second.
    pub updated_at: String,
}

impl From<search::Hit> for HitView {
    fn from(h: search::Hit) -> Self {
        let snippet = if h.tldr.is_none() { Some(h.snippet) } else { None };
        Self {
            section_id: h.section_id,
            rel_path: h.rel_path,
            heading_path: h.heading_path,
            line_start: h.line_start,
            line_end: h.line_end,
            token_estimate: h.token_estimate,
            tldr: h.tldr,
            snippet,
            matched: h.matched,
            pending: h.pending,
            updated_at: h.updated_at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        }
    }
}

/// What `mda_card` returns: the stored section without its raw text.
#[derive(Debug, Clone, Serialize)]
pub struct CardView {
    /// `<doc_id>#<index>`.
    pub section_id: String,
    /// Path relative to the root.
    pub rel_path: String,
    /// Headings down to the section.
    pub heading_path: Vec<String>,
    /// First line, 1-based.
    pub line_start: u32,
    /// Last line, 1-based, inclusive.
    pub line_end: u32,
    /// Rough token count of the section.
    pub token_estimate: u32,
    /// pending, summarized or failed.
    pub state: crate::store::SectionState,
    /// The card, when there is one.
    pub summary: Option<crate::card::SectionSummary>,
    /// How the card was produced.
    pub provenance: Option<crate::card::Provenance>,
    /// When the section content last changed.
    pub updated_at: jiff::Timestamp,
    /// Why summarization failed, when it did.
    pub fail_reason: Option<String>,
}

/// What `mda_status` returns.
#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    /// Watched root.
    pub root: PathBuf,
    /// Store counts.
    pub counts: crate::store::Counts,
    /// Embedding setting and, when on, coverage.
    pub embeddings: EmbeddingStatus,
    /// The daemon's live counters, when one is running.
    pub daemon: Option<crate::daemon::LiveStatus>,
}

/// Embedding state for status.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingStatus {
    /// `local-small` or `off`.
    pub setting: crate::config::Embeddings,
    /// Model name when on.
    pub model: Option<String>,
    /// The model is on disk (or loaded), so searches use vectors.
    pub ready: bool,
    /// Carded hashes with a vector / carded hashes.
    pub embedded: u64,
    /// Total carded hashes.
    pub carded: u64,
}

/// The server: one engine, one optional embedder. Engine work (SQLite, file reads, the
/// query embedding) is blocking, so every tool runs it on the blocking thread pool through
/// the private `blocking` helper and the async runtime stays free to answer other requests.
pub struct McpServer {
    engine: Arc<Mutex<Engine>>,
    embedder: Option<Arc<dyn Embedder>>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer").finish_non_exhaustive()
    }
}

fn internal(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn structured<T: Serialize>(value: &T) -> std::result::Result<CallToolResult, McpError> {
    Ok(CallToolResult::structured(serde_json::to_value(value).map_err(internal)?))
}

fn parse_opt_time(s: Option<&str>) -> std::result::Result<Option<jiff::Timestamp>, McpError> {
    s.map(crate::timefmt::parse_time)
        .transpose()
        .map_err(|e| McpError::invalid_params(e.to_string(), None))
}

fn embedding_status(
    engine: &Engine,
    embedder: Option<&dyn Embedder>,
) -> crate::Result<EmbeddingStatus> {
    let cfg: &Config = engine.config();
    let Some(e) = embedder else {
        return Ok(EmbeddingStatus {
            setting: cfg.embeddings,
            model: None,
            ready: false,
            embedded: 0,
            carded: 0,
        });
    };
    let counts = engine.store().embedding_counts(e.model())?;
    Ok(EmbeddingStatus {
        setting: cfg.embeddings,
        model: Some(e.model().to_owned()),
        ready: e.ready(),
        embedded: counts.embedded,
        carded: counts.carded,
    })
}

#[tool_router]
impl McpServer {
    /// Open the engine for `root` and build the embedder the config asks for.
    pub fn open(root: &Path) -> crate::Result<Self> {
        let engine = Engine::open(root)?;
        let embedder = embed::embedder_for(engine.config());
        Ok(Self { engine: Arc::new(Mutex::new(engine)), embedder })
    }

    /// Run `f` with the engine on the blocking pool.
    async fn blocking<T, F>(&self, f: F) -> std::result::Result<T, McpError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Engine, Option<&dyn Embedder>) -> std::result::Result<T, McpError>
            + Send
            + 'static,
    {
        let engine = Arc::clone(&self.engine);
        let embedder = self.embedder.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard: MutexGuard<'_, Engine> =
                engine.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard, embedder.as_deref())
        })
        .await
        .map_err(|e| internal(format!("tool task failed: {e}")))?
    }

    /// Hybrid search over the markdown index: cards, raw text and card vectors fused, most
    /// recently changed sections favoured. Each hit has a `section_id` for `mda_open`, a line
    /// range, a token estimate, a one-line tldr (or a raw snippet when the section has no card
    /// yet) and when it last changed. Five hits by default.
    #[tool(name = "mda_search")]
    async fn mda_search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let (k, raw_only) = (p.k.unwrap_or(DEFAULT_K).clamp(1, 50), p.raw.unwrap_or(false));
        let since = parse_opt_time(p.since.as_deref())?;
        let until = parse_opt_time(p.until.as_deref())?;
        let path_prefix = p.path_prefix.clone();
        let query = p.query.clone();
        let hits = self
            .blocking(move |engine, embedder| {
                // The root's tunables (config.toml) are read where the engine is available.
                let opts = SearchOptions {
                    k,
                    raw_only,
                    since,
                    until,
                    path_prefix,
                    ..SearchOptions::for_config(engine.config())
                };
                search::search_with(engine.store(), &query, &opts, embedder).map_err(internal)
            })
            .await?;
        let partial = hits.iter().any(|h| h.via_or_fallback);
        structured(&SearchView {
            query: p.query,
            partial,
            hits: hits.into_iter().map(HitView::from).collect(),
        })
    }

    /// The full card of a section: tldr, summary, keywords, questions it answers, grounded
    /// dates and entities, decisions, action items, and how it was produced.
    #[tool(name = "mda_card")]
    async fn mda_card(
        &self,
        Parameters(p): Parameters<SectionParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let id = p.section_id.clone();
        let s = self
            .blocking(move |engine, _| {
                engine
                    .store()
                    .section(&id)
                    .map_err(internal)?
                    .ok_or_else(|| McpError::invalid_params(format!("no section {id}"), None))
            })
            .await?;
        structured(&CardView {
            section_id: s.section_id,
            rel_path: s.rel_path,
            heading_path: s.heading_path,
            line_start: s.line_start,
            line_end: s.line_end,
            token_estimate: s.token_estimate,
            state: s.state,
            summary: s.summary,
            provenance: s.provenance,
            updated_at: s.updated_at,
            fail_reason: s.fail_reason,
        })
    }

    /// The exact source lines of a section, re-checked against the file at read time. If
    /// the file changed since it was indexed, the current lines are returned with stale=true
    /// and `section_id` is the section's current id.
    #[tool(name = "mda_open")]
    async fn mda_open(
        &self,
        Parameters(p): Parameters<SectionParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let id = p.section_id.clone();
        let opened = self
            .blocking(move |engine, _| {
                engine.open_section(&id).map_err(|e| match e {
                    Error::NotFound(m) => McpError::invalid_params(m, None),
                    other => internal(other),
                })
            })
            .await?;
        structured(&opened)
    }

    /// What was created, changed, renamed or deleted in a time window, oldest first, with
    /// document paths.
    #[tool(name = "mda_timeline")]
    async fn mda_timeline(
        &self,
        Parameters(p): Parameters<TimelineParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let since = parse_opt_time(p.since.as_deref())?;
        let until = parse_opt_time(p.until.as_deref())?;
        let limit = p.limit.unwrap_or(200).clamp(1, 2000);
        let prefix = p.path_prefix.clone();
        let entries = self
            .blocking(move |engine, _| {
                engine.timeline(since, until, prefix.as_deref(), limit).map_err(internal)
            })
            .await?;
        structured(&entries)
    }

    /// The most recently updated documents with their section and pending counts.
    #[tool(name = "mda_recent")]
    async fn mda_recent(
        &self,
        Parameters(p): Parameters<RecentParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let n = p.n.unwrap_or(10).clamp(1, 200);
        let docs = self.blocking(move |engine, _| engine.recent(n).map_err(internal)).await?;
        structured(&docs)
    }

    /// Documents whose index is not final: sections still waiting for a card, failed ones,
    /// or files changed on disk since they were indexed. Empty when the daemon is keeping up.
    #[tool(name = "mda_stale")]
    async fn mda_stale(&self) -> std::result::Result<CallToolResult, McpError> {
        let stale = self.blocking(|engine, _| engine.stale().map_err(internal)).await?;
        structured(&stale)
    }

    /// Index coverage, pending and failed sections, spend, embedding state and whether the
    /// daemon is running.
    #[tool(name = "mda_status")]
    async fn mda_status(&self) -> std::result::Result<CallToolResult, McpError> {
        let (root, counts, embeddings) = self
            .blocking(|engine, embedder| {
                let counts = engine.store().counts().map_err(internal)?;
                let embeddings = embedding_status(engine, embedder).map_err(internal)?;
                Ok((engine.root().to_path_buf(), counts, embeddings))
            })
            .await?;
        let daemon = live_status(&root).await;
        structured(&StatusView { root, counts, embeddings, daemon })
    }
}

async fn live_status(root: &Path) -> Option<crate::daemon::LiveStatus> {
    use crate::daemon::{Client, Request, Response};
    let mut c = Client::connect(root).await.ok()?;
    match c.request(&Request::Status).await {
        Ok(Response::Status(s)) => Some(*s),
        _ => None,
    }
}

#[allow(clippy::unused_async_trait_impl)] // generated by the macro
#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("markdownattractor", crate::VERSION))
            .with_instructions(INSTRUCTIONS)
    }
}

/// Serve MCP over stdin/stdout for `root` until the client disconnects.
pub async fn serve_stdio(root: &Path) -> crate::Result<()> {
    let server = McpServer::open(root)?;
    tracing::info!(root = %root.display(), "mcp server starting on stdio");
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| Error::Daemon(format!("mcp initialize failed: {e}")))?;
    let reason = running
        .waiting()
        .await
        .map_err(|e| Error::Daemon(format!("mcp server task failed: {e}")))?;
    tracing::info!(?reason, "mcp server stopped");
    Ok(())
}
