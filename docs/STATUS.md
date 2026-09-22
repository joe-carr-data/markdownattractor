# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: the newest file in `docs/handoffs/`.**
Phase: 1 — **complete on branch `feat/daemon` (PR #4)**; Phase 2 next        Active plan: docs/plans/2026-09-phase1-daemon.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card

## Done (last 5)
- Daemon and watcher (plan row 13, ADR-0003): `mda start|stop|restart|watch|pause|resume`, live `status`/`doctor`, `index` delegation, bootstrap auto-start. Live on docs/: save→raw 1.27 s, save→card 4.57 s, rename at zero cost. `docs/design/daemon.md`.
- Codex review of the daemon step: 16 findings, 15 fixed + 1 in part (`docs/reviews/codex/2026-09-22-daemon.md`); 241 tests, MSRV 1.89.
- CI verified on GitHub for the first time: all three OSes green on main after one Windows-only `unused_mut` fix.
- Backends per ADR-0002, all three exercised live: `api` default, `local` (llama.cpp), `claude-cli` opt-in.
- Codex review of `crates/` (14 findings) triaged and fixed; engine end to end live on all of docs/.

## Next (max 3, in order)
1. Merge PR #4 when CI is green (Windows job is the slow one, ~15 min); then Phase 2 plan: `docs/plans/2026-10-phase2-search.md`.
2. Phase 2: sqlite-vec + fastembed as a third fused list, MCP server (`mda mcp`, rmcp), `mda timeline|recent|stale|explain`, eval harness with the 30-doc golden set and the answer-quality parity gate (plan §11).
3. First-run UX (plan §9.5): `mda start` example query, `mda cost`, `mda diagnostics`, `mda nudge on|off`; release pipeline (cargo-dist, SHA256SUMS for bootstrap.sh).

## Blockers / open questions
- Rotate the API key (it passed through the chat transcript on 2026-09-22); it lives in `~/.config/markdownattractor/env`.
- Peer authentication on the daemon socket (review F8) is deferred until a multi-user threat model matters.
- §13 open decisions left: default embedding model, commit `cards/` or not, MCP as subcommand vs separate binary. (Per-root daemon: decided, ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-daemon.md) — 16 findings, 15 fixed, 1 fixed in part, 0 open
