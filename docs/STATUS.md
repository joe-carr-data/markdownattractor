# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: `docs/handoffs/2026-09-22-phase4-handoff.md`.**
Phase: 4 — **launch: first-run UX (#7), release pipeline (#8) and A/B parity (#9) merged; release dry run being fixed**; Phases 0–2 merged (#4–#6)        Active plan: docs/plans/2026-09-phase4-launch.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card · G5 recall@5 ≥ 0.85

## Done (last 5)
- A/B parity protocol (#9): 12 questions with references, headless runner, Sonnet grader; golden corpus parity 12/12, index reads more source tokens on that tiny corpus (`docs/benchmarks.md`). First release dry run: Intel macOS target missing on the pinned toolchain, fixed on `docs/phase4-wrap`.
- Release pipeline (ADR-0005): `release.yml` (five archives, `SHA256SUMS`, `bootstrap.sh` install gate, conditional notarisation), `check-version.sh` in CI, `bump-version.sh`; local mirror install verified; Codex first-run review 8/8 triaged and fixed.
- Plugin watched live in a headless Claude Code session (`--plugin-dir`): MCP server connects, tools answer (`design/mcp.md`). First-run UX: `mda start` example query (5.4 s on a fresh `docs/`), `mda cost`, `mda diagnostics` (redacted), `mda nudge [--global]`, `mda index <dir>`.
- Phase 2 search layer (ADR-0004): card embeddings (fastembed bge-small, static ONNX, schema v3), vector list fused with the two BM25 lists, `explain`, `timeline|recent|stale`, `rebuild --embeddings`, `embeddings`, MCP server `mda mcp` (7 tools, rmcp client test), `.mcp.json`, `mda eval` + golden set. Hybrid success@5 0.983 / MRR 0.853 vs 0.883 / 0.747 lexical (`docs/benchmarks.md`).
- Windows hotfix (#5): the detached daemon inherited the parent's stdout pipe; std handles are made non-inheritable before spawning (one `unsafe` site, lint forbid→deny).

## Next (max 3, in order)
1. Merge `docs/phase4-wrap` (release target fix + handoff), re-run `gh workflow run release.yml --ref main`, read the job list, fix until all five builds and `publish` pass.
2. Tag `v0.1.0`, confirm the Release (five archives + `SHA256SUMS`), then install the plugin from the marketplace on this machine and watch `bootstrap.sh` fetch the binary.
3. Owner decisions: repo public, Apple notarisation secrets, read ledger (schema v4). Then A/B on a realistic corpus with a leaner hit payload.

## Blockers / open questions
- API key: the owner decided on 2026-09-22 that it does not need rotating; it lives in `~/.config/markdownattractor/env`.
- Hybrid query latency (≈ 50 ms in-process, 257 ms as a cold process) misses the plan's 30 ms budget; lexical meets it. Recorded, not hidden.
- §13 open decisions left: commit `cards/` or not. (Embeddings and MCP-as-subcommand decided in ADR-0004; daemon per-root in ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-first-run.md) — 8 findings, 7 fixed, 1 in part, 0 open
