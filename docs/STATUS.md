# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: `docs/handoffs/2026-09-22-session3-final-handoff.md`.**
Phase: benchmarks — **B0a `.mdx` ingestion done; B0b leaner payload next**; Phases 0–4 shipped (v0.1.1)        Active plan: docs/plans/2026-09-benchmarks.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card · G5 recall@5 ≥ 0.85

## Done (last 5)
- **B0a `.mdx` ingestion** (benchmark plan): walker accepts `.mdx`; parser excludes the leading ESM block, keeps headings glued to JSX tags, titles pages from front matter (YAML/TOML) or `export const title`; `docs/design/ingestion.md` with the numbers on all four DocsQA corpora at the pinned commits (every heading kept, every non-partial page titled). Benchmark plan v3.2: every model call through the owner's Claude Code login (§0a.3).
- **v0.1.0 + v0.1.1 released** (five archives + `SHA256SUMS` each). Marketplace install verified end to end from a clean state: hook downloads the binary, daemon starts, MCP answers. v0.1.1 fixed the manifest (standard component paths must not be listed in `plugin.json`).
- Release workflow verified end to end on `main` (dry run: five builds, checksums, `bootstrap.sh` install gate all green) after two fixes (cross target on the pinned toolchain, Linux on ubuntu-24.04). Repo public; history scrubbed of workspace ids.
- A/B parity protocol (#9): 12 questions with references, headless runner, Sonnet grader; golden corpus parity 12/12, index reads more source tokens on that tiny corpus (`docs/benchmarks.md`). First release dry run: Intel macOS target missing on the pinned toolchain, fixed on `docs/phase4-wrap`.
- Release pipeline (ADR-0005): `release.yml` (five archives, `SHA256SUMS`, `bootstrap.sh` install gate, conditional notarisation), `check-version.sh` in CI, `bump-version.sh`; local mirror install verified; Codex first-run review 8/8 triaged and fixed.

## Next (max 3, in order)
1. Benchmark plan (`docs/plans/2026-09-benchmarks.md`, v3.2, accepted; every model call through the owner's Claude Code login, §0a.3): B0b leaner hit payload, then B0 harness hardening and the B2 DocsQA ingestion gate (the four repos are cloned at their pinned commits under `~/.cache/markdownattractor/bench/`, the dataset under `bench/docsqa-data`).
2. Launch checklist (plan §8 Phase 4): README leads with the one-line install (`/plugin install markdownattractor --marketplace joe-carr-data/markdownattractor`) and drops the "not released yet" note; short demo; submit to `claude-plugins-community`; recruit design partners.
3. Owner decisions: Apple notarisation secrets; read ledger (schema v4) for `cost`/`status` savings and hit rate. Follow-ups: subtree `index`, live config reload, socket peer auth, time-filter evals, hybrid latency.

## Blockers / open questions
- Repo is **public** since 2026-09-22 (history rewritten to scrub workspace ids; old SHAs in docs are stale). Actions budget is $0 by owner choice; public-repo minutes are free. Earlier today GitHub Actions stopped starting jobs: "The job was not started because recent account payments have failed or your spending limit needs to be increased" (Billing & plans → Actions spending limit; the release dry run's macOS/Windows/arm64 minutes are billed at multipliers). PR #10 (`docs/phase4-wrap`: two release-workflow fixes + handoff) has no CI until that is fixed; merge it after a green job list, then re-run the release dry run.
- API key: the owner decided on 2026-09-22 that it does not need rotating; it lives in `~/.config/markdownattractor/env`.
- Hybrid query latency (≈ 50 ms in-process, 257 ms as a cold process) misses the plan's 30 ms budget; lexical meets it. Recorded, not hidden.
- §13 open decisions left: commit `cards/` or not. (Embeddings and MCP-as-subcommand decided in ADR-0004; daemon per-root in ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-mdx-ingestion.md) — `.mdx` ingestion, 6 findings, all fixed before merge
