# Phase 4 — Launch: plugin verified live, first-run UX, release pipeline, parity protocol

Status: **in progress** · started 2026-09-22 · plan §8 Phase 4, §9 distribution, §9.5 first run, §11 evals · decisions in ADR-0005 (release pipeline)

Goal: a user installs the plugin with one command, types `/mda start`, and sees a real search hit with a line range before walking away; the binary arrives through the bootstrap hook from a checksummed GitHub Release; the README's token-saving claim, when it appears, is backed by the A/B parity protocol. Nothing here changes how the engine indexes or searches.

## Module contracts

| # | Area | Contract |
|---|---|---|
| 1 | Plugin live check | The plugin loaded through `claude --plugin-dir` (or an install) starts `mda mcp` from `.mcp.json`, the tools appear as `mcp__plugin_markdownattractor_markdownattractor__mda_*`, the SessionStart hook starts the daemon for a root that has `.markdownattractor/`, and a headless prompt gets an answer from `mda_search`. What that shows is recorded in `docs/design/mcp.md`; anything broken is fixed in this phase. |
| 2 | `store` | `usage_by_day(since) -> Vec<DailyUsage { day, model, outcome, calls, input_tokens, output_tokens, cost_usd }>` over `usage_log`. No schema change. |
| 3 | `mda cost [--since 7d]` | Spend from the ledger: totals for the window, per day and per model/outcome, plus the all-time total from `counts()`. `--json` gives the rows. States what it does *not* know yet: tokens saved on reads (needs a read ledger, see follow-ups). |
| 4 | `mda diagnostics [--out <file>]` | A redacted JSON bundle for an issue report: version, features, OS/arch, config with the home directory and workspace id redacted (the config never holds a key), store counts and schema version, embedding check, daemon live status, the last 40 daemon log lines with the home directory redacted, `doctor` results. No document content, no paths outside the state dir except the root itself (redacted to `~`). Written to stdout, or to `--out`. |
| 5 | `mda nudge [on\|off] [--global]` | Per root: saves `nudge` in `config.toml`. `--global`: creates or removes the off file at `$MDA_NUDGE_FILE` (the plugin launcher sets it to `${CLAUDE_PLUGIN_DATA}/nudge.off`; without it, `~/.claude/plugins/data/markdownattractor-markdownattractor/nudge.off`, the same fallback the scripts use). No argument shows both. `scripts/nudge.sh` honours both: the global file, then `nudge = false` in the project's config. |
| 6 | `mda start` first run | On a root that had no cards before this start and a working backend, `start` waits up to 60 s for the first cards (10, or every section when there are fewer), picks a question from a card's `questions_answered`, runs it through the search and prints the top hit with its line range and `mda open` id. `--no-example` skips it; the SessionStart hook passes it. JSON: `example: { query, hit }` or `null` with `example_skipped: <reason>`. |
| 7 | Release pipeline (ADR-0005) | `.github/workflows/release.yml` on tag `v*`: builds `mda` for darwin-arm64, darwin-x64 (`--no-default-features`, lexical only), linux-x64, linux-arm64, windows-x64; packs `mda-<os>-<arch>.tar.gz` (the binary at the archive root, `mda.exe` on Windows) and one `SHA256SUMS`; publishes a GitHub Release. macOS binaries are signed and notarised when the Apple secrets exist and shipped unsigned with a note otherwise. `scripts/dev/check-version.sh` fails CI when `VERSION`, `plugin.json`, `marketplace.json` and the workspace version disagree; `scripts/dev/bump-version.sh <x.y.z>` changes all four. `bootstrap.sh` handles `mda.exe`. |
| 8 | A/B parity protocol (plan §11) | `evals/ab/questions.jsonl` (question, reference answer, corpus) over the golden corpus; `scripts/eval/ab.sh` runs each question through `claude -p` with and without the plugin, N times, records tokens, tool calls, wall-clock; `scripts/eval/grade.sh` scores both answers against the reference with Sonnet through the Messages API (rubric: correctness, completeness, 0–3 each); `docs/benchmarks.md` gets the table with the parity gate applied (savings reported only where the with-index score ≥ baseline; failures listed). |

## Tasks

- [x] Plugin live check with `claude -p --plugin-dir` on a scratch copy of `docs/`; recorded in `design/mcp.md`; `mda index <dir>` fixed on the way.
- [x] `store::usage_by_day` + test.
- [x] `mda cost` + CLI test.
- [x] `mda diagnostics` + CLI test (redaction asserted).
- [x] `mda nudge` + `scripts/nudge.sh` config check + CLI tests.
- [x] `mda start` example query + `--no-example`; `bootstrap.sh` passes `--no-example`; CLI test (skip reason asserted) and an engine-level test of the picker.
- [x] Skill and README updated for the new commands; CHANGELOG.
- [ ] Codex review of the first-run PR, triaged; merge.
- [x] ADR-0005 release pipeline; `release.yml`; `check-version.sh` in CI; `bump-version.sh`; `bootstrap.sh` Windows archive; `bootstrap.sh` installs a locally mirrored `mda-darwin-arm64.tar.gz` + `SHA256SUMS` on this machine (`mda --version` matches). Dry run of the workflow itself: see the PR (`workflow_dispatch` on the branch).
- [x] A/B protocol: `evals/ab/questions.jsonl` (12, with references), `scripts/eval/ab.sh`, `scripts/eval/grade.sh`, first numbers on the golden corpus in `docs/benchmarks.md` and `evals/ab/results/`: 12/12 parity, index reads *more* source tokens on the tiny corpus (recorded, not hidden).
- [ ] Docs: STATUS, aha, index, `design/commands.md` (new: the command surface as built), CHANGELOG.

## Exit criteria

- [x] A headless Claude Code session with the plugin loaded answers a question through `mda_search` (transcript excerpt in `design/mcp.md`).
- [x] On a fresh copy of `docs/` with the `api` backend, `mda start` prints a real hit with a line range within 60 s of starting: 5.4 s, after the first 10 cards.
- [x] `mda cost`, `mda diagnostics`, `mda nudge` exist, have `--json`, and are covered by CLI tests; `diagnostics` output contains neither the home directory nor the workspace id.
- [ ] A tag build produces the five archives and `SHA256SUMS` (pending the first `workflow_dispatch` run); `scripts/bootstrap.sh` installed the darwin-arm64 archive on this machine from a local mirror of the release layout and `mda --version` matched `VERSION` (done 2026-09-22).
- [x] `check-version.sh` runs in CI and fails on a mismatch (verified locally with a wrong tag; the CI job runs the same script).
- [x] The A/B table exists in `docs/benchmarks.md` with the parity gate applied (golden corpus: parity 12/12, no saving). The README makes no token-saving number claim.

## Decisions taken in this phase

- The binary still never reads `CLAUDE_PLUGIN_DATA`; the launcher passes `MDA_NUDGE_FILE` like it passes `MDA_MODEL_DIR`.
- The example query in `start` is bounded (60 s) and skipped by the SessionStart hook; it never blocks a session.
- A read ledger (tokens saved on `mda open`, the "index hit rate" of plan §6) is a schema change and an owner decision; deferred to follow-ups rather than slipped into this phase.

## Follow-ups

- Read ledger (schema v4: `opens_log`) so `cost` and `status` can show tokens saved and the index hit rate.
- Homebrew tap and `cargo install mda-cli` once the first release is out.
- Submission to `claude-plugins-community` after design partners.
