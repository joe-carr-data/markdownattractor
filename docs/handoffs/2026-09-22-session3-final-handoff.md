# Handoff — 2026-09-22, end of session 3 (Phase 4 shipped: v0.1.1 released, repo public, benchmark plan accepted)

Written by Claude (Fable 5.1) at the end of the third build session, for the next session after compaction. **This is the authoritative handoff. Read this whole file first, then every file in §3 in the order given; nothing here replaces reading the code and the docs it points to.** Everything below was verified in-session unless marked otherwise. Earlier handoffs stay accurate for the internals they describe: `2026-09-22-session-handoff.md` (Phases 0–1 engine), `2026-09-22-phase2-handoff.md` (daemon, search layer, MCP), `2026-09-22-phase4-handoff.md` (first-run UX, release pipeline, A/B protocol, with addendum). The `*-auto.md` files are hook-written snapshots and can be ignored.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF

## 0. One-paragraph state

markdownattractor is **released and public**: v0.1.0 then v0.1.1 are on GitHub Releases (five prebuilt archives + `SHA256SUMS` each), the repository is public (history rewritten first to scrub two workspace ids), and the whole user path was verified from a clean machine state: `claude plugin marketplace add joe-carr-data/markdownattractor` → `claude plugin install markdownattractor@markdownattractor` → the SessionStart hook downloads the binary from the Release, verifies the checksum, starts the daemon → a headless session answers through `mda_search`. Phases 0–4 are done (plans all ticked; the Phase 4 plan is marked done). The next phase is the **benchmark plan** (`docs/plans/2026-09-benchmarks.md`, v3.1), which went through three Codex passes and is accepted; its first two implementation tasks are `.mdx` ingestion and a leaner MCP hit payload, neither started. 272 tests, `make check` green, CI green on ubuntu/macOS/Windows on every merged PR. GitHub Actions minutes: the private-repo pool was exhausted mid-session (2,002/2,000), which is why the repo went public; the owner has no payment method on GitHub, so overage is blocked anyway and public-repo minutes are free.

