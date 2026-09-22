# Handoff — 2026-09-22 session 3 (Phase 4: plugin watched live, first-run UX, release pipeline, A/B parity)

Written by Claude (Fable 5.1) at the end of the third build session, for the next session after compaction. **Read this whole file first, then the reading list in §3; nothing here replaces the code and the docs it points to.** Everything below was verified in-session unless marked otherwise. The previous handoff (`2026-09-22-phase2-handoff.md`) stays accurate for Phases 0–2 internals; this one supersedes it for repo state and next steps.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF

## 0. One-paragraph state

Phase 4 (launch) is most of the way through. Merged on `main` today: **PR #7** first-run UX (`mda start` example query, `mda cost`, `mda diagnostics`, `mda nudge`, `mda index <dir>`, plugin watched live in a headless Claude Code session, Codex review 8/8 triaged and fixed), **PR #8** release pipeline (ADR-0005: `release.yml`, `check-version.sh` in CI, `bump-version.sh`, conditional notarisation, `bootstrap.sh` install gate), and **PR #9** A/B parity protocol (questions, runner, grader, first honest numbers: parity 12/12, no token saving on the tiny golden corpus). The first `workflow_dispatch` dry run of the release workflow failed on the Intel macOS build (`rust-toolchain.toml` pins 1.98.1, the cross target was added to `stable` only); the fix is on branch `docs/phase4-wrap` together with this handoff. 272 tests, `make check` green, CI green on ubuntu/macOS/Windows for every merged PR. Session API spend ≈ $1.60 (docs copy carded twice, golden corpus once, grading); Claude Code usage for the headless checks and the A/B run ≈ $2.

## 1. Repo, branches, PRs, threads

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor, **private**. Owner Joe Carr. |
| `main` | after #9 (see `git log`); PR merges: #7 `feat/phase4-first-run`, #8 `feat/release-pipeline`, #9 `feat/ab-parity`. Merged branches are left on origin (harmless). |
| Open branch / PR | `docs/phase4-wrap` = **PR #10**: the two release workflow fixes, aha lines, this handoff, STATUS. **Blocked on GitHub Actions billing**: at ~09:01 UTC every job of its CI run failed in 4 s with "The job was not started because recent account payments have failed or your spending limit needs to be increased" (the private repo's Actions minutes; the release dry run's macOS/Windows/arm64 builds bill at multipliers). Owner fixes Billing & plans → then `gh run rerun <run-id>` (or push), merge on a green job list, then `gh workflow run release.yml --ref main`. |
| Release workflow dry run | run `35707331315`: `version agrees` ok; `build (darwin-arm64)` **succeeded** (build, smoke test, pack); `build (darwin-x64)` failed (`can't find crate for core`: the cross target was added to `stable`, not to the toolchain `rust-toolchain.toml` pins); `build (linux-x64)` and `build (linux-arm64)` failed at link time (`__cxa_call_terminate`, `__isoc23_strtoll`, `basic_string::_M_replace_cold` undefined: the prebuilt static ONNX Runtime wants GCC 13's libstdc++ and glibc 2.38+, 22.04 has neither); `build (windows-x64)` was still running at handoff. Both fixes are on `docs/phase4-wrap` (PR #10): `rustup target add` after the toolchain step, Linux builds on `ubuntu-24.04[-arm]`. `publish` never ran. |
| Codex threads | Phase 4 first-run review `01a0c83e-04a5-7f21-b560-cb6ff62ad473` (pinned `72a72ca`, model `gpt-6-astra`, CLI 0.155.1) via the shared companion runtime; the bare `codex exec --sandbox read-only --ephemeral` attempt failed to initialise. Earlier threads in the Phase 2 handoff. |
| Secrets | `~/.config/markdownattractor/env` (`ANTHROPIC_API_KEY`, `ANTHROPIC_WORKSPACE_ID`); load with `set -a; source ~/.config/markdownattractor/env; set +a`; never print or commit; **rotation declined by the owner**. |
| Machine state | Embedding model in `~/.cache/markdownattractor/models` (dev) and, since the hand-started daemon's embed pass, also in `~/.claude/plugins/data/markdownattractor-inline/models` (the `--plugin-dir` identity's data dir; a marketplace install uses `markdownattractor-markdownattractor/`). No `mda daemon` processes left running. Scratch roots under the session scratchpad (`live-plugin`, `live-start`, `ab-corpus`) can be deleted. |

