# Benchmark it with Claude — the reproducibility runbook

This file is written for a Claude Code session (or a person) that wants to reproduce the markdownattractor benchmark from public data and get the same numbers, or as close as the step's determinism allows. It is the human-readable twin of `scripts/eval/preflight.sh`: every published table on `docs/benchmarks.md` must be reproducible by following this file on a clean checkout before it is published (execution plan §7). `/benchmark` in this repository loads this file as a skill.

Rules for whoever runs it: do every step in order; never skip a checksum; write down every deviation (hardware, versions, a step that did not match) in your report; do not tune anything; if a step's number is outside its tolerance, that is a finding, not a reason to rerun until it matches.

## 0. What you get, and how exact it is

| Step | Determinism | Tolerance |
|---|---|---|
| Dataset checksums, clones at the pinned commits, coverage and evidence-anchor counts | exact | none: any difference is a finding |
| Raw and carded retrieval rows (`mda eval --dataset docsqa`) with the **committed cards** | exact (same cards, same binary version) | none |
| Carded rows with **regenerated cards** (Haiku through Claude Code) | close: cards are model output | success@5 within ±0.03 per project |
| Answer-quality rows (`claude -p` arms, `claude -p` grader) | statistical: sampled answers and grades | parity fraction within 2 questions of 12 on the golden set; medians within ±20%; T2 within the published 95% intervals |
| Latency | machine-dependent | published with the hardware; compare ratios, not absolutes |
| Carding wall-clock and list-price equivalent | machine- and plan-dependent | informational |

## 1. Prerequisites

