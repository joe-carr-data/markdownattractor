# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: the newest file in `docs/handoffs/`.**
Phase: 2 — **search layer merged (#6, main `28fafa6`)**; Phase 1 merged (#4, #5)        Active plan: docs/plans/2026-09-phase2-search.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card · G5 recall@5 ≥ 0.85

## Done (last 5)
- Phase 2 search layer (ADR-0004): card embeddings (fastembed bge-small, static ONNX, schema v3), vector list fused with the two BM25 lists, `explain`, `timeline|recent|stale`, `rebuild --embeddings`, `embeddings`, MCP server `mda mcp` (7 tools, rmcp client test), `.mcp.json`, `mda eval` + golden set. Hybrid success@5 0.983 / MRR 0.853 vs 0.883 / 0.747 lexical (`docs/benchmarks.md`).
- Windows hotfix (#5): the detached daemon inherited the parent's stdout pipe; std handles are made non-inheritable before spawning (one `unsafe` site, lint forbid→deny).
- Daemon and watcher (#4, ADR-0003): `mda start|stop|restart|watch|pause|resume`, live `status`/`doctor`, `index` delegation; save→raw 1.27 s, save→card 4.57 s; Codex review 16/16 addressed.
- CI verified green on all three OSes on main.
- Backends per ADR-0002; engine end to end live on docs/.

## Next (max 3, in order)
1. Confirm by hand that `.mcp.json` loads in Claude Code (`claude plugin validate .` passes; `/reload-plugins`, call `mda_search`); then write the Phase 4 plan (`docs/plans/2026-09-phase4-launch.md`).
2. First-run UX (plan §9.5): `mda start` example query, `mda cost`, `mda diagnostics`, `mda nudge on|off`; hybrid latency lever if it matters (query encoder / ONNX threads).
3. Release pipeline (Phase 4): cargo-dist, GitHub Releases + `SHA256SUMS` for bootstrap.sh; the A/B answer-quality parity protocol (plan §11).

## Blockers / open questions
- API key: the owner decided on 2026-09-22 that it does not need rotating; it lives in `~/.config/markdownattractor/env`.
- Hybrid query latency (≈ 50 ms in-process, 257 ms as a cold process) misses the plan's 30 ms budget; lexical meets it. Recorded, not hidden.
- §13 open decisions left: commit `cards/` or not. (Embeddings and MCP-as-subcommand decided in ADR-0004; daemon per-root in ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-phase2.md) — 17 findings, 14 fixed, 2 in part, 1 rejected with reason, 0 open
