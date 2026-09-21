# Phase 1 — Summarization engine

Status: **in progress** · started 2026-09-22 · engine + CLI green (rows 1–12), daemon (row 13) next · plan §8 Phase 1

Goal: `mda index <root>` parses every markdown file, makes it raw-text searchable immediately, summarizes new or changed sections through `claude -p`, and `mda search` returns hybrid hits with line ranges. No daemon yet (that is the last step of this phase), no vectors (Phase 2).

## Module contracts

Every module lives in `crates/mda-core/src/`, has its own tests, and knows nothing about the modules around it except through the types below. Built in the order listed; items marked ∥ are independent and can be built concurrently.

| # | Module | Owner | Contract |
|---|---|---|---|
| 1 | `markdown` | done | `parse_str(&str) -> Document`, `parse_file(&Path) -> Result<Document>` |
| 2 | `card` | done | `SectionSummary` (LLM contract, schema source of truth), `SectionCard`, `Provenance` |
| 3 | `config` | done | `Config::load(root)`, defaults |
| 4 ∥ | `store` | agent A | SQLite. `Store::open(path)`, `Store::open_in_memory()`. Tables: `docs`, `sections`, `summaries` (keyed by `section_hash`, so moved/duplicated sections reuse a summary), `jobs`, `events`, `meta`; FTS5 `sections_raw_fts` (heading_path, text) and `cards_fts` (heading_path, tldr, summary, keywords, questions_answered, entities). `upsert_document(path, &Document, times) -> Delta` inserts/updates sections, refreshes line ranges for all, attaches existing summaries by hash, and returns which hashes need summarization. `attach_summary(hash, SectionSummary, Provenance)`. `mark_failed(hash, reason)`. `search_raw(q, k)`, `search_cards(q, k)` return `(section_id, bm25)`. `section(id)`, `document(id)`, `pending_hashes(limit)`, `counts() -> Counts`, `timeline(since, until)`. Migrations by `meta.schema_version`. WAL mode. |
| 5 ∥ | `walk` | agent A | `discover(root, &Config) -> Vec<PathBuf>`: markdown files under root, honouring `.gitignore`, `.markdownattractorignore`, `config.ignore`, always skipping `.markdownattractor/`. Deterministic order. |
| 6 ∥ | `worker` | agent B | `trait Backend { async fn summarize(&self, req: &SummarizeRequest) -> Result<Outcome> }`. `ClaudeCli` backend: spawns `claude -p` per ADR-0001, writes chunk to stdin and closes it, enforces the wall-clock timeout, parses the result JSON, classifies into `Outcome::{Ok{summary, usage}, Retryable(reason), RateLimited, Fatal(reason), Malformed{raw}}` per plan §4.2. `Mock` backend for tests. `Pool`: runs requests with AIMD concurrency (start 4, +1 after 8 consecutive successes, halve on `RateLimited`), retry policy (1 retry on `Retryable`/`Malformed`, escalation model if configured, backoff 1/4/16 s on `RateLimited`), per-job outcome reporting. Fixtures: `tests/fixtures/claude/*.json` are real CLI outputs. |
| 7 | `diff` | me | `SectionDelta::compute(old: &[(hash, idx)], new: &Document)` → `{ new_hashes, unchanged, removed }`. Pure. |
| 8 | `planner` | me | `plan(&Document, max_tokens) -> Vec<Chunk>`. v1: one section = one chunk; a section over `max_tokens` is truncated head-first and flagged `truncated` in provenance. |
| 9 | `validate` | me | `validate(&Section, SectionSummary) -> Validated { summary, dropped_dates, truncated_lists }`. Caps applied (lists trimmed, not rejected); a date whose `evidence` is not a substring of the section after normalisation (quotes/dashes folded, whitespace collapsed) is dropped; empty `tldr` is a rejection. |
| 10 | `pipeline` | me | `Engine { store, backend, config }`. `index_file(path)`: parse → upsert (raw-searchable now) → enqueue new hashes. `index_root()`: walk + index_file each, priority by size (small first). `summarize_pending()`: drain the queue through the pool, validate, attach. `open(section_id) -> Opened { lines, stale }` re-hashes at read time. |
| 11 | `search` | me | `search(store, query, opts) -> Vec<Hit>`: BM25 from both FTS tables, reciprocal rank fusion, recency prior on `updated_at`; each hit carries `matched: Cards\|Raw\|Both`, `pending`, line range, tldr or raw snippet. |
| 12 | CLI | me | `mda index [path] [--no-summarize]`, `mda search <q> [--raw] [-k] [--since]`, `mda open <section_id>`, `mda card <id>`, `mda status`. `--json` everywhere. |
| 13 | watcher + daemon | later this phase | `notify` watcher → debounce → `index_file`; `mda start/stop`; Unix socket for `status`. Separate plan section when 1–12 are green. |

## Exit criteria

- [x] `mda index` makes every section raw-searchable in < 1 s (full `docs/`: 119 sections, 28 ms) and all cards available in < 2 min (113 model calls in 1 min 53 s, AIMD 4→16, 0 failures, $0.63).
- [x] `mda search` returns the right section with its line range and `--since` filters; `mda open` returns exactly those lines and flags `stale` when the file moved on.
- [x] Re-running `mda index` after touching one section re-summarizes exactly one section (test `editing_one_section_needs_exactly_one_card` + CLI test).
- [x] Inserting lines above an unchanged section updates its line range without a job (front-matter test).
- [x] Coverage 84% workspace-wide; every module has unit tests; pipeline end-to-end with the mock backend; worker fixture tests for every outcome class; CLI end-to-end tests.
- [ ] Codex review of `crates/` filed and triaged.
- [ ] `docs/design/summarization.md` and `docs/design/search.md` written from the code as built.

## Decisions taken in this phase

- Summaries are keyed by `section_hash`, not by `section_id`. A section that moves within a file or appears verbatim in another file gets its card for free. `section_created_at` is when the hash was first seen.
- `doc_id` = first 16 hex chars of blake3(relative path). Renames are detected later (Phase 1 daemon step) via same content hash + delete/create pair.
- Time in the store is RFC 3339 UTC text, produced by `jiff`. SQLite compares it lexically, which is correct for that format.
