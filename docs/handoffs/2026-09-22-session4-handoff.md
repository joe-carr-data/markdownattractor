# Handoff — 2026-09-22, session 4 (benchmark plan B0a, B0b, DocsQA adapter, ingestion gate)

Written by Claude (Fable 5.1) at the end of the fourth build session, for the next session after compaction. **Read this whole file first, then the reading list in §3.** Everything below was verified in-session unless marked otherwise. The previous handoff (`2026-09-22-session3-final-handoff.md`) stays accurate for releases, the history rewrite, the plugin install path and the recipes; this one supersedes it for repo state and next steps.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF

## 0. One-paragraph state

The benchmark plan (`docs/plans/2026-09-benchmarks.md`, v3.2) is under way. Merged today: **PR #17** (plan §0a.3: every benchmark model call goes through the owner's Claude Code login, never an API key), **PR #18** `.mdx` ingestion (B0a, Codex-reviewed, 6/6 fixed), **PR #19** lean `mda_search` payload + eval scripts through `claude -p` (B0b, Codex-reviewed, 4 fixed + 2 in part). Open at the time of writing: **PR #20** the DocsQA-Repo adapter (`mda eval --dataset docsqa`), coverage report and seeded split, with the **ingestion gate passed at 100% label coverage on all four projects** and the first axis-A row (raw lexical, dev split) published as the floor. Codex review of #20 requested at `edb518b`; CI was running. 294 tests, `make check` green. The four repositories are cloned at their pinned commits and raw-indexed under `~/.cache/markdownattractor/bench/`; the dataset is at `bench/docsqa-data` (corpus decompressed). No cards exist for the DocsQA corpora yet: that is the next task.

## 1. Repo, branches, PRs, threads