## 2. What was built this session (where to look)

- **Plugin live check** (`docs/design/mcp.md`, "Watched live"): `claude -p --plugin-dir <repo> --allowedTools "mcp__plugin_markdownattractor_markdownattractor__*"` connects `plugin:markdownattractor:markdownattractor`, lists the seven tools, and with cards Claude searched, opened seven lines of `docs/design/daemon.md` and answered a rename question correctly in four turns. `--plugin-dir` plugins are `<name>@inline` with data dir `markdownattractor-inline`.
- **First-run UX** (`docs/design/commands.md`): `start` waits ≤ 60 s for the first usable card (`Engine::example`, `Store::carded_sections_sample`), `--no-example` (hook, `restart`), `--example-timeout` (hidden, ≤ 3600). Live: 5.4 s on a fresh `docs/` copy. `cost` from `Store::usage_by_day`; `diagnostics` = allowlisted config + one scrubber (`Scrub`) + bounded symlink-safe log tail + `create_new` output; `nudge [on|off] [--global]` with `MDA_NUDGE_FILE` passed by the launcher and honoured by `scripts/nudge.sh`; `index <dir>` = that root, or the enclosing indexed root.
- **Release pipeline** (ADR-0005, `docs/design/distribution.md`): `.github/workflows/release.yml` (tags `v*` + `workflow_dispatch`), five archives named for `bootstrap.sh`, `SHA256SUMS`, the real `bootstrap.sh` installs linux-x64 from a local mirror as the gate, `gh release create` on tags; `scripts/dev/{check-version,bump-version,notarize-macos}.sh`; `version agrees` job in `ci.yml`. Verified locally: bump round-trip, wrong-tag rejection, mirror install of `mda-darwin-arm64.tar.gz`.
- **A/B parity** (`evals/README.md`, `docs/benchmarks.md`, `evals/ab/results/2026-09-22-golden.md`): `scripts/eval/ab.sh` (headless `claude -p`, `--setting-sources "" --strict-mcp-config`, baseline Read/Grep/Glob vs index + MCP + search-first rules, records source tokens = size of tool results), `scripts/eval/grade.sh` (Sonnet via the Messages API, 0–6, parity gate, `parity.md`). Result on the 450-line golden corpus: parity 12/12; median source tokens 341 baseline vs 1,305 index; total input tokens 38K vs 33K; the index is slower to pay off than the plan hoped on small corpora and the page says so.

## 3. Files to re-read, in order