- macOS or Linux, ≈ 3 GB free (clones ≈ 300 MB, stores ≈ 330 MB, models 33 MB; plus qmd's ≈ 2 GB if you run its arm), `git`, `python3`, `jq`, `curl`.
- Rust toolchain: `rust-toolchain.toml` pins it (1.98.1); `curl https://sh.rustup.rs -sSf | sh` installs rustup, the pinned toolchain follows on first `cargo` call.
- Claude Code installed and logged in (for the answer-quality arms and for regenerating cards). Every model call in this benchmark goes through Claude Code's own login; **no API key is needed or used** (`ANTHROPIC_API_KEY` must not be set, so nothing can fall back to it).
- This repository at the SHA named in the table's `FROZEN.md` (for the exploratory rows below: `main` at `41fdbb1` or later), built with `cargo build --release -p mda-cli`; `mda --version` must print the version in `FROZEN.md`.
- The embedding model: `export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models`; the first `mda index` with cards downloads `bge-small-en-v1.5-q` (33 MB) there. Record the model directory's revision (`ls $MDA_MODEL_DIR`).

## 2. Data, pinned

```bash
B=$HOME/.cache/markdownattractor/bench; mkdir -p "$B"; cd "$B"
git clone https://github.com/PowderXu/docsqa-data.git docsqa-data
git -C docsqa-data checkout 19af578bead6c8317d29598c409e982886951cbe
shasum -a 256 docsqa-data/data/manifest.json     # c6193cc88cdf88adc2c8561441b03280415bf871e0b22f7d006a5968c714a361
gunzip -k docsqa-data/data/corpus.jsonl.gz
python3 docsqa-data/scripts/verify.py            # the dataset's own checksum check must pass
```

Four repositories, sparse and blob-filtered, at the exact commits the dataset pins (`docsqa-data/sources.json`):

```bash
clone() { rm -rf "$1"; mkdir -p "$1"; cd "$1"; git init -q; git remote add origin "$2"
  git sparse-checkout init --cone; git sparse-checkout set "$4"
  git fetch -q --depth 1 --filter=blob:none origin "$3"; git checkout -q FETCH_HEAD; echo "$3" > .mda-pinned; cd ..; }
clone github-docs https://github.com/github/docs.git c34e3dccad00f61133c799d20e7d1208a0e6cc92 content
clone prisma      https://github.com/prisma/web.git   c4ac0e9dd35d46ae34b5e979b2768be5cd0c390c apps/docs/content/docs
clone tailwindcss https://github.com/tailwindlabs/tailwindcss.com.git bd868a314bd05ca78acd047e3da289274dd6ccd7 src/docs
clone supabase    https://github.com/supabase/supabase.git 6ea3567948178e81369cd485bc06c5aa40009db3 apps/docs/content
```

Expected file counts (`find <dir>/<path> -name '*.md' -o -name '*.mdx' | wc -l`): github-docs 3,740 · prisma 685 · tailwindcss 197 · supabase 829. The dataset calls the Tailwind project `tailwind-css`; the directory name does not matter because everything keys on `repository_source_path`.

## 3. Index (raw), then coverage and the ingestion gate

```bash
export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models
for d in tailwindcss supabase prisma github-docs; do mda index --no-summarize --root "$B/$d"; done
# tailwind-css ↔ tailwindcss, others share their name
mda eval --dataset docsqa --data "$B/docsqa-data" --project prisma --root "$B/prisma" --split dev --out /tmp/docsqa/prisma
```

Expected (exact) per project, from `coverage.json`:

| Project | docs / sections | labels indexed | anchors found | eligible / excluded (image evidence) | dev / test / holdout |
|---|---|---|---|---|---|
| github-docs | 3,742 / 23,066 | 260 / 260 | 183 / 222 | 161 / 36 | 59 / 108 / 30 |
| prisma | 693 / 10,438 | 179 / 179 | 176 / 176 | 118 / 7 | 37 / 68 / 20 |
| supabase | 836 / 6,548 | 63 / 63 | 47 / 48 | 40 / 12 | 15 / 28 / 9 |
| tailwind-css | 198 / 1,518 | 99 / 99 | 96 / 96 | 84 / 9 | 27 / 51 / 15 |

Expected (exact) raw-lexical dev-split row: success@5 github-docs 0.306 · prisma 0.216 · supabase 0.333 · tailwind-css 0.600 (`docs/benchmarks.md`, exploratory). `split.json` must be byte-identical to `evals/results/docsqa/<project>/split.json` (same seed 20260922, same ids). Never pass `--open-holdout`.

## 4. Cards

Two ways. **Committed cards (exact):** once `evals/results/docsqa/cards-<version>-<project>.json` exist (milestone M1 of the execution plan), attach them: `mda eval … --cards evals/results/docsqa/cards-<version>-<project>.json`, which also embeds them; the carded and hybrid rows are then exact. **Regenerate (close):** on each checkout, `mda backend claude-cli --i-accept-the-policy --root "$B/<dir>"` (your own Claude Code login; this is the acknowledged personal-use path of ADR-0002), then `mda index --root "$B/<dir>"` in rounds (`--limit 500`) until `mda status` shows `pending 0`, then `mda rebuild --embeddings`. Reference run on an Apple M3, Haiku 4.5, 15–16 workers: tailwind 19 min, supabase 89 min, prisma 90 min, github-docs 3 h 59 min, 36,899 cards, 0 failures; list-price equivalent ≈ $176 (informational; through the login it cost nothing). A carded row is meaningful only at 100% coverage: check `mda status` before scoring.

## 5. Answer quality on the golden corpus (statistical)

```bash
cp -R evals/golden/docs /tmp/golden && mda index /tmp/golden        # cards from evals/golden/cards.json are attached by `mda eval`; for the A/B, card the copy or attach as above
MDA_BIN=$PWD/target/release/mda scripts/eval/ab.sh /tmp/golden evals/ab/questions.jsonl /tmp/ab-out 1 sonnet
scripts/eval/grade.sh evals/ab/questions.jsonl /tmp/ab-out          # parity.md; a missing or failed run shows as ungraded/incomplete, never as zero
```

Reference (2026-09-22, lean payload): parity 11 of 12; index mean 5.50, baseline 5.42; median source tokens index 762.5, baseline 245.5 (all 12 questions; `evals/ab/results/2026-09-22-golden-lean.md`). Your run is a different sample of Sonnet answers: compare within the tolerances of §0.

## 6. Competitor arms (from milestone M2 on)

Pinned installs and drivers are specified in the execution plan §2.1; this section is filled in when they land, with the exact commands, the coverage each arm reached and the three activation probes to rerun. Until then, competitor numbers on the page do not exist and none should be quoted.

## 7. Report

Write `evals/results/reproductions/<date>-<who>.md`: hardware, OS, versions (`mda --version`, `claude --version`, model directory listing), the checksums of §2, the tables of §3 and §5 with your numbers next to the expected ones, tolerance verdict per row, deviations. A reproduction that finds a difference outside tolerance is the most valuable outcome this file can produce: open an issue with the report.