## 1. Repo, branches, PRs, releases, threads

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor, **public since 2026-09-22 ~15:35 UTC**. Owner Joe Carr. |
| `main` | `475c30c` (merge of PR #14) at the time of writing, plus PR #15 and this handoff's PR when they merge. Tree clean. |
| Open PRs | **#15** `plan/benchmarks-owner-decisions` (plan v3.1: model panel instead of a human gate, no no-retrieval control) — merges itself on a green job list (a background watcher was running); **this handoff's PR** (`docs/session3-final-handoff`). Check `gh pr list` first thing. |
| Merged today | #7 first-run UX (Codex-reviewed), #8 release pipeline (ADR-0005), #9 A/B parity protocol, #10 wrap-up + two release fixes, #11 docs, #12 `plugin.json` fix + v0.1.1, #13 docs, #14 benchmark plan (v3 + review). Merged branches are left on origin. |
| Releases | `v0.1.0` (tag `b307d8b…`): first tag; **its marketplace install fails to load** (manifest bug). `v0.1.1`: fixed; **this is the one users get**. Assets: `mda-{darwin-arm64,darwin-x64,linux-arm64,linux-x64,windows-x64}.tar.gz` + `SHA256SUMS` (12.7 / 5.5 / 15.2 / 15.1 / 13.1 MB). |
| Versions | `VERSION` = `plugin.json` = `marketplace.json` = `Cargo.toml` = **0.1.1** (`scripts/dev/check-version.sh` runs in CI). Next release: `scripts/dev/bump-version.sh 0.1.2`, commit `chore(release): v0.1.2`, `git tag v0.1.2 && git push origin v0.1.2`. |
| History rewrite | Done with `git filter-repo --replace-text` on 2026-09-22 ~15:30 UTC (replacing two `wrkspc_…` ids with `wrkspc_<redacted>`; no keys or tokens were ever committed). **Every commit SHA quoted in docs written before that time refers to the old history** (review pins `b0814de`, `f7c1d77`, `a8d17ed`, `72a72ca`; handoff refs). Look commits up by message. Old objects may linger on GitHub until GC; the two Dependabot branches on the old history were deleted by the owner. |
| Plugin on this machine | Installed at **user scope from the GitHub marketplace** (`~/.claude/plugins/cache/markdownattractor/markdownattractor/0.1.1/`); data dir `~/.claude/plugins/data/markdownattractor-markdownattractor/bin/mda` (0.1.1, downloaded from the Release). The old `--plugin-dir` data dir (`markdownattractor-inline`) was removed. The marketplace source is the GitHub repo, so `claude plugin update` tracks `main`'s `plugin.json` version. |
| Codex threads (all `gpt-6-astra`, CLI 0.155.1, via the codex-rescue subagent + the shared companion runtime) | Pre-mortem `01a0c558-3d54-7fb2-920a-cdc40379dcf6`; crates `01a0c5bd-d175-7150-9385-4e49b1598d2e`; daemon `01a0c77b-6ce4-7712-b2bd-c8baf7221a12`; Phase 2 `01a0c7cc-43c4-7452-b6c4-69f819526fd3`; **first-run UX `01a0c83e-04a5-7f21-b560-cb6ff62ad473`** (8 findings); **benchmark plan `01a0ca49-d99d-7282-8417-91b5c14ecaaf`** (three passes: 13 findings → 8 resolved/5 partly → all resolved, accepted). |
| Secrets | `~/.config/markdownattractor/env` holds `ANTHROPIC_API_KEY` and `ANTHROPIC_WORKSPACE_ID`; load with `set -a; source ~/.config/markdownattractor/env; set +a`; never print or commit; the owner declined rotation. The permission classifier blocks reading `.env`-style files; ask rather than search. |
| Embedding model | `~/.cache/markdownattractor/models` (dev; `export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models`); the plugin uses `${CLAUDE_PLUGIN_DATA}/models` (downloaded on the first embed pass). |
| Spend this session | ≈ $2.60 API (docs copy carded twice ≈ $1.9, golden corpus $0.31, grading cents); ≈ $4 Claude Code usage (headless plugin checks, A/B runs). |
| Scratch | Session scratchpad has `live-plugin`, `live-start`, `ab-corpus` (golden copy with cards+vectors, indexed), `estimate` (the owner's trading repo markdown, parsed only), `rewrite` (the filter-repo clone), `ab-full`/`ab-out` (A/B logs). All disposable. No `mda daemon` processes were left running. |

## 2. What this session did, in order (with the files)

1. **Plugin watched live** in a headless session (`claude -p --plugin-dir …`): MCP server `plugin:markdownattractor:markdownattractor` connects; tools `mcp__plugin_markdownattractor_markdownattractor__mda_*`. Recorded in `docs/design/mcp.md` ("Watched live"). Bug found on the way: `mda index .` treated `.` as a file → fixed (a directory is the root, or the enclosing indexed root).
2. **First-run UX** (`docs/design/commands.md`): `mda start` waits ≤ 60 s for the first usable card and prints an example query + hit (`Engine::example`, `Store::carded_sections_sample`; `--no-example`, hidden `--example-timeout ≤ 3600`); `mda cost [--since]` (`Store::usage_by_day`); `mda diagnostics [--out]` (allowlisted config, one `Scrub` for every free string, bounded symlink-safe log tail, `create_new` output); `mda nudge [on|off] [--global]` (`config::nudge_off_file`, `set_global_nudge[_at]`, launcher passes `MDA_NUDGE_FILE`, hook honours it and `nudge = false` in `config.toml`). Live: example after 5.4 s on a fresh `docs/` copy. Codex review `docs/reviews/codex/2026-09-22-first-run.md` (8/8 addressed).
3. **Release pipeline** (ADR-0005, `docs/design/distribution.md`): `.github/workflows/release.yml` (tags `v*` + `workflow_dispatch`; five native builds; `SHA256SUMS`; the real `bootstrap.sh` installs linux-x64 from a local mirror as the gate; `gh release create` on tags; conditional notarisation via `scripts/dev/notarize-macos.sh` when six `APPLE_*` secrets exist, which they do not). Two fixes from the first dry run: `rustup target add` after the toolchain step (`rust-toolchain.toml` pins 1.98.1), Linux builds on `ubuntu-24.04[-arm]` (the static ONNX Runtime needs GCC 13's libstdc++ and glibc 2.38+). `scripts/dev/{check-version,bump-version}.sh`; `version agrees` job in `ci.yml`. `bootstrap.sh` handles `mda.exe`.
4. **A/B parity protocol** (`evals/README.md` A/B section, `evals/ab/questions.jsonl` 12 questions with references, `scripts/eval/ab.sh`, `scripts/eval/grade.sh`, `evals/ab/results/2026-09-22-golden.md`, `docs/benchmarks.md` A/B section): parity 12/12 on the golden corpus, **index reads more source tokens** (median 1,305 vs 341) because eight hits with cards ≈ 1.3K tokens whatever the corpus. README makes no token-saving claim.
5. **Repo public + history rewrite** (§1). Memory note `repo-secrets-hygiene`.
6. **v0.1.0 → marketplace install failed to load** ("Duplicate hooks file detected": `plugin.json` must not list `hooks`, `skills`, `mcpServers` when they sit at the standard paths; `claude plugin validate` and `--plugin-dir` did not catch it) → **v0.1.1** (PR #12). Verified from a clean state (§0).
7. **Benchmark plan** `docs/plans/2026-09-benchmarks.md` v3.1 + `docs/reviews/codex/2026-09-22-benchmark-plan.md`: see §4.
8. Owner Q&A recorded in docs where relevant: five builds and their sizes (`design/distribution.md`), cost/time to index the owner's trading repo (582 files after `.gitignore`, 7,380 sections, ≈ 2.0M tokens: raw index 2.3 s; cards ≈ 80 min and ≈ $27 on Haiku, or 8–12 h at $0 on gpt-oss-20b; a smaller local model is **not** faster because gpt-oss-20b is MoE with ~3.6B active params; the guide `docs/guides/local-model.md` has the table).

## 3. Files to re-read, in order (T0 → T2)

1. `CLAUDE.md`; `.claude/rules/{rust,workflow,docs}.md` (rust rules carry all four Codex reviews' lessons); `docs/STATUS.md`; `docs/aha.md` (the top ~12 lines are this session).
2. `docs/plans/2026-09-benchmarks.md` (**the active plan**, v3.1: rules §0, owner decisions §0a, axes §1, datasets §2, competitors §3, feature order §4, tasks §5, exit criteria §6) and `docs/reviews/codex/2026-09-22-benchmark-plan.md` (three passes, verbatim verdicts, scope caveat).
3. `docs/plans/2026-09-phase4-launch.md` (done; what shipped and why), `docs/adr/0005-release-pipeline.md`, `docs/design/{commands,distribution,mcp}.md`, `docs/benchmarks.md`, `docs/reviews/codex/2026-09-22-first-run.md`.
4. `evals/README.md`, `evals/ab/questions.jsonl`, `scripts/eval/{ab,grade}.sh` (the harness the benchmark plan's B0 hardens), `evals/golden/*`, `crates/mda-cli/src/commands/eval.rs`.
5. Code touched this session: `crates/mda-cli/src/commands/{start,cost,diagnostics,nudge,index,doctor,stop}.rs`, `crates/mda-core/src/{config,pipeline}.rs`, `crates/mda-core/src/store/mod.rs` (`DailyUsage`, `usage_by_day`, `carded_sections_sample`), tests in `crates/mda-cli/tests/{engine_cli,daemon_cli}.rs`. For the next tasks: `crates/mda-core/src/walk.rs` (`is_markdown`, the `.mdx` gap), `crates/mda-core/src/markdown/mod.rs` (parser), `crates/mda-core/src/mcp.rs` (hit payload), `crates/mda-core/src/search.rs` (`Hit`).
6. Plugin surface: `.claude-plugin/plugin.json` (no component keys!), `.claude-plugin/marketplace.json`, `.mcp.json`, `hooks/hooks.json`, `scripts/{bootstrap.sh,mda,nudge.sh}`, `skills/*/SKILL.md`, `.github/workflows/{ci,release}.yml`, `scripts/dev/*.sh`.
7. Earlier handoffs for internals (§0 above); `docs/project-plan.md` §2 (landscape), §11 (evals), §9 (distribution) only as reference.

## 4. The benchmark plan, condensed (so the next session starts right)

- **Primary dataset: DocsQA-Repo** (`github.com/PowderXu/docsqa-data`, schema v3): 467 real community questions, 4,860 pages, four repos pinned to commits (GitHub Docs `github/docs@c34e3dc` `content/` 197 q; Prisma `prisma/web@c4ac0e9` `apps/docs/content/docs` 125 q; Tailwind `tailwindlabs/tailwindcss.com@bd868a3` `src/docs` 93 q; Supabase `supabase/supabase@6ea3567` `apps/docs/content` 52 q), `answers.jsonl` (`qrel_ids`, `qrel_anchors`, 601 judgments), `aspects.jsonl` (model-assisted), `corpus.jsonl.gz` (rendered text), `manifest.json`, `SCHEMA.md`, verification script in the sibling repo `PowderXu/Research-on-DSH`. Secondary FreshStack (docs subset only, labelled derived). Temporal: git-derived on the DocsQA repos with historical replay (mtimes from commit author dates + `MDA_NOW` observation-clock override, validated against `git log`) and a git-log baseline. TEMPO rejected (document time stamps, not versioned files).
- **Arms:** grep baseline (Read/Grep/Glob), mda, qmd (`tobi/qmd`, `npm i -g @tobilu/qmd`, full config + reranker-off ablation), graphify (`safishamsi/graphify`), git baseline for temporal. No no-retrieval control (owner decision).
- **Gates before any savings claim (rule 0.4):** paired-bootstrap CI lower bound of (index − baseline) mean ≥ −0.25 on 0–6; index mean ≥ 4.0; grounding pass rate ≥ 95%. Failures count as results. Dev/test/sealed-holdout split (30/55/15, seed 20260922). Tokens via `count_tokens`. Panel (Fable + Astra) replaces the human for calibration (30 answers/dataset) and metadata audit (100 cards/corpus); publish agreement.
- **Order of work:** B0a `.mdx` ingestion → B0b leaner hit payload (MCP `k` 5, no `snippet` with a card, compact fields) → B0 harness hardening (dataset adapters `mda eval --dataset docsqa`, page-level aggregation, per-arm manifests + smoke tests, fail-loud, counted tokens, conventional median, `FROZEN.md`, split tool, `panel.sh`) → B1 own corpora → B2 DocsQA ingestion gate (≥ 95% qrel coverage) + axes A/B → B3 freshness → B4 temporal → B5 FreshStack → B6 page + README row. Budget: ≤ $60 API for cards, ≤ $150 Claude Code usage; ask before exceeding.
- **Scope caveat to carry on the page (Codex, verbatim in the review):** results cover declared dataset adaptations, a simulated historical replay and a disclosed test reuse; they do not establish universal superiority or production recovery of historical timestamps.

## 5. Recipes that worked

```bash
# gate and tests
source "$HOME/.cargo/env"; make check                       # fmt, clippy pedantic -D warnings, nextest (272), deny, rustdoc
cargo nextest run -p mda-cli -E 'test(diagnostics) | test(start_status)'
cargo build --release -p mda-cli                            # ~8 min; the SessionStart hook copies target/release/mda (path 3) if present — keep it current or delete it before testing installs

# headless Claude Code with the plugin (strip nested-session env first)
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done
claude -p "<prompt>" --plugin-dir /Users/jcarr/markdownattractor --allowedTools "mcp__plugin_markdownattractor_markdownattractor__*" --max-turns 8 --output-format stream-json --verbose
claude -p "<prompt>" --allowedTools "mcp__plugin_markdownattractor_markdownattractor__*" ...   # installed plugin, no --plugin-dir
# NOT --bare (it skips keychain reads → "Not logged in"); put `--` before the prompt; --tools a b c are separate args

# marketplace install test from a clean state
claude plugin uninstall markdownattractor@markdownattractor; claude plugin marketplace remove markdownattractor
rm -rf ~/.claude/plugins/data/markdownattractor-markdownattractor
claude plugin marketplace add joe-carr-data/markdownattractor && claude plugin install markdownattractor@markdownattractor --scope user && claude plugin list
# a local checkout works too and catches manifest bugs before tagging: claude plugin marketplace add /Users/jcarr/markdownattractor

# A/B
set -a; source ~/.config/markdownattractor/env; set +a; export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models
mda index <corpus>; scripts/eval/ab.sh <corpus> evals/ab/questions.jsonl <out> 1 sonnet; scripts/eval/grade.sh evals/ab/questions.jsonl <out>

# release
scripts/dev/check-version.sh [vX.Y.Z]; scripts/dev/bump-version.sh X.Y.Z
gh workflow run release.yml --ref main          # dry run (needs the workflow on the default branch); tags publish
gh run view <id> --json jobs --jq '.jobs[] | "\(.name): \(.conclusion)"'     # ALWAYS the job list, never the checks column
gh api repos/joe-carr-data/markdownattractor/actions/jobs/<job>/logs | sed -E 's/\x1b\[[0-9;]*m//g' | grep -nE "error|##\[error"

# Codex review: Agent(subagent_type="codex:codex-rescue") with "--fresh --model gpt-6-astra", packet pinned to a SHA, reviewer prompt verbatim; tell it to use the shared companion runtime, NOT bare `codex exec --sandbox read-only --ephemeral` (fails to initialise here). SendMessage to the same agent reuses the thread for follow-up passes.

# history / secrets check before any push that touches docs about secrets
git log --all -p | grep -E "sk-ant-|wrkspc_01"

# waiting on background work: use run_in_background + a completion notification, or an `until … sleep` loop; bare `sleep N; cmd` is blocked
```

Patching files from Bash: `cargo fmt` first, then per-block Python patches that assert exactly one anchor match and report failures (the pattern used all session).

## 6. Caveats and gotchas (this session's, deduplicated; earlier handoffs hold the rest)

- `plugin.json` must not name `hooks`, `skills` or `mcpServers` when they live at the standard paths; only a real marketplace install (GitHub or local path) catches it.
- A hook calling the installed binary with a flag it lacks fails silently (`>/dev/null 2>&1 &`): ship script + binary together; re-copy or delete `target/release/mda` before testing hooks (the hook prefers a local build over the download).
- `rust-toolchain.toml` pins the CI toolchain; cross targets need `rustup target add` after the toolchain action. ONNX Runtime prebuilt static libs need GCC 13 libstdc++ / glibc 2.38+ → Linux release runners are 24.04 and users need glibc ≥ 2.39.
- `gh workflow run` needs the workflow on the default branch. GitHub's budgets REST API is org-only; a personal account sets budgets in the web UI.
- Actions minutes: Windows ×2, macOS ×10; the Windows test job is 6–24 min. Public repo = free standard-runner minutes.
- macOS temp paths have two spellings; Windows `home_dir()` reads `USERPROFILE`; the diagnostics scrubber handles both and tests set both env vars.
- `mda status`'s daemon line counts cards per finished round; the store line is live; they differ mid-round.
- A/B: source tokens are what the tools return; eight hits with cards ≈ 1.3K tokens; a 5× saving needs ≈ 6.5K baseline tokens per answer at that floor → shrink the payload before measuring.
- History was rewritten: old SHAs in docs are stale; GitHub may still serve old objects by SHA until GC.
- The walker rejects `.mdx`; three of four DocsQA corpora are MDX (the benchmark cannot start without B0a).
- Our clocks are filesystem clocks (`file_times`: mtime, birthtime-or-first-seen, `now`); any historical replay must set mtimes and pin `now` (`MDA_NOW`, to be built) and validate against git.

## 7. Next steps, in order

1. **Check the open PRs** (#15 plan v3.1, this handoff). Merge on green job lists if the background watchers did not.
2. **B0a `.mdx` ingestion** (branch, plan line in `2026-09-benchmarks.md` §5): accept `.mdx` in `walk::is_markdown`; make the parser tolerate JSX blocks, `import`/`export` lines and MDX components without losing headings (treat them as text or drop them deterministically, decide and document in `design/search.md` or a new `design/ingestion.md`); tests with Tailwind/Prisma-style samples; `mda parse` on real files from the pinned commits as the acceptance check. Codex review (it is `crates/`).
3. **B0b leaner hit payload**: MCP `mda_search` default `k` 5, omit `snippet` when `tldr` exists, consider dropping `score`/`vector_score` from the MCP view; measure with `scripts/eval/ab.sh` on the golden corpus before/after (source tokens per answer). Keep the CLI `--json` shape stable or version it.
4. **B0 harness hardening** per plan §5, then **B2 DocsQA ingestion gate** (clone the four repos at the pinned commits, index, map `qrel_ids` ↔ paths, publish coverage) and the first axis-A table.
5. Launch checklist in parallel when convenient: README leads with the one-line install and drops "Not released yet"; status table updated; demo; community marketplace submission; design partners.
6. Owner decisions still open: Apple notarisation secrets; read ledger (schema v4). Decided today: no human gates (Fable + Astra panel), no no-retrieval control, no mention-scrubbing beyond secrets (attribution lines stay).
7. Carried follow-ups: subtree-only `index`, live config reload, socket peer auth, hybrid latency lever, a `nudge.sh` shell test, populated `cost` CLI test.
