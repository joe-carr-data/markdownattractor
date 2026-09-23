# Handoff — 2026-09-23, end of session 4 (benchmark B0a/B0b, DocsQA adapter and gate, all four corpora carded, execution plan v3.1 + runbook)

Written by Claude (Fable 5.1) at the end of the fourth build session, for the next session after compaction. **This is the authoritative handoff. Read this whole file first, then every file in §3 in the order given; nothing here replaces reading the code and the docs it points to.** It supersedes `2026-09-22-session4-handoff.md` (an earlier snapshot of the same session, kept for its §5 caveats) and, for repo state and next steps, `2026-09-22-session3-final-handoff.md` (still the reference for releases, the history rewrite, the plugin install path and its recipes). Everything below was verified in-session unless marked otherwise.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF

## 0. One-paragraph state

The benchmark effort has its strategy plan (`docs/plans/2026-09-benchmarks.md` v3.2), an execution plan that went through three Codex passes (`docs/plans/2026-09-benchmark-execution.md` v3.1, verdict "not yet without qualifications", the qualifications being exactly milestones M1–M5), and a reproducibility runbook (`evals/benchmark_it_with_claude.md`, with `/benchmark` as a dev skill). Merged today: PR #17 (every benchmark call through the owner's Claude Code login), PR #18 `.mdx` ingestion, PR #19 lean MCP payload + eval scripts through `claude -p`, PR #20 DocsQA adapter + coverage gate + seeded split + a CI change (Windows runs `cargo test`). **PR #21** (execution plan v3.1, runbook, skill, review file) was open with CI running at the time of writing; a detached watcher merges it on eleven green jobs (check `gh pr view 21`). All four DocsQA corpora are **fully carded and embedded** through the owner's login (36,899 cards, 0 failures, 7 h 17 min, list-price equivalent $176 which the owner ruled irrelevant on the Max plan). The ingestion gate passed at 100% label coverage; the exploratory raw-lexical axis-A row is on `docs/benchmarks.md`. 297 tests, `make check` green, CI green on the three OSes.