| Item | State |
|---|---|
| `main` | `129de27` (merge of #19) plus #20 when it merges. Tree clean apart from hook-written `docs/handoffs/*-auto.md`. |
| Open PR | **#20** `feat/eval-docsqa` at `edb518b`: merge on a green job list (`gh run view <id> --json jobs`) **after** triaging the Codex review into `docs/reviews/codex/2026-09-22-docsqa-adapter.md` (file not yet written when this handoff was made; check `gh pr view 20` and the review agent's output). |
| Merged today | #17 plan §0a.3; #18 `.mdx` (review `docs/reviews/codex/2026-09-22-mdx-ingestion.md`, thread `01a0ca73-bcc9-7770-b083-cd7abebbc986`); #19 lean payload (review `docs/reviews/codex/2026-09-22-lean-payload.md`, thread `01a0ca82-90c3-7530-bf8f-5ffbc2938b1d`). |
| Codex | CLI 0.155.1, `gpt-6-astra`, via the `codex:codex-rescue` subagent. **The subagent must be told not to spawn a nested `codex exec`** (it did on the first attempt and failed to initialise); the companion thread it opens is the reviewer and answers directly. Say so in the prompt every time. |
| Bench data on this machine | `~/.cache/markdownattractor/bench/{github-docs,prisma,supabase,tailwindcss}` (sparse, blob-filtered clones at the pinned SHAs, marker `.mda-pinned`; `clone.sh` there recreates them) and `bench/docsqa-data` (clone of `PowderXu/docsqa-data`, `data/corpus.jsonl` decompressed). Each checkout has a raw index in its `.markdownattractor/` (no cards, no vectors). Note the directory is `tailwindcss` while the dataset calls the project `tailwind-css`; the adapter keys on `repository_source_path`, so the name does not matter. |
| Spend this session | ≈ $1.0 list-price equivalent of Claude Code usage (three A/B run sets on the golden corpus, grading, smoke test). No API-key spend. |
| Scratch | Session scratchpad: `ab-lean` (lean-payload A/B logs and grades), `ab-smoke`, `docsqa-out` (the four adapter outputs, copied into `evals/results/docsqa/`), `mdx-accept.py` (the heading-preservation checker used for `design/ingestion.md`). Disposable. |

## 2. What this session did, in order

1. **Plan §0a.3** (PR #17): Claude Code login for every benchmark call; rule 0.7 now counts tokens from the transcript's per-turn `usage`; budget figures stay as the ask-before threshold in list-price equivalent.
2. **B0a `.mdx` ingestion** (PR #18; `docs/design/ingestion.md`): `walk` accepts `.mdx`; `markdown::Flavor` chosen from the path and used by every re-parse; leading ESM block excluded (MDX only, real statements only); headings inside CommonMark HTML blocks of type 6/7 recovered with fence/comment/`<pre>` state, parsed by comrak; title falls back to front matter `title:` (YAML or TOML) then `export const title`; snippets skip markup-only lines. Checked on the four corpora: every heading kept, every non-partial page titled (before: 0 of 1,711 MDX pages had a title). Codex found six real bugs in the first version (prose starting with "import", `#` inside nested fences, `# C#`, …); all fixed, rules added to `.claude/rules/rust.md`.
3. **B0b lean payload** (PR #19; `design/mcp.md`): `SearchView`/`HitView`, `k` 5, no diagnostics, snippet only without a card, `partial` flag. `grade.sh` through `claude -p --json-schema` with the rubric as system prompt and the submission as tagged data; `ab.sh` resolves paths before `cd`, fails loud, writes `manifest.json`; `grade.sh` checks completeness against it. Golden-corpus numbers regenerated from the raw grades (`evals/ab/results/2026-09-22-golden-lean.md`): median source tokens 1,304.5 → 762.5 at 11/12 parity, but more turns; exploratory, one run, protocol changes disclosed.
4. **DocsQA adapter** (PR #20; `mda_core::eval`, `evals/README.md`): coverage, split, page-level metrics; results in `evals/results/docsqa/<project>/`. Raw-lexical dev-split success@5: github-docs 0.306, prisma 0.216, supabase 0.333, tailwind 0.600; ≈ 0.5 s per query on the three larger corpora.

## 3. Files to re-read, in order

1. `CLAUDE.md`; `.claude/rules/{rust,workflow,docs}.md` (rust rules now end with the `.mdx` review's three lines); `docs/STATUS.md`; `docs/aha.md` (top four lines are this session).
2. `docs/plans/2026-09-benchmarks.md` (v3.2; §0a.3; §5 task status lines for B0, B0a, B0b, B2); the three reviews `docs/reviews/codex/2026-09-22-{mdx-ingestion,lean-payload}.md` and, once written, `2026-09-22-docsqa-adapter.md`.
3. `docs/design/ingestion.md` (new), `docs/design/mcp.md` (lean view), `docs/benchmarks.md` (lean-payload and DocsQA sections), `evals/README.md`, `evals/ab/results/2026-09-22-golden-lean.md`.
4. Code: `crates/mda-core/src/markdown/mod.rs` (`Flavor`, `leading_esm_block`, `is_esm_statement`, `RawScan`, `atx_heading`, `quoted_string`), `crates/mda-core/src/eval/{mod,docsqa}.rs`, `crates/mda-core/src/mcp.rs` (`SearchView`, `HitView`, `DEFAULT_K`), `crates/mda-cli/src/commands/eval.rs` (`run_docsqa`), `scripts/eval/{ab,grade}.sh`, tests in `crates/mda-core/src/markdown/tests.rs` (MDX section), `crates/mda-cli/tests/{engine_cli,mcp_cli}.rs`.
5. Earlier handoffs for everything else: `2026-09-22-session3-final-handoff.md` (§5 recipes, §6 caveats), then the Phase 4 / Phase 2 / session-1 handoffs for internals.

## 4. Recipes that worked this session

```bash
# DocsQA, per project (dir names on this machine: github-docs prisma supabase tailwindcss)
export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models; B=$HOME/.cache/markdownattractor/bench
target/debug/mda index --no-summarize --root $B/prisma            # raw index, 2.5 s (github-docs 8.8 s)
target/debug/mda eval --dataset docsqa --data $B/docsqa-data --project prisma --root $B/prisma --split dev --out <dir>
# then copy <dir>/{coverage,split,results}.json into evals/results/docsqa/<project>/ with $HOME replaced by ~

# A/B on the golden copy (scratchpad/ab-corpus, indexed with cards + vectors), every call through claude -p
MDA_BIN=$PWD/target/debug/mda scripts/eval/ab.sh <corpus> evals/ab/questions.jsonl <fresh out dir> 1 sonnet
scripts/eval/grade.sh evals/ab/questions.jsonl <out dir>          # writes grades.jsonl + parity.md; checks manifest.json

# heading-preservation check on a corpus (the script is in the scratchpad; recreate from design/ingestion.md if gone)
python3 mdx-accept.py target/debug/mda <docs dir>

# Codex review prompt must contain: "do NOT spawn a nested codex exec; the companion thread IS the reviewer"
```

## 5. Caveats and gotchas (this session's)

- **Do not commit hook-written auto handoffs by accident**: `git add -A docs` picks up `docs/handoffs/*-auto.md` (one slipped into #19; harmless, but the docs rule says hooks own those files).
- **Switching branches carries uncommitted work along**: the adapter's untracked `eval/` directory and `lib.rs` edit travelled onto the lean branch; stash unrelated work before a rebase.
- **A running bash script reads itself incrementally**: never edit or stash `scripts/eval/*.sh` while an A/B run is in flight.
- **`cargo nextest` rebuilds `target/debug/mda`**: do not run the gate while a background loop is executing that binary.
- **Numbers on a page come from a script over the raw file**, never retyped (Codex caught wrong "before" means in the lean-payload table).
- `ab.sh` refuses an output directory that already holds `runs.jsonl`; `grade.sh` marks a question **incomplete** when the manifest's question × arm × run set is not exactly present.
- `claude -p --system-prompt` plus `--json-schema` plus stdin works for the grader; the answer is data inside `<submission>` tags.
- The adapter refuses an unindexed root (`mda index --no-summarize <root>` first) and needs `data/corpus.jsonl` decompressed.
- `image_text_evidence_used` is a **list** in the dataset (non-empty = used); `requires_multimodal_judgment` is a bool; the adapter accepts both shapes.
- Raw-lexical latency on the large corpora is ≈ 0.5 s/query in the debug binary with `--fetch 30` (120 candidates per list, one row read each); measure the release build and a smaller fetch before axis B.

## 5b. Addendum (later the same session)

- PR #20's Codex review (`docs/reviews/codex/2026-09-22-docsqa-adapter.md`): six findings, all fixed on the branch (symlink-safe reports, evidence-anchor check, fetch until ten distinct pages, sealed holdout behind `--open-holdout`, deduplicated labels, `~` paths). Results regenerated: evidence anchors 96/96, 176/176, 47/48, 183/222 (GitHub Docs' misses are rendered Liquid includes/variants, stated on the page).
- **Carding rate measured** on Tailwind with the `claude-cli` backend (policy acknowledged on that checkout's `config.toml`): 200 sections in 182 s, 0 failures, $0.85 list-price equivalent → all four corpora ≈ 10 h and ≈ $175 equivalent. The owner ruled the dollar figure irrelevant (Max plan, Claude Code login: "100% free"); **the full carding run was started detached** (`~/.cache/markdownattractor/bench/card-all.sh`, log `card-all.log`, per-round logs `card-<dir>-round<n>.log`, order tailwindcss → supabase → prisma → github-docs, `--limit 500` per round, sleeps 20 min after three rounds without progress). Check `card-all.log` first thing next session; if it died, re-run the script (it resumes from the store). Tailwind's index now holds 216 cards + vectors (1,116 sections pending); the other three checkouts have raw indexes only.
- **Partial cards bias the fusion**: with 14% carded, Tailwind's cards+raw and hybrid rows fell far below raw (published as such). Carded rows are meaningful only at full coverage; the product implication (first backfill) is a plan follow-up.

## 6. Next steps, in order

1. **Finish PR #20**: triage the Codex review into `docs/reviews/codex/2026-09-22-docsqa-adapter.md`, fix what it finds, merge on a green job list, update `STATUS` (Last Codex review line).
2. **Cards for the four DocsQA corpora** (plan B2, §0a.3) — running (see 5b); when `card-all.log` says ALL DONE, write the card export (`{"<section_hash>": <SectionSummary>}` per project, like `mda eval --record` does for the golden set; a `mda eval --dataset docsqa … --export-cards <file>` flag is the natural home) and commit the files. If it is still running, leave it. The old recipe: on each checkout set `backend = "claude-cli"` and `claude_cli_policy_ack = true` in `.markdownattractor/config.toml`, run `mda index` (summarize) in bounded rounds (`--limit`), watch the rate limit, record wall-clock per corpus under axis E, then export the cards (`{"<section_hash>": <SectionSummary>}`, the shape `mda eval --record` writes for the golden set) to `evals/results/docsqa/cards-<version>-<project>.json` and commit them (rule 0.9). ≈ 41K sections in total: start with tailwind (1.5K) to measure the rate, then prisma, supabase, github-docs. Ask the owner before exceeding the list-price-equivalent budget or if a corpus stretches past a day.
3. **Carded and hybrid axis-A rows** on the dev split with `--cards`; embed with `MDA_MODEL_DIR` set. Then the qmd arm (`npm i -g @tobilu/qmd`, full config + reranker-off) and BM25-over-files; publish the axis-A table (dev split, labelled) and the "where we lose" notes.
4. **B0 leftovers**: transcript-counted tokens in `ab.sh` (rule 0.7), grounding check in `grade.sh`, `FROZEN.md` writer, `scripts/eval/panel.sh` (Fable via `claude -p --model`, Astra via the Codex CLI), per-arm manifests with smoke traces; the harness failure-matrix shell test.
5. Latency lever for raw search on large corpora (candidate depth, prepared statements, release build), measured, before axis B.
6. Launch checklist in parallel when convenient (README install line, drop "Not released yet", demo, community marketplace, design partners). Owner decisions still open: Apple notarisation secrets; read ledger (schema v4).
