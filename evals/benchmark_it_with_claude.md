# Benchmark it with Claude — the reproducibility runbook

Written for a Claude Code session (or a person) that wants to reproduce the markdownattractor benchmark from public data. It is the human-readable twin of `scripts/eval/preflight.sh` (execution plan §2.0). `/benchmark` loads it as a skill. Two different things are called "reproducing" here, and the two are never mixed (plan §2.0b):

- **Regeneration**: recompute a published table from its archived raw observations on a clean checkout. Must be byte-identical. This is the gate every table passes before it is published.
- **Independent rerun**: run the stochastic parts again (Claude answers, grades) with the frozen inputs and the same repeat count. Its result is published next to the original with the paired difference, whatever it shows. There is no pass/fail and nothing is rerun "until it matches".

Rules for whoever runs it: do every step in order; every checksum line is an assertion, stop when one fails; never tune; never open the holdout; record every attempt in the report (§8), including the ones that failed; never write into `evals/results/docsqa/` or `docs/benchmarks.md` from a reproduction.

## 0. What each step gives you

| Step | Kind | What "same" means |
|---|---|---|
| Dataset checksums, clone SHAs, file counts, coverage and anchor counts (§2–3) | regeneration | exact, any difference is a finding |
| Raw-lexical retrieval rows with the same `mda` version (§3) | regeneration | exact metrics (same store, same binary) |
| Carded and hybrid rows with the **committed cards** and the **hashed model files** (§4) | regeneration | exact metrics |
| Carded rows with **regenerated cards** (§4b) | independent rerun | published beside the original; no tolerance |
| Answer-quality tables from the **archived logs and grades** (§5a) | regeneration | byte-identical `parity.md` / analysis output |
| Answer-quality tables **rerun** (§5b) | independent rerun | published beside the original; no tolerance |
| Latency (§6) | machine-dependent | reported with hardware; compare ratios |

Historical note: the numbers on `docs/benchmarks.md` labelled *exploratory* (the raw/carded dev rows of 2026-09-22 and the golden-corpus A/B) predate any freeze and were produced with `mda` 0.1.1 at `41fdbb1`. They regenerate exactly only with that binary; a later binary may rank differently on purpose (tuning, plan §3). Final tables each carry their own `FROZEN.md` and their own section here (§7).

## 1. Environment, asserted

```bash
set -euo pipefail
REPO="$HOME/markdownattractor"           # the source checkout; every later command is run from here or uses absolute paths
RUN="$HOME/.cache/markdownattractor/bench"   # data and stores; a fresh run uses a fresh directory
[ -z "${ANTHROPIC_API_KEY:-}" ] || { echo "unset ANTHROPIC_API_KEY: every model call goes through Claude Code's login"; exit 1; }
EXPECT_SHA=$(sed -n 's/^- source commit: \([0-9a-f]\{40\}\).*/\1/p' "$HOME/markdownattractor/evals/results/docsqa/FROZEN.md")   # the frozen source commit (development protocol today; a published table's own FROZEN.md at §7); the exploratory rows of 2026-09-22 were produced at 41fdbb1
EXPECT_VERSION="mda 0.1.1"
cd "$REPO" && [ "$(git rev-parse HEAD)" = "$(git rev-parse "$EXPECT_SHA")" ] || { echo "checkout is not $EXPECT_SHA"; exit 1; }
cargo build --release -p mda-cli && MDA="$REPO/target/release/mda" && [ "$("$MDA" --version)" = "$EXPECT_VERSION" ] || exit 1
claude --version                                  # recorded; the answering and grading model ids resolved by the alias are compared with FROZEN.md before a rerun (one probe call, its result JSON `model` field) and read from every run's result JSON afterwards
export MDA_MODEL_DIR="$HOME/.cache/markdownattractor/models"
```

The embedding model (`bge-small-en-v1.5-q`, 33 MB) downloads on the first embedding pass; after it, hash the files and compare with `FROZEN.md`:

```bash
( cd "$MDA_MODEL_DIR" && find . -type f ! -name '*.lock' | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r f; do printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -c1-64)" "$f"; done ) > "$RUN/model.sha"
diff "$RUN/model.sha" "$REPO/evals/results/docsqa/model.sha" || { echo "embedding model files differ from the committed model.sha (its own sha256 is in FROZEN.md)"; exit 1; }
```