## 1. Repo, branches, PRs, releases, threads

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor, public. Owner Joe Carr. `main` at `41fdbb1` (merge of #20) plus #21 when it merges. Release v0.1.1 unchanged. |
| Open PR | **#21** `plan/benchmark-execution` at `d3ca63a` (docs only: plan v3.1, runbook, skill, review, STATUS, index). Merge on a green job list; the watcher (`~/.cache/markdownattractor/bench/pr20-merge.log` pattern) was not restarted for it: run `gh run view <id> --json jobs`, then `gh pr merge 21 --merge`. |
| Merged today | #17 plan §0a.3; #18 `.mdx` (`docs/reviews/codex/2026-09-22-mdx-ingestion.md`, thread `01a0ca73-bcc9-7770-b083-cd7abebbc986`); #19 lean payload (`…/2026-09-22-lean-payload.md`, thread `01a0ca82-90c3-7530-bf8f-5ffbc2938b1d`); #20 DocsQA adapter (`…/2026-09-22-docsqa-adapter.md`, thread `01a0ca94-9a86-7283-ae27-fbd805660878`). |
| Execution-plan pre-mortem | `docs/reviews/codex/2026-09-23-execution-plan.md`, thread **`01a0ccb1-a678-7041-9dad-1f08485d767f`** (three passes on one thread: 13 findings → v2; second pass 5 resolved / 8 partly / N1 + 9 runbook findings → v3; third pass no new High, verdict quoted → v3.1). Reuse this thread with `SendMessage` to the `codex:codex-rescue` agent for a fourth pass if a plan change is large. |
| Codex how-to | CLI 0.155.1, `gpt-6-astra`, via the `codex:codex-rescue` subagent. **Always write in the prompt: "do NOT spawn a nested `codex exec`; the companion thread IS the reviewer and answers directly."** The first attempt of every new agent tried `codex exec --sandbox read-only --ephemeral` and failed ("Operation not permitted"); resending the instruction fixed it. |
| CI | `.github/workflows/ci.yml`: Windows now runs `cargo test --workspace --all-features` instead of nextest (GitHub's `taiki-e/install-action` could not start bash on any Windows image for hours on 2026-09-22, partner-runner-images#169). Everything else unchanged. Always read the job list (`gh run view <id> --json jobs --jq '.jobs[] | "\(.name): \(.conclusion)"'`), never the checks column. |
| Bench data on this machine | `~/.cache/markdownattractor/bench/`: `docsqa-data` (clone at `19af578`, `data/corpus.jsonl` decompressed), `github-docs`, `prisma`, `supabase`, `tailwindcss` (sparse, blob-filtered clones at the pinned SHAs, marker `.mda-pinned`, recreated by `clone.sh` there). **Each checkout's `.markdownattractor/` store holds every card and vector** (12 / 54 / 72 / 192 MB); `config.toml` there has `backend = "claude-cli"`, `claude_cli_policy_ack = true`. Logs: `card-all.log` (one line per round), `card-<dir>-round<n>.log`, `card-<dir>-cost.json`. Nothing under this directory is in the repo. |
| Owner decisions (new) | (1) Every benchmark model call goes through the owner's Claude Code login on a Max x20 plan: **no dollar budget**, list-price equivalents are reporting only, never a reason to ask before running (memory `claude-code-login-is-free`). (2) The benchmark must be reproducible by anyone in a Claude session → the runbook. Standing from before: no human gates (Fable + Astra panel), no no-retrieval control, secrets never in the repo. |
| Spend | ≈ $1 list-price equivalent of Claude Code usage for A/B runs and grading; $176 equivalent for the cards; $0 API. |

## 2. What this session did, in order (files)

1. **Plan §0a.3** (PR #17): Claude Code login for every benchmark call; rule 0.7 tokens from the transcript's per-turn `usage`.
2. **B0a `.mdx` ingestion** (PR #18; `docs/design/ingestion.md`): `walk` accepts `.mdx`; `markdown::Flavor` (`Markdown`/`Mdx`) chosen by `Flavor::of_path` and used by `parse_file`, `disk_state` and `open_section` alike; leading ESM block excluded (Mdx only, `is_esm_statement` requires a real statement); headings inside CommonMark HTML blocks of type 6/7 recovered with `RawScan` (fences with length rule, comments, `<pre>`/`<script>`/`<style>`/`<textarea>`) and parsed by comrak (`atx_heading`); title fallback front matter `title:` (YAML or TOML, `quoted_string` handles escapes) then `export const title`; snippets skip markup-only lines (`is_markup_only`, `opens_tag`). Acceptance on the four corpora in the design doc (every heading kept; every non-partial page titled; before: 0 of 1,711 MDX pages had a title). Codex found six real bugs (prose starting with "import", `#` inside nested fences, `# C#`, …), all fixed; three lines added to `.claude/rules/rust.md`.
3. **B0b lean payload** (PR #19; `design/mcp.md`): `SearchView`/`HitView` in `mcp.rs`, `DEFAULT_K` 5, no `score`/`vector`/`vector_score`/`title`, snippet only without a card, `pending` only when true, `updated_at` to the second, `partial` flag; CLI `--json` unchanged. `scripts/eval/grade.sh` through `claude -p --json-schema` with the rubric as `--system-prompt` and the submission over stdin inside `<submission>` tags; ungraded never zero; conventional medians; completeness against `manifest.json`. `scripts/eval/ab.sh` resolves paths before `cd`, refuses an existing `runs.jsonl`, writes `manifest.json` (after the loop: the execution plan asks for before + resume, M5), fails loud. Golden numbers regenerated from raw grades: source tokens 1,304.5 → 762.5 at 11/12 parity, more turns (`evals/ab/results/2026-09-22-golden-lean.md`).
4. **DocsQA adapter + gate** (PR #20; `mda_core::eval`, `evals/README.md`): `eval/mod.rs` (`Split`, `split_ids` by `blake3(seed ‖ id)`, `score_pages`, `pages_of`), `eval/docsqa.rs` (`Dataset::load`, `coverage` with the evidence-anchor check and `normalize_heading`, `evaluate` fetching until ten distinct pages with `truncated`, `RunOptions.include_holdout`), CLI `run_docsqa` in `commands/eval.rs` (`--dataset docsqa --data --project --root --split --seed --cards --fetch --out --open-holdout`; `report_dir` refuses `--out` inside the checkout, `write_report` refuses symlinks, `portable()` writes `~`). Results in `evals/results/docsqa/<project>/{coverage,split,results}.json`; `docs/benchmarks.md` DocsQA section (gate passed, anchors 96/96 · 176/176 · 47/48 · 183/222 with the GitHub Docs gap explained as rendered Liquid includes; raw success@5 0.306 / 0.216 / 0.333 / 0.600; the partial-cards finding; the carding rate).
5. **Carding** of all four corpora through `claude-cli` (`~/.cache/markdownattractor/bench/card-all.sh`, detached with `nohup`): tailwindcss 19 min, supabase 89 min, prisma 90 min, github-docs 3 h 59 min; 15–16 parallel Haiku workers (AIMD pool, cap 16); 0 failures.
6. **Execution plan** (PR #21): competitor profiles (qmd 2.8.3, graphify 0.9.66, baselines), §2 protocol (freeze lifecycle, preflight, arms with pinned installs, external-arm scorer, pooled judgments, panel, whole-sample analysis with failures scoring 0, resumable runner, timing boundaries), §3 greedy tuning loop with eight single-change candidates, §4 tables T1–T6, §5 milestones M1–M9, §6 risks, §7 deliverables, §8 tasks, §9 exit criteria. Runbook `evals/benchmark_it_with_claude.md` (regeneration vs independent rerun, asserted checksums, per-attempt directories) and `.claude/skills/benchmark/SKILL.md`.

## 3. Files to re-read, in order (T0 → T2)

1. `CLAUDE.md`; `.claude/rules/{rust,workflow,docs}.md`; `docs/STATUS.md`; `docs/aha.md` (top ~8 lines are this session).
2. `docs/plans/2026-09-benchmark-execution.md` (v3.1, **the working plan**) and `docs/plans/2026-09-benchmarks.md` (v3.2, the rules that bind it); `docs/reviews/codex/2026-09-23-execution-plan.md` (three passes, the verdict, what each milestone must satisfy).
3. `evals/benchmark_it_with_claude.md` and `.claude/skills/benchmark/SKILL.md` (the runbook grows a section per published table; §7 there is empty until M4).
4. `docs/benchmarks.md` (exploratory sections: lean payload, DocsQA), `evals/README.md`, `evals/results/docsqa/*/coverage.json`, `evals/ab/results/2026-09-22-golden-lean.md`.
5. The three code reviews of the day (`docs/reviews/codex/2026-09-22-{mdx-ingestion,lean-payload,docsqa-adapter}.md`) and `docs/design/{ingestion,mcp,search}.md`.
6. Code: `crates/mda-core/src/eval/{mod,docsqa}.rs`, `crates/mda-cli/src/commands/eval.rs` (`run_docsqa`, `attach_cards`, `record_cards` — the card export will mirror `record_cards`), `crates/mda-core/src/markdown/mod.rs` (`Flavor`, `RawScan`, `atx_heading`), `crates/mda-core/src/mcp.rs` (`HitView`), `crates/mda-core/src/search.rs` (`snippet_of`), `scripts/eval/{ab,grade}.sh`, tests `crates/mda-core/src/markdown/tests.rs` (MDX section), `crates/mda-cli/tests/{engine_cli,mcp_cli}.rs` (DocsQA fixture and three tests).
7. Earlier handoffs only for internals: `2026-09-22-session3-final-handoff.md` (§5 recipes, §6 caveats), then Phase 4 / Phase 2 / session 1.

## 4. Recipes that worked

```bash
# gate
source "$HOME/.cargo/env"; make check                        # fmt, clippy pedantic -D warnings, nextest (297), deny, rustdoc
# never run the gate while a background loop executes target/debug/mda: nextest rebuilds the binary

# DocsQA adapter on the carded checkouts (dir names: github-docs prisma supabase tailwindcss; project ids: github-docs prisma supabase tailwind-css)
export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models; B=$HOME/.cache/markdownattractor/bench
target/release/mda eval --dataset docsqa --data $B/docsqa-data --project prisma --root $B/prisma --split dev --out <fresh dir>
#   rows: lexical (raw only) · lexical (cards + raw) · hybrid — the last two appear when the store has cards (it does now)
#   copy <dir>/{coverage,split,results}.json into evals/results/docsqa/<project>/ (paths are already portable)

# A/B on the golden copy (every call through claude -p)
MDA_BIN=$PWD/target/release/mda scripts/eval/ab.sh <corpus> evals/ab/questions.jsonl <fresh out dir> 1 sonnet
scripts/eval/grade.sh evals/ab/questions.jsonl <out dir>     # grades.jsonl + parity.md; checks manifest.json

# carding a checkout through the owner's login (already done for all four; for a new corpus)
mda backend claude-cli --i-accept-the-policy --root <dir>; mda index --limit 500 --root <dir>   # repeat until pending 0; then mda rebuild --embeddings --root <dir>
# long jobs: nohup script > log 2>&1 < /dev/null & disown      (survives the session; the tool's own timeout is 10 min)

# Codex review / pre-mortem: Agent(subagent_type="codex:codex-rescue") with "--fresh --model gpt-6-astra", the packet as paths,
# the reviewer prompt verbatim, and the sentence "do NOT spawn a nested codex exec; the companion thread IS the reviewer".
# Follow-up passes: SendMessage to the same agent id with the new SHA.

# CI: gh run list --branch <b> --limit 1 --json databaseId,headSha ; gh run view <id> --json jobs --jq '.jobs[] | "\(.name): \(.conclusion)"'
# Windows job log: gh api repos/joe-carr-data/markdownattractor/actions/jobs/<job>/logs | sed -E 's/\x1b\[[0-9;]*m//g' | grep -nE "::error|FAIL|panicked"

# patching files from Bash: cargo fmt first (rustfmt re-wraps long lines and breaks literal anchors), then per-block
# Python patches asserting exactly one match; regex anchors with \s* survive re-wrapping.
```

## 5. Caveats and gotchas (this session's; earlier handoffs hold the rest)

- **Branch hygiene**: switching branches carries uncommitted work along (the adapter's untracked `eval/` and `lib.rs` travelled onto another branch; stash before rebasing). `git add -A docs` picks up hook-written `docs/handoffs/*-auto.md`. Never edit `scripts/eval/*.sh` while an A/B run is in flight (bash reads scripts incrementally).
- **Numbers on a page are regenerated by a script from the raw file, never retyped** (Codex caught wrong "before" means once).
- **Partial cards bias the fusion**: at 14% coverage Tailwind's carded rows were far below raw. A carded row is meaningful only at 100% coverage (all four checkouts are at 100% now; check `mda status` before scoring). Product follow-up in the plan.
- **Evidence anchors** in DocsQA are rendered headings: backticks kept, Liquid variables expanded, the page title as an anchor. `normalize_heading` folds case, backticks, `{% … %}` and whitespace; the remaining GitHub Docs misses are includes/variants the source cannot show.
- **The adapter refuses**: an unindexed root, `--out` inside the checkout, a symlinked report file, `--split holdout` without `--open-holdout`. `image_text_evidence_used` is a list in the dataset (non-empty = used).
- **`ab.sh`** refuses an output directory that already holds `runs.jsonl`; `grade.sh` marks a question *incomplete* unless the manifest's question × arm × run set is exactly present; a run with `error: true` or an empty answer is never graded.
- **Windows CI**: `install-action` bash failure was an infrastructure bug; the fix is `cargo test` on Windows. Two of my own Windows-only lints followed (unused variable inside a `cfg(unix)` test; then the pedantic underscore-binding lint): keep fixture handles used on every platform.
- **Store size**: a store is 2–8× its markdown (text held in `sections`, `sections_raw_fts` content and `cards_fts` content; vectors f32). External-content FTS and f16 vectors are the levers; they go on the axis-E table.
- **Raw-lexical latency** on the large corpora is 0.25–0.42 s per query in the debug binary (long OR queries, deep candidate lists, one row read per candidate); measure the release build and the candidate depth before axis B.
- **Codex's verdict** on the plan is conditional; do not treat the plan as accepted for publication: M1–M5 are the conditions.

## 6. Next steps, in order (execution plan §5; each milestone ends with its exit criterion met or the shortfall written down)

0. `gh pr view 21`: merge on eleven green jobs if the watcher did not. Then `git checkout main && git pull`.
1. **M1**: card export (`mda eval --dataset docsqa … --export-cards <file>`, the `{"<section_hash>": <SectionSummary>}` shape `record_cards` writes for the golden set) and commit `evals/results/docsqa/cards-0.1.1-<project>.json` for the four projects; `evals/results/docsqa/model.sha` (hashes of the ONNX model files, see runbook §1); `scripts/eval/freeze.sh` writing `FROZEN.md` and the first development-protocol freeze; `scripts/eval/preflight.sh` with the reconstruction check (rebuild from committed cards → identical whole-dev-split metrics) and three activation probes for the mda and grep arms; `--arm-output` scorer with tests; runbook §4a filled with the real hashes. Exit: preflight passes for mda; the runbook regenerates the exploratory rows exactly.
2. **M2**: qmd (`npm i -g @tobilu/qmd@2.8.3`, one home per project, `collection add` / `update` / `embed`, MCP requests captured, coverage incl. whether `.mdx` is indexed), graphify (`uv tool install graphifyy==0.9.66`, `graphify install`, `/graphify <checkout>` from a session, `graph.json` archived), BM25-over-files script, drivers producing `{question_id, paths[], truncated}`, three probes with traces per arm, build times for T3. Exit: preflight passes for every arm on every project.
3. **M3**: T1 development rows for all arms; pooled judgments (diagnostic); the greedy tuning loop of plan §3 logged in `evals/results/docsqa/TUNING.md`. **M4**: final freeze, T1 on the test split once, "where we lose", Codex pass on scorer/drivers/freeze → **T1 published**, runbook §7 gets its T1 section.
4. **M5**: T2 harness (manifest before the loop, resumable rows, transcript-counted tokens, grounding check, `mda eval --analysis` with the paired bootstrap and the one failure policy, `panel.sh` with Fable via `claude -p --model` and Astra via the Codex CLI, failure-matrix shell test, `--export-questions` for the reference answers), development pilot for throughput, owner decision on the rule-0.9 amendment (T2 servers start cold per question for every arm), Codex harness pass. Then **M6** T2 + T3, **M7** T4, **M8** T5 (`MDA_NOW`), **M9** T6 + README row.
5. In parallel when convenient: the launch checklist (README leads with `/plugin install markdownattractor --marketplace joe-carr-data/markdownattractor`, drop "Not released yet", demo, community marketplace, design partners); the read ledger and notarisation secrets remain owner decisions.
