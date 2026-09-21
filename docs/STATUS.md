# STATUS (updated 2026-09-22 by Claude)
Phase: 1 — summarization engine (engine + CLI done, daemon next)        Active plan: docs/plans/2026-09-phase1-engine.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 20 s save→card

## Done (last 5)
- Backends per ADR-0002, all three exercised live: `api` default (10/10 cards in 15 s, 1 turn each, $0.043 on docs/plans; workspace-id header; schema enums normalised for structured outputs), `local` (gpt-oss-20b 9/9 once memory-resident, Gemma 3 4B usable; pool 2→4, 300 s), `claude-cli` opt-in behind a policy acknowledgement. `mda backend`, per-backend `mda doctor`, local-model guide. 216 tests.
- Codex review of `crates/` (14 findings, 3 High) triaged and all fixed: path containment, pipe deadlock, conditional store updates, usage ledger + schema v2, retry admission, nonce delimiters, iso/precision grounding, filter-before-cut search, hard-capped chunks, truncation provenance, stable open ids.
- Engine end to end live on all of docs/: 119 sections raw-searchable in 28 ms, 113 model calls in 1 m 53 s, 0 failures, $0.63.
- Fixed the two real-world failure modes the live run exposed: sections that look like instructions (prompt v2 + `<section>` data delimiters, ADR-0001 amended) and heading-only sections (deterministic cards). Escalation defaults to Sonnet.
- Large-corpus controls: `--limit`, daily token budget enforcement (deferred sections stay pending), `--retry-failed`.
- 165 tests (unit, snapshot, proptest, fixture, mock-backend e2e, CLI e2e), coverage 84%, clippy pedantic clean, CI on three OSes.

## Next (max 3, in order)
1. Daemon step (plan row 13): `notify` watcher → debounce → `index_file`, `mda start|stop|watch`, Unix socket status; priority queue (user edits before backfill).
2. Phase 2: sqlite-vec + fastembed, MCP server (`mda mcp`), `mda timeline|recent|stale|explain`, evals harness with the 30-doc golden set.
3. Prompt experiment: get Haiku to call StructuredOutput on turn one reliably (halves input tokens).

## Blockers / open questions
- Rotate the API key used today (it passed through the chat transcript); it lives in `~/.config/markdownattractor/env` with the workspace id.
- Cost/turn optimisation: most calls still take 2 API turns (enforce reminder); a prompt experiment could halve input tokens.
- §13 open decisions: default embedding model, commit `cards/` or not, single vs. separate MCP binary, per-root vs. global daemon.

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-crates.md) — 14 findings, all accepted and fixed, 0 open
