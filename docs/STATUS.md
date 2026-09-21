# STATUS (updated 2026-09-22 by Claude)
Phase: 1 — summarization engine (engine + CLI done, daemon next)        Active plan: docs/plans/2026-09-phase1-engine.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 20 s save→card

## Done (last 5)
- Engine end to end: `mda index | search | open | card | status`. Live on docs/plans: 11 sections raw-searchable in 6 ms, 11/11 carded in ~45 s over two runs, $0.10 list price, cards searchable by question.
- Fixed the two real-world failure modes the live run exposed: sections that look like instructions (prompt v2 + `<section>` data delimiters, ADR-0001 amended) and heading-only sections (deterministic cards). Escalation defaults to Sonnet.
- Large-corpus controls: `--limit`, daily token budget enforcement (deferred sections stay pending), `--retry-failed`.
- 165 tests (unit, snapshot, proptest, fixture, mock-backend e2e, CLI e2e), coverage 84%, clippy pedantic clean, CI on three OSes.
- Plugin skeleton validated: manifest, marketplace, `/mda` + `search-first` skills, bootstrap + nudge hooks.

## Next (max 3, in order)
1. Codex review of `crates/` (`/codex-review crates/`), triage, file under `docs/reviews/codex/`.
2. Daemon step (plan row 13): `notify` watcher → debounce → `index_file`, `mda start|stop|watch`, Unix socket status; priority queue (user edits before backfill).
3. `docs/design/summarization.md` + `docs/design/search.md` from the code as built; then Phase 2 (vectors, MCP server).

## Blockers / open questions
- Login-policy confirmation (spike exit criterion) — owner action, not blocking code.
- Cost/turn optimisation: most calls still take 2 API turns (enforce reminder); a prompt experiment could halve input tokens.
- §13 open decisions: default embedding model, commit `cards/` or not, single vs. separate MCP binary, per-root vs. global daemon.

## Last Codex review: 2026-09-21 (docs/reviews/codex/2026-09-21-pre-mortem.md) — 0 findings open, 2 rejected with reasons
