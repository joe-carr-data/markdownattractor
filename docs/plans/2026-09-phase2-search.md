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

- [ ] ADR-0004 — written.
- [ ] Store v3 + tests (round trip, model filter, live-only, counts).
- [ ] `config` embeddings fields; `embed` module with `LocalEmbedder` (ignored live test that downloads the model once) and `embed_text` tests.
- [ ] `search`: `VectorIndex`, third list, `Matched::Vector`, `explain`; tests with hand-made vectors.
- [ ] `pipeline`: `embed_pending`, `stale`, `recent`; embedding after cards in `summarize_pending`; tests with a fake embedder.
- [ ] `daemon`: embed pass + status/event.
- [ ] `mcp` module + `mda mcp`; stdio round-trip test (rmcp `client` + `transport-child-process` as dev-dependency) covering `mda_search` and `mda_open`.
- [ ] CLI: `explain`, `timeline`, `recent`, `stale`, `rebuild --embeddings`, `embeddings`, `eval`; `status`/`doctor`/`search` updates; `.mcp.json`, `plugin.json`, skill.
- [ ] `evals/golden` corpus and queries; `mda eval`; `docs/benchmarks.md` with lexical-only vs hybrid numbers.
- [ ] Live: model download once, `docs/` embedded, search latency measured; MCP server seen by `claude` through the plugin (manual).
- [ ] Docs: `design/search.md` updated (vectors, explain), `design/mcp.md`, README, CHANGELOG, STATUS, aha, index.
- [ ] Codex review of the phase, triaged.

## Exit criteria

- [ ] recall@5 ≥ 0.85 and MRR reported on the golden set, hybrid; lexical-only reported beside it. If hybrid does not beat lexical-only, say so in `docs/benchmarks.md` and keep vectors off by default.
- [ ] `mda search` p50 < 30 ms on `docs/` + the golden set including the vector scan; the scan stays under 10 ms at 10K synthetic vectors (unit benchmark).
- [ ] `mda mcp` answers `tools/list` and `mda_search`/`mda_open` over stdio in a test; the plugin's `.mcp.json` loads in Claude Code.
- [ ] `embeddings = "off"` never downloads and everything else works; a failed download leaves search lexical with one warning.
- [ ] `timeline`, `recent`, `stale`, `explain` work on `docs/` with `--json`.
- [ ] Codex review filed with every finding triaged.
