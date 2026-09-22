# Phase 2 — Search layer: vectors, MCP, time commands, evals

Status: **in progress** · started 2026-09-22 · plan §8 Phase 2 · decisions in ADR-0004 · branch `feat/phase2-search`

Goal: Claude searches through MCP and gets hybrid hits fused from cards, raw text and card embeddings; `mda timeline|recent|stale|explain` answer the time questions; `mda eval` measures recall on a golden set so G5 (recall@5 ≥ 0.85) is a number, not a hope.

## Module contracts

| # | Module | Contract |
|---|---|---|
| 1 | `store` (v3) | `embeddings(section_hash PK REFERENCES summaries, model, dim, vector BLOB)`; `put_embedding(hash, model, &[f32])`, `vector_set(model) -> VectorSet { hashes, dim, data }` (one contiguous `Vec<f32>`), `cards_without_embedding(model, limit) -> Vec<CardText>` (hash + the fields to embed, live sections only), `embedding_counts(model) -> (embedded, carded)`, `delete_embeddings_not(model)`. `recent_documents(n)`, `documents_with_pending() -> Vec<(StoredDocument, pending)>`. Migration appended to `MIGRATIONS`. |
| 2 | `embed` | `trait Embedder: Send + Sync { fn model(&self) -> &str; fn dim(&self) -> usize; fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>; }`. `LocalEmbedder` (fastembed `BGESmallENV15Q`, lazily initialised behind a `Mutex`, download on first use with `with_show_download_progress(false)` and our own one-line log), `embed_text(title, heading_path, &SectionSummary) -> String`, `normalise(&mut [f32])`, `cache_dir(&Config) -> PathBuf`, `embedder_for(&Config) -> Option<Arc<dyn Embedder>>` (`None` when `embeddings = "off"`), `check(&Config) -> EmbedCheck { model, cached: bool, dir }` for doctor. |
| 3 | `config` | `embeddings: Embeddings { LocalSmall (default), Off }`, `embedding_cache_dir: Option<PathBuf>`. |
| 4 | `search` | `VectorIndex::load(store, model) -> Option<VectorIndex>`, `top_k(&self, query: &[f32], k) -> Vec<(section_id, score)>` (hash → live section ids through the store). `search(store, query, opts, embedder: Option<&dyn Embedder>)`: third RRF list; `Hit.vector: bool`, `Matched::Vector`; `SearchOptions.vectors: bool`. `explain(store, query, k, embedder) -> Explain { cards: Vec<FtsHit>, raw: Vec<FtsHit>, vector: Vec<(id, score)>, fused: Vec<Hit> }`. |
| 5 | `pipeline` | `Engine::embed_pending(&mut self, embedder, limit) -> EmbedReport { embedded, skipped, ms }` in batches of 32 on `spawn_blocking`; `summarize_pending` takes `Option<Arc<dyn Embedder>>` and embeds what it attached; `Engine::stale() -> Vec<StaleDoc { rel_path, pending, changed_on_disk }>`; `Engine::recent(n) -> Vec<RecentDoc>`. |
| 6 | `daemon` | after every round and once when idle: `embed_pending`; `LiveStatus.embedded: u64`, event `Embedded { count, ms }`; the summarizer owns the embedder. |
| 7 | `mcp` | `McpServer { engine: Mutex<Engine>, embedder }`, `#[tool_router]` with `mda_search(query, k?, since?, until?, path_prefix?, raw?)`, `mda_card(section_id)`, `mda_open(section_id)`, `mda_timeline(since?, until?, path_prefix?, limit?)`, `mda_recent(n?)`, `mda_stale()`, `mda_status()`; `serve_stdio(root) -> Result<()>`; instructions text = the search-first rules from the skill. Structured results via `Json<T>` of the CLI's own types. |
| 8 | CLI | `mda mcp [--root]`, `mda explain <q>`, `mda timeline [--since] [--until] [--in] [--limit]`, `mda recent [n]`, `mda stale`, `mda rebuild --embeddings`, `mda embeddings <local-small|off>`, `mda eval --golden <dir> [-k]`; `status` shows embedding coverage; `doctor` gets an `embeddings` check; `search` prints `vec` in the matched column. `.mcp.json` in the plugin root; `plugin.json` points at it; the skill mentions the tools. |
| 9 | evals | `evals/golden/docs/*.md` (30 documents: ADRs, runbooks, changelogs, meeting notes, specs), `evals/golden/queries.jsonl` (60 queries with expected `rel_path` + heading substring, a third of them temporal), `evals/README.md`. `mda eval` indexes the corpus into a temp root with `--no-summarize` (lexical-only run) and, when `cards.json` recordings exist, attaches them (hybrid run); prints recall@k, MRR, misses; `docs/benchmarks.md` records the numbers. |

## Tasks

- [x] ADR-0004 — written.
- [x] Store v3 + tests (round trip, model filter, live-only, counts).
- [x] `config` embeddings fields; `embed` module with `LocalEmbedder` (ignored live test that downloads the model once) and `embed_text` tests.
- [x] `search`: `VectorIndex`, third list, `Matched::Vector`, `explain`; tests with hand-made vectors.
- [x] `pipeline`: `embed_pending`, `stale`, `recent`; embedding after cards in `summarize_pending`; tests with a fake embedder.
- [x] `daemon`: embed pass + status/event.
- [x] `mcp` module + `mda mcp`; stdio round-trip test (rmcp `client` + `transport-child-process` as dev-dependency) covering `mda_search` and `mda_open`.
- [x] CLI: `explain`, `timeline`, `recent`, `stale`, `rebuild --embeddings`, `embeddings`, `eval`; `status`/`doctor`/`search` updates; `.mcp.json`, `plugin.json`, skill.
- [x] `evals/golden` corpus and queries; `mda eval`; `docs/benchmarks.md` with lexical-only vs hybrid numbers.
- [x] Live: model downloaded once (33 MB), 162 `docs/` cards embedded in 38 s, latency measured (`docs/benchmarks.md`). MCP server exercised through an rmcp client in tests; loading through the plugin in Claude Code is still to be seen by hand.
- [ ] Docs: `design/search.md` updated (vectors, explain), `design/mcp.md`, README, CHANGELOG, STATUS, aha, index.
- [ ] Codex review of the phase, triaged.

## Exit criteria

- [x] recall@5 0.983 / MRR 0.853 hybrid vs 0.883 / 0.747 lexical-only on the golden set (`docs/benchmarks.md`).
- [ ] **Not met as written**: lexical p50 is 2–3 ms in-process (20 ms as a process); hybrid is ≈ 50–60 ms in-process because of the query embedding, 257 ms as a fresh process (model load). The scan itself is microseconds at this size. Recorded in `docs/benchmarks.md`; the MCP server amortises the load.
- [x] `mda mcp` answers `tools/list` and every tool over stdio in `mcp_cli.rs`; `.mcp.json` written per the plugin reference (loading in Claude Code to be confirmed by hand).
- [x] `embeddings = "off"` never downloads (daemon and CLI tests run with it); a failed or absent model leaves search lexical (`Embedder::ready`), `mda index` prints one warning.
- [x] `timeline`, `recent`, `stale`, `explain` work on `docs/` with `--json` (engine tests + live).
- [ ] Codex review filed with every finding triaged.
