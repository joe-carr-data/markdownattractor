# STATUS (updated 2026-09-22 by Claude)
**Start here after compaction: `docs/handoffs/2026-09-22-session4-handoff.md` (then the session-3 handoff for releases and recipes).**
Phase: benchmarks — **B0a, B0b, the DocsQA adapter and the B2 ingestion gate done (100% coverage); cards for the four corpora next**; Phases 0–4 shipped (v0.1.1)        Active plan: docs/plans/2026-09-benchmarks.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 15 s save→card · G5 recall@5 ≥ 0.85

## Done (last 5)
- **DocsQA adapter + ingestion gate**: `mda eval --dataset docsqa` (page-level success@5/MRR@5/nDCG@10, coverage and evidence-anchor report, seeded dev/test/holdout split, in `mda_core::eval`); all four projects at 100% label coverage, anchors 100/100/98/82% (GitHub Docs' gap is rendered Liquid includes); first axis-A row (raw lexical, dev split) is the floor: success@5 0.31/0.22/0.33/0.60. Carding rate through the Claude Code login: ≈ 1.1 sections/s, ≈ $175 equivalent for all four corpora (`docs/benchmarks.md`).
- **B0b lean MCP payload**: `mda_search` returns five hits without ranking diagnostics; golden-corpus median source tokens 1,304.5 → 762.5 at 11/12 parity, but more turns; exploratory (`docs/benchmarks.md`). `ab.sh`/`grade.sh` run through `claude -p`, fail loud, check a run manifest (Codex review 6/6 triaged).
- **B0a `.mdx` ingestion** (benchmark plan): walker accepts `.mdx`; parser excludes the leading ESM block, keeps headings glued to JSX tags, titles pages from front matter (YAML/TOML) or `export const title`; `docs/design/ingestion.md` with the numbers on all four DocsQA corpora at the pinned commits (every heading kept, every non-partial page titled). Benchmark plan v3.2: every model call through the owner's Claude Code login (§0a.3).
- **v0.1.0 + v0.1.1 released** (five archives + `SHA256SUMS` each). Marketplace install verified end to end from a clean state: hook downloads the binary, daemon starts, MCP answers. v0.1.1 fixed the manifest (standard component paths must not be listed in `plugin.json`).
- Release workflow verified end to end on `main` (dry run: five builds, checksums, `bootstrap.sh` install gate all green) after two fixes (cross target on the pinned toolchain, Linux on ubuntu-24.04). Repo public; history scrubbed of workspace ids.

## Next (in order)
1. Benchmark plan (`docs/plans/2026-09-benchmarks.md`): **owner decision** — cards for the four DocsQA corpora through the `claude-cli` backend cost ≈ 10 h and ≈ $175 list-price equivalent (plan §6 said ≤ $60); on a yes, run them in bounded rounds (`--limit`), commit the cards under `evals/results/docsqa/`, then the carded and hybrid axis-A rows on the dev split (full coverage only: partial cards bias the fusion); the query-latency lever (≈ 0.5 s/question raw on the large corpora). Clones live under `~/.cache/markdownattractor/bench/`, the dataset under `bench/docsqa-data` (decompress `corpus.jsonl.gz`).
2. B0 leftovers: transcript-counted tokens (rule 0.7), grounding check in the grader, `FROZEN.md` writer, `panel.sh`, per-arm manifests for qmd/graphify with smoke traces; then the qmd and BM25-over-files arms (axis A) and axis B on the frozen test sample.
3. Launch checklist (plan §8 Phase 4): README leads with the one-line install (`/plugin install markdownattractor --marketplace joe-carr-data/markdownattractor`) and drops the "not released yet" note; short demo; submit to `claude-plugins-community`; recruit design partners.
4. Owner decisions: Apple notarisation secrets; read ledger (schema v4) for `cost`/`status` savings and hit rate. Follow-ups: subtree `index`, live config reload, socket peer auth, time-filter evals, hybrid latency.

## Blockers / open questions
- Repo is **public** since 2026-09-22 (history rewritten to scrub workspace ids; old SHAs in docs are stale). Actions budget is $0 by owner choice; public-repo minutes are free. Earlier today GitHub Actions stopped starting jobs: "The job was not started because recent account payments have failed or your spending limit needs to be increased" (Billing & plans → Actions spending limit; the release dry run's macOS/Windows/arm64 minutes are billed at multipliers). PR #10 (`docs/phase4-wrap`: two release-workflow fixes + handoff) has no CI until that is fixed; merge it after a green job list, then re-run the release dry run.
- API key: the owner decided on 2026-09-22 that it does not need rotating; it lives in `~/.config/markdownattractor/env`.
- Hybrid query latency (≈ 50 ms in-process, 257 ms as a cold process) misses the plan's 30 ms budget; lexical meets it. Recorded, not hidden.
- §13 open decisions left: commit `cards/` or not. (Embeddings and MCP-as-subcommand decided in ADR-0004; daemon per-root in ADR-0003.)

## Last Codex review: 2026-09-22 (docs/reviews/codex/2026-09-22-docsqa-adapter.md) — DocsQA adapter, 6 findings, all fixed before merge