1. `CLAUDE.md`, `.claude/rules/{rust,workflow,docs}.md` (rust rules now carry the first-run review's three lines), `docs/STATUS.md`, `docs/aha.md` (top 8 lines are this session).
2. `docs/plans/2026-09-phase4-launch.md` (contracts, ticks, exit criteria; one still open: the tag build).
3. `docs/adr/0005-release-pipeline.md`, `docs/design/{commands,distribution,mcp}.md`, `docs/benchmarks.md` (A/B section), `docs/reviews/codex/2026-09-22-first-run.md`.
4. Code touched: `crates/mda-cli/src/commands/{start,cost,diagnostics,nudge,index,doctor}.rs`, `crates/mda-core/src/{config,pipeline}.rs` (`nudge_off_file`, `set_global_nudge[_at]`, `Example`, `Engine::example`), `crates/mda-core/src/store/mod.rs` (`DailyUsage`, `usage_by_day`, `carded_sections_sample`); tests in `tests/{engine_cli,daemon_cli}.rs`.
5. Plugin surface: `scripts/{bootstrap.sh,mda,nudge.sh}`, `skills/mda/SKILL.md`, `.github/workflows/{ci,release}.yml`, `scripts/dev/*.sh`, `scripts/eval/*.sh`, `evals/ab/`.

## 4. Recipes that worked

```bash
# headless plugin check (scratchpad/plugin-check.sh pattern): strip nested-session env, then
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done
claude -p "<prompt>" --plugin-dir /Users/jcarr/markdownattractor --allowedTools "mcp__plugin_markdownattractor_markdownattractor__*" --max-turns 8 --output-format stream-json --verbose
# first-run live check on a fresh copy of docs/ (spends ~$1 on Haiku for the full backfill)
set -a; source ~/.config/markdownattractor/env; set +a; export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models
cp -R docs "$S/live-start/docs"; target/debug/mda start "$S/live-start"    # prints the example after the first cards
# A/B
target/release/mda index <corpus>; scripts/eval/ab.sh <corpus> evals/ab/questions.jsonl <out> 1 sonnet; scripts/eval/grade.sh evals/ab/questions.jsonl <out>
# release
scripts/dev/check-version.sh [vX.Y.Z]; scripts/dev/bump-version.sh X.Y.Z; gh workflow run release.yml --ref main; gh run list --workflow=release.yml
# CI: read the job list
gh run view <id> --json jobs --jq '.jobs[] | "\(.name): \(.conclusion)"'
```

## 5. Caveats learned this session

- A hook calling the binary with a flag the *installed* binary lacks fails silently; ship script and binary together and re-copy the binary before testing a hook.
- `claude -p --bare` disables keychain reads ("Not logged in"); variadic flags swallow the prompt (`--` before it); `--tools a b c` are separate args.
- `gh workflow run` needs the workflow on the default branch.
- `rust-toolchain.toml` pins the toolchain cargo uses in CI; `dtolnay/rust-toolchain@stable` with `targets:` adds the target to `stable`, not to the pinned one (fixed on `docs/phase4-wrap`).
- The prebuilt static ONNX Runtime wants GCC 13's libstdc++: linking on `ubuntu-22.04-arm` fails with `__cxa_call_terminate` undefined. Linux release builds run on 24.04 (glibc ≥ 2.39 for users).
- macOS temp paths have two spellings; Windows `home_dir()` reads `USERPROFILE`; the scrubber handles both, tests set both env vars.
- `mda status`'s daemon line counts cards per finished round; the store line is live. They disagree mid-round by design.
- A/B on tiny corpora: eight hits with cards are ~1.3K tokens regardless of size; `k` and the per-hit payload are the lever if G3 is to be met on mid-size corpora.

## 6. Next steps, in order

1. **Owner**: fix GitHub Actions billing / spending limit. Then re-run PR #10's CI, merge on a green job list, `gh workflow run release.yml --ref main`, read the job list. Still unverified there: the Intel cross build (`--no-default-features`; `aws-lc-sys` cross-compiles with Apple's toolchain in theory), Windows packing (`tar` on the Windows runner, `mda.exe`), and the `publish` job's `bootstrap.sh` gate. Fix what it shows.
2. Once the dry run is green: `git tag v0.1.0 && git push origin v0.1.0` (VERSION is already 0.1.0) and confirm the Release carries five archives + `SHA256SUMS`; then install the plugin from the marketplace on this machine and confirm `bootstrap.sh` fetches the binary (no local build present in `${CLAUDE_PLUGIN_DATA}`).
3. Owner decisions to raise: repo visibility (public), Apple Developer secrets for notarisation, the read ledger (schema v4) for `cost`/`status` savings and index hit rate.
4. A/B on a realistic corpus (this repo's `docs/`, 5K lines, or a design partner's) and a leaner hit payload (`k`, fields) before any token-saving claim.
5. Follow-ups carried: subtree-only `index`, a shell test for `nudge.sh`, populated `cost` CLI test, socket peer auth, live config reload, time-filter eval cases, hybrid latency lever.
