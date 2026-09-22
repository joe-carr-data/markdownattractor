# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: the newest file in `docs/handoffs/`.**
Phase: 4 — **launch: first-run UX merged (#7); release pipeline on `feat/release-pipeline` (#8)**; Phases 0–2 merged (#4–#6)        Active plan: docs/plans/2026-09-phase4-launch.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card · G5 recall@5 ≥ 0.85

## Done (last 5)
- Release pipeline (ADR-0005): `release.yml` (five archives, `SHA256SUMS`, `bootstrap.sh` install gate, conditional notarisation), `check-version.sh` in CI, `bump-version.sh`; local mirror install verified; Codex first-run review 8/8 triaged and fixed.
- Plugin watched live in a headless Claude Code session (`--plugin-dir`): MCP server connects, tools answer (`design/mcp.md`). First-run UX: `mda start` example query (5.4 s on a fresh `docs/`), `mda cost`, `mda diagnostics` (redacted), `mda nudge [--global]`, `mda index <dir>`.
- Phase 2 search layer (ADR-0004): card embeddings (fastembed bge-small, static ONNX, schema v3), vector list fused with the two BM25 lists, `explain`, `timeline|recent|stale`, `rebuild --embeddings`, `embeddings`, MCP server `mda mcp` (7 tools, rmcp client test), `.mcp.json`, `mda eval` + golden set. Hybrid success@5 0.983 / MRR 0.853 vs 0.883 / 0.747 lexical (`docs/benchmarks.md`).
- Windows hotfix (#5): the detached daemon inherited the parent's stdout pipe; std handles are made non-inheritable before spawning (one `unsafe` site, lint forbid→deny).
- Daemon and watcher (#4, ADR-0003): `mda start|stop|restart|watch|pause|resume`, live `status`/`doctor`, `index` delegation; save→raw 1.27 s, save→card 4.57 s; Codex review 16/16 addressed.

## Next (max 3, in order)
1. Read the job list of the release workflow dry run (`gh run list --workflow=release.yml`), fix what it shows, merge PR #8.
2. A/B parity protocol (plan §11): `evals/ab/questions.jsonl`, `scripts/eval/{ab,grade}.sh` (drafted and piloted: 3/3 parity, index reads more source tokens on the tiny golden corpus), first full numbers into `docs/benchmarks.md`.
3. First real release: `bump-version.sh 0.1.0`? (already 0.1.0) → tag `v0.1.0` once the dry run is green; then decide with the owner on making the repo public.

## Blockers / open questions
- API key: the owner decided on 2026-09-22 that it does not need rotating; it lives in `~/.config/markdownattractor/env`.
- Hybrid query latency (≈ 50 ms in-process, 257 ms as a cold process) misses the plan's 30 ms budget; lexical meets it. Recorded, not hidden.
- §13 open decisions left: commit `cards/` or not. (Embeddings and MCP-as-subcommand decided in ADR-0004; daemon per-root in ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-first-run.md) — 8 findings, 7 fixed, 1 in part, 0 open