Prerequisites: macOS or Linux, ≈ 3 GB free (clones ≈ 300 MB, stores ≈ 330 MB; qmd's arm adds ≈ 2 GB of models), `git`, `python3`, `jq`, `curl`, the Rust toolchain (`rust-toolchain.toml` pins 1.98.1), Claude Code installed and logged in. The one model call that does not go through Claude Code is the Astra half of the grading panel, which goes through the Codex CLI (plan §2.4).

## 2. Data, pinned and asserted

```bash
mkdir -p "$RUN" && cd "$RUN"
[ ! -e docsqa-data ] || { echo "docsqa-data exists: use a fresh RUN directory, do not delete evidence"; exit 1; }
git clone -q https://github.com/PowderXu/docsqa-data.git docsqa-data
git -C docsqa-data checkout -q 19af578bead6c8317d29598c409e982886951cbe
[ "$(git -C docsqa-data rev-parse HEAD)" = 19af578bead6c8317d29598c409e982886951cbe ] || exit 1
[ "$(shasum -a 256 docsqa-data/data/manifest.json | cut -c1-64)" = c6193cc88cdf88adc2c8561441b03280415bf871e0b22f7d006a5968c714a361 ] || exit 1
gunzip -k docsqa-data/data/corpus.jsonl.gz
[ "$(shasum -a 256 docsqa-data/data/corpus.jsonl | cut -c1-64)" = d7ade1a007c04fcec5627b1583d31f360ef5f672da466599d5b156c3ea9ff1b4 ] || exit 1
python3 docsqa-data/scripts/verify.py            # the dataset's own check must pass

clone() { # dir url sha sparse-path — refuses an existing directory, asserts the checked-out SHA
  [ ! -e "$1" ] || { echo "$1 exists"; exit 1; }
  mkdir -p "$1" && ( cd "$1" && git init -q && git remote add origin "$2" && git sparse-checkout init --cone \
    && git sparse-checkout set "$4" && git fetch -q --depth 1 --filter=blob:none origin "$3" && git checkout -q FETCH_HEAD \
    && [ "$(git rev-parse HEAD)" = "$3" ] && echo "$3" > .mda-pinned )
}
clone github-docs https://github.com/github/docs.git c34e3dccad00f61133c799d20e7d1208a0e6cc92 content
clone prisma      https://github.com/prisma/web.git   c4ac0e9dd35d46ae34b5e979b2768be5cd0c390c apps/docs/content/docs
clone tailwindcss https://github.com/tailwindlabs/tailwindcss.com.git bd868a314bd05ca78acd047e3da289274dd6ccd7 src/docs
clone supabase    https://github.com/supabase/supabase.git 6ea3567948178e81369cd485bc06c5aa40009db3 apps/docs/content
```

Expected markdown/MDX file counts inside the sparse path: github-docs 3,740 · prisma 685 · tailwindcss 197 · supabase 829. The sparse cone also checks out the repository's top-level files (README, CONTRIBUTING and the like), which is why the indexed document counts in §3 are slightly higher; the indexed-file list is `mda --json recent 100000 --root <dir>` and its hash belongs in your report. The dataset calls the Tailwind project `tailwind-css`; the directory name does not matter, everything keys on `repository_source_path`.

## 3. Raw index, coverage, the ingestion gate (regeneration, exact)

```bash
cd "$REPO"
for d in tailwindcss supabase prisma github-docs; do "$MDA" index --no-summarize --root "$RUN/$d"; done
for p in tailwind-css:tailwindcss prisma:prisma supabase:supabase github-docs:github-docs; do
  "$MDA" eval --dataset docsqa --data "$RUN/docsqa-data" --project "${p%%:*}" --root "$RUN/${p##*:}" --split dev --out "$RUN/out/${p%%:*}"
  diff <(jq -S . "$RUN/out/${p%%:*}/split.json") <(jq -S . "$REPO/evals/results/docsqa/${p%%:*}/split.json") && echo "split identical: ${p%%:*}"
done
```

Expected, exact, from `coverage.json`:

| Project | docs / sections | labels indexed | anchors found | eligible / excluded (image evidence) | dev / test / holdout |
|---|---|---|---|---|---|
| github-docs | 3,742 / 23,066 | 260 / 260 | 183 / 222 | 161 / 36 | 59 / 108 / 30 |
| prisma | 693 / 10,438 | 179 / 179 | 176 / 176 | 118 / 7 | 37 / 68 / 20 |
| supabase | 836 / 6,548 | 63 / 63 | 47 / 48 | 40 / 12 | 15 / 28 / 9 |
| tailwind-css | 198 / 1,518 | 99 / 99 | 96 / 96 | 84 / 9 | 27 / 51 / 15 |

Expected raw-lexical dev row with `mda` 0.1.1 (exploratory): success@5 github-docs 0.306 · prisma 0.216 · supabase 0.333 · tailwind-css 0.600. Compare `runs[0].metrics` in `results.json` with `evals/results/docsqa/<project>/results.json` to three decimals; `mean_ms` is not compared. Never pass `--open-holdout`.

## 4. Cards

**4a. Committed cards (regeneration, exact).** Since milestone M1 the cards of the four corpora are committed as `evals/results/docsqa/cards-0.1.1-<project>.json` (one card per line, `{"<section_hash>": <SectionSummary>}`) with a `.provenance.json` beside each (backend `claude-cli`, model `claude-haiku-4-5`, prompt `section.v2`, a few tiny sections carded deterministically, time span, list-price usage). Their hashes are in `evals/results/docsqa/FROZEN.md` (protocol *development*, source commit in that file) and are, at the time of writing:

| File | sha256 | Cards |
|---|---|---|
| `cards-0.1.1-github-docs.json` | `1872bcda4b507c6671811f181b26383fa7cf0aaf7759ca88e311b02757262481` | 20842 cards for 23066 of 23066 sections (complete: true) |
| `cards-0.1.1-prisma.json` | `80a05cf7807989a21480c3e6c47e2591201c3aeb74dc0478b579ba01a48d0d87` | 8339 cards for 10438 of 10438 sections (complete: true) |
| `cards-0.1.1-supabase.json` | `92027a0578c933e128cbec9aab201963b4829b8bf0c2b099e85501322c390692` | 6386 cards for 6548 of 6548 sections (complete: true) |
| `cards-0.1.1-tailwind-css.json` | `4ba04219891dda408474711ddccff52fe85376260fe69febd96a25de4eebb041` | 1332 cards for 1518 of 1518 sections (complete: true) |

Assert the hashes against `FROZEN.md` (never against this page), then attach and score on a **clean, raw-indexed copy** of each checkout: the point of the check is that the cards plus the hashed model files rebuild the index without a model.

```bash
cd "$REPO"
for f in evals/results/docsqa/cards-0.1.1-*.json; do
  grep -q "$(basename "$f"): sha256 $(shasum -a 256 "$f" | cut -c1-64)" evals/results/docsqa/FROZEN.md || { echo "$f does not match FROZEN.md"; exit 1; }
done
for p in tailwind-css:tailwindcss supabase:supabase prisma:prisma github-docs:github-docs; do
  proj="${p%%:*}"; dir="${p##*:}"; copy="$RUN/reconstruct/$dir"
  [ ! -e "$copy" ] || { echo "$copy exists: use a fresh RUN directory"; exit 1; }
  mkdir -p "$copy" && rsync -a --exclude .markdownattractor --exclude .git "$RUN/$dir/" "$copy/"
  "$MDA" index --no-summarize --root "$copy" && "$MDA" embeddings local-small --root "$copy"
  "$MDA" eval --dataset docsqa --data "$RUN/docsqa-data" --project "$proj" --root "$copy" --split dev \
    --cards "$REPO/evals/results/docsqa/cards-0.1.1-$proj.json" --out "$RUN/out/$proj-reconstructed"
done
```

`--cards` attaches every card whose section hash is in the store (the `cards:` line of the output must read *N of N sections carded*; a carded row below full coverage is not publishable) and embeds them with the hashed model files; the raw, carded and hybrid rows must then be exact:

```bash
for proj in tailwind-css supabase prisma github-docs; do
  diff <(jq -S '{runs: [.runs[] | {run, questions: .metrics.questions, success_at_5: .metrics.success_at_5, mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10, results: [.results[] | {id, rank, ndcg_at_10, top, truncated}]}]}' "$RUN/out/$proj-reconstructed/results.json") \
       <(jq -S '{runs: [.runs[] | {run, questions: .metrics.questions, success_at_5: .metrics.success_at_5, mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10, results: [.results[] | {id, rank, ndcg_at_10, top, truncated}]}]}' "$REPO/evals/results/docsqa/$proj/results.json") \
    && echo "reconstruction identical: $proj"
done
```

This is exactly what `scripts/eval/preflight.sh development <project>` runs as its *reconstruction* check (plus the frozen-input, model-file, store-completeness, regeneration, coverage and activation-probe checks); its reports are committed under `evals/results/docsqa/preflight/` and a reproduction may simply run the script and compare its report. Expected development rows (dev split, one run, `mda 0.1.1` under the development freeze): the table on `docs/benchmarks.md` ("carded and hybrid rows at full coverage"), regenerated by `scripts/eval/table.sh`. `mean_ms` is not compared.

**4b. Regenerated cards (independent rerun).** On each checkout, `"$MDA" backend claude-cli --i-accept-the-policy --root "$RUN/<dir>"` (your own Claude Code login: the acknowledged personal-use path of ADR-0002), then `"$MDA" index --root "$RUN/<dir>" --limit 500` in rounds until `"$MDA" status --root …` shows `pending 0`, then `"$MDA" rebuild --embeddings --root …`. Reference run (Apple M3 24 GB, Haiku 4.5 through Claude Code, 15–16 workers): tailwindcss 19 min, supabase 89 min, prisma 90 min, github-docs 3 h 59 min; 36,899 cards, 0 failures; list-price equivalent ≈ $176 (informational). Score as in 4a without `--cards`; publish beside the committed-card rows.

## 5. Answer quality on the golden corpus

**5a. Regeneration (exact).** The archived observations are `evals/ab/results/<date>-*.md` with their `runs.jsonl` and `grades.jsonl` under `evals/results/` (from M5; the 2026-09-22 runs predate the archive rule and are exploratory). Recompute the table from the archived `grades.jsonl` with `scripts/eval/grade.sh --table-only <questions> <dir>` (M5) and diff against the published file: it must be byte-identical.

**5b. Independent rerun.** Card the golden copy the same way as 4b (there is no committed card set for a scratch copy; the `evals/golden/cards.json` set is attached by `mda eval --golden`, which is the retrieval eval, not the A/B), then:

```bash
A="$RUN/attempts/$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$A"        # one directory per attempt; nothing is overwritten, every attempt stays
cd "$REPO"; cp -R evals/golden/docs "$A/golden" && "$MDA" backend claude-cli --i-accept-the-policy --root "$A/golden" && "$MDA" index --root "$A/golden"
MDA_BIN="$MDA" scripts/eval/ab.sh "$A/golden" evals/ab/questions.jsonl "$A/ab-out" 1 sonnet   # refuses a directory that already has runs.jsonl
scripts/eval/grade.sh evals/ab/questions.jsonl "$A/ab-out"                                # a failed run is ungraded/incomplete, never zero
```

Read the resolved model ids from `$A/ab-out/*.jsonl` (`model` in the result event) and record them; the attempt directory is part of the report. Reference (2026-09-22, exploratory, lean payload, one run): parity 11 of 12; index mean 5.50, baseline 5.42; median source tokens index 762.5, baseline 245.5 over all 12 questions (`evals/ab/results/2026-09-22-golden-lean.md`). Publish yours beside it with the paired difference; a different sample of Sonnet answers is expected to differ.

## 6. Latency

Through each tool's MCP server with the common client (`scripts/eval/mcp-time.sh`, M2): one server per arm per project, the first query reported as cold (process start and model load included), the rest warm; hardware, OS and the release build in the report. Ratios between arms on the same machine are the comparable quantity.

## 7. Published tables

One subsection per published table lands here with the table (plan §7): its `FROZEN.md` path, the exact regeneration command and expected artifact hashes, the competitor arm commands with the coverage each reached and the three activation probes to rerun, and the independent-rerun protocol. Until a table's subsection exists here, it is not published and must not be quoted.

## 8. Report

Write `evals/results/reproductions/<date>-<who>.md`: hardware, OS, `mda --version` and SHA, `claude --version`, the model file hashes, the checksums of §2, every attempt (timestamp, step, outcome) including failed ones, the tables of §3–§5 with your numbers next to the expected ones marked *regeneration* (exact match: yes/no) or *rerun* (paired difference), and every deviation. A regeneration that does not match is the most valuable outcome this file can produce: open an issue with the report.
