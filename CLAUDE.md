# markdownattractor — charter

A Claude Code plugin that turns every folder of markdown into a time-aware, searchable knowledge layer Claude reads *before* it reads your files. One Rust binary is watcher, summarizer orchestrator (spawns the user's `claude -p`, no API key), hybrid index (FTS5 + sqlite-vec + time), MCP server and CLI. Claude searches the index, reads a ~200-token card, and opens only the lines it needs. Every hit carries provenance: file, section, line range, and when it was created, changed and last seen.

## Goals (optimise for these, in this order when they conflict)

- **G6 Trust** — never hallucinated metadata; every extracted date/entity has evidence in the source.
- **G1 Speed** — p50 < 15 s from save to card available.
- **G3 Token economy** — ≥ 5× fewer source tokens read per answer, at answer-quality parity.
- **G5 Searchability** — recall@5 ≥ 0.85 on the golden set; time-filtered queries work.
- **G4 Temporal provenance** — every card dated; sections have their own `updated_at`.
- **G2 Zero-config** — `/mda start` and forget.

## Non-goals (v1)

Not a note-taking app or wiki editor. No cloud, no hosted service, no telemetry. Markdown only. **Never writes into the user's source files.**

## Golden rules

1. Never modify a file under the watched root. `doc_id` lives only in SQLite.
2. Hash before summarise: unchanged sections never go to the LLM.
3. Every extracted date/entity carries an `evidence` substring that must exist in the section, or it is dropped.
4. Line ranges are refreshed on every parse; `mda_open` re-hashes at read time and flags `stale`.
5. Raw section text is always FTS-indexed at parse time, before any LLM call — search never depends on the queue.
6. Every hook exits 0 immediately when `MARKDOWNATTRACTOR_WORKER=1` (recursion guard).
7. Never read OAuth tokens from the keychain or `~/.claude`. Workers are spawned `claude -p` only.
8. No new crate without an ADR in `docs/adr/`. `cargo clippy -D warnings` clean before merge.
9. Every CLI command supports `--json`. Every command prints one line and, if relevant, a log link.
10. No top-level `bin/` in the plugin; the binary lives in `${CLAUDE_PLUGIN_DATA}/bin`.

## How to orient yourself

1. Read `docs/STATUS.md` — current phase, active plan, next steps, blockers.
2. Read the plan it points to under `docs/plans/`.
3. Read `docs/aha.md` — things learned the hard way.
4. `docs/index.md` maps everything else. The full design is `docs/project-plan.md` (reference tier; read sections, not the whole file).

## Working rules

- Plan mode before anything medium or larger; plans go to `docs/plans/` with exit criteria before code.
- Small commits: `feat|fix|docs|chore(scope): …`.
- Before ending a task: update `docs/STATUS.md`; add to `docs/aha.md` if something was learned; ADR if something was decided.
- Codex reviews every PR touching `crates/`; findings are triaged in `docs/reviews/codex/`, never silently dropped.
