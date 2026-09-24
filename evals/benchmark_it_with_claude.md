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
| Competitor arm builds and rows (§4c) | independent rerun for graphify (a model pass), regeneration for qmd and BM25-over-files (deterministic indexes, hashed models) | graphify: published beside the original; qmd/BM25: exact rows from the same index, latency machine-dependent |
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
  diff <(jq -S '{runs: [.runs[] | {run, questions: .metrics.questions, success_at_5: .metrics.success_at_5, mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10, results: [.results[] | {id, rank, ndcg_at_10, pages, truncated}]}]}' "$RUN/out/$proj-reconstructed/results.json") \
       <(jq -S '{runs: [.runs[] | {run, questions: .metrics.questions, success_at_5: .metrics.success_at_5, mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10, results: [.results[] | {id, rank, ndcg_at_10, pages, truncated}]}]}' "$REPO/evals/results/docsqa/$proj/results.json") \
    && echo "reconstruction identical: $proj"
done
```

This is exactly what `scripts/eval/preflight.sh development <project>` runs as its *reconstruction* check (plus the frozen-input, binary, model-file, store-completeness, regeneration-from-archived-lists, store-replay, coverage and activation-probe checks); its reports are committed under `evals/results/docsqa/preflight/` and a reproduction may simply run the script and compare its report. Expected development rows (dev split, one run, `mda 0.1.1` under the development freeze): the table on `docs/benchmarks.md` ("carded and hybrid rows at full coverage"), regenerated by `scripts/eval/table.sh`. `mean_ms` is not compared.

**Regeneration without a store (plan §2.0b).** Every question's first ten distinct pages are archived in `results.json`, so the metrics recompute from the file alone: feed a run's page lists back as an arm and compare. A rank the store found beyond ten (its deeper fetch) stays in `results.json` for the reader; no metric depends on it, so ranks are compared up to ten.

```bash
for proj in tailwind-css supabase prisma github-docs; do
  f="$REPO/evals/results/docsqa/$proj/results.json"
  for i in 0 1 2; do
    jq -c --argjson i $i '.runs[$i].results[] | {question_id: .id, paths: .pages, truncated}' "$f" > "$RUN/out/$proj-archived-$i.jsonl"
    "$MDA" --json eval --dataset docsqa --data "$RUN/docsqa-data" --project "$proj" --root "$RUN/$(case $proj in tailwind-css) echo tailwindcss;; *) echo $proj;; esac)" --split dev \
      --arm-output "$RUN/out/$proj-archived-$i.jsonl" --arm-name "$(jq -r --argjson i $i '.runs[$i].run' "$f")" \
      | jq -S '.runs[0] | {run, metrics: (.metrics | del(.mean_ms)), results: [.results[] | {id, rank: (if .rank != null and .rank <= 10 then .rank else null end), ndcg_at_10, pages, truncated}]}' > "$RUN/out/$proj-regen-$i.json"
    diff "$RUN/out/$proj-regen-$i.json" <(jq -S --argjson i $i '.runs[$i] | {run, metrics: (.metrics | del(.mean_ms)), results: [.results[] | {id, rank: (if .rank != null and .rank <= 10 then .rank else null end), ndcg_at_10, pages, truncated}]}' "$f") && echo "regenerates: $proj run $i"
  done
done
```

**4b. Regenerated cards (independent rerun).** On each checkout, `"$MDA" backend claude-cli --i-accept-the-policy --root "$RUN/<dir>"` (your own Claude Code login: the acknowledged personal-use path of ADR-0002), then `"$MDA" index --root "$RUN/<dir>" --limit 500` in rounds until `"$MDA" status --root …` shows `pending 0`, then `"$MDA" rebuild --embeddings --root …`. Reference run (Apple M3 24 GB, Haiku 4.5 through Claude Code, 15–16 workers): tailwindcss 19 min, supabase 89 min, prisma 90 min, github-docs 3 h 59 min; 36,899 cards, 0 failures; list-price equivalent ≈ $176 (informational). Score as in 4a without `--cards`; publish beside the committed-card rows.

## 4c. Competitor arms (M2; development rows today, the T1 recipe once frozen)

Every arm is built and driven by a script under `scripts/eval/arms/`, with the same three rules: one build per freeze (a second build refuses to overwrite the first), the arm's own documented interface for scoring (its MCP tool, never our re-implementation), and the timed call is the scored call (`crates/mda-cli/examples/mcp_time.rs` spawns the arm's MCP server over stdio, records the cold first call and the warm ones, and dumps every result the driver then maps to repository paths). The arm records (`evals/results/docsqa/arms/<arm>-<project>.json`: version, install, effective configuration, build times, coverage against the dataset corpus, model or graph hashes) are what `FROZEN.md`'s Arms section lists, and a probe (`scripts/eval/probe.sh <arm> <project> <question_id> <trace>`) proves the arm's tool was actually used (a call with a non-error result) before any table.

Three rules learned on the first day and now enforced by the scripts, which a reproduction must respect too:

- **No provider key in the environment.** Every script that spawns a model session or an arm's server (`probe.sh`, `mcp-time.sh`, the qmd and graphify drivers and builds) unsets `*_API_KEY` first, the preflight refuses to certify with one present, and the preflight itself is launched with `env -u OPENAI_API_KEY -u GEMINI_API_KEY …` from a shell that exports them: a headless graphify session found `GEMINI_API_KEY` in the shell and ran its whole extraction through Gemini instead of the Claude login (that attempt was discarded).
- **The GPU must be free.** qmd's query expansion and reranker run on Metal; another model server holding the unified memory makes every `query` fail (`kIOGPUCommandBufferCallbackErrorOutOfMemory`), and CPU mode is unusable (5–6 min per question). Stop such servers before the qmd rows.
- **Lift the headless background-wait ceiling** for graphify builds (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0`, set by the script): graphify dispatches dozens of extraction subagents and `claude -p` otherwise ends the session after 600 s with chunks still running.

```bash
cd "$REPO"; export MDA="$REPO/target/release/mda"; cargo build --release --example mcp_time; export MCP_TIME="$REPO/target/release/examples/mcp_time"
# qmd 2.8.3 (Node ≥ 22): one index per project, the collection mask widened to MDX (qmd's own option; its default **/*.md would skip three of the four corpora)
npm i -g @tobilu/qmd@2.8.3 && qmd pull                      # ≈ 2 GB of models under ~/.cache/qmd/models, hashed into the arm record
for p in tailwind-css supabase prisma github-docs; do scripts/eval/arms/qmd.sh build "$p"; done          # collection add --mask '**/*.{md,mdx,markdown}', update, embed; times recorded
for p in tailwind-css supabase prisma github-docs; do for m in full no-rerank bm25; do scripts/eval/arms/qmd.sh drive "$p" "$m" "$RUN/qmd-runs/$p-$m.jsonl"; done; done
#   full = MCP `query` {query, limit 20 → 40, rerank true}; no-rerank = the same with rerank false (the whole configuration diff);
#   bm25 = a lex-only sub-query with rerank false (qmd's lexical form ANDs every term and has no OR fallback: near-zero on long questions by design)
# graphify 0.9.66: built on a COPY of the checkout with graphify's skill and hooks installed at project level inside the copy (never into your Claude Code config)
uv tool install "graphifyy[mcp]==0.9.66"
for p in tailwind-css supabase prisma github-docs; do scripts/eval/arms/graphify.sh build "$p"; done      # headless `claude -p "/graphify <copy> --no-viz"` through your login; whole path, no narrowing; graph.json archived and hashed
for p in tailwind-css supabase prisma github-docs; do scripts/eval/arms/graphify.sh drive "$p" "$RUN/graphify-runs/$p.jsonl"; done   # MCP query_graph, every NODE's src in tool order; token_budget 8000 when under ten pages
for p in tailwind-css supabase prisma github-docs; do scripts/eval/arms/graphify.sh build "$p" haiku; GRAPHIFY_MODEL=haiku scripts/eval/arms/graphify.sh drive "$p" "$RUN/graphify-runs/$p-haiku.jsonl"; done   # the graphify-haiku configuration
# a drive refuses a graph that differs from the archived copy; to score from the archive on another machine, restore it first:
#   mkdir -p "$RUN/graphify/prisma-haiku" && gunzip -c "$REPO/evals/results/docsqa/arms/graphs/graphify-haiku-prisma.graph.json.gz" > "$RUN/graphify/prisma-haiku/graph.json"
# regenerate EVERY committed external-arm result from its archived rows (what the preflight's `regenerate` check does):
for p in tailwind-css supabase prisma github-docs; do d="$(case $p in tailwind-css) echo tailwindcss;; *) echo $p;; esac)"; for rows in "$REPO/evals/results/docsqa/$p/arms/"*.jsonl; do a="$(basename "$rows" .jsonl)"; \
  "$MDA" --json eval --dataset docsqa --data "$RUN/docsqa-data" --project "$p" --root "$RUN/$d" --split dev --arm-output "$rows" --arm-name "$(jq -r '.runs[0].run' "$REPO/evals/results/docsqa/$p/arms/$a.results.json")" \
  | jq -S '.runs[0] | {run, metrics: (.metrics | del(.mean_ms)), results: [.results[] | {id, rank, ndcg_at_10, pages, truncated}]}' | diff - <(jq -S '.runs[0] | {run, metrics: (.metrics | del(.mean_ms)), results: [.results[] | {id, rank, ndcg_at_10, pages, truncated}]}' "$REPO/evals/results/docsqa/$p/arms/$a.results.json") && echo "regenerates: $p $a"; done; done
# BM25-over-files control (no model): FTS5 over whole pages with mda's query form
for p in tailwind-css supabase prisma github-docs; do scripts/eval/bm25-files.sh build "$p"; scripts/eval/bm25-files.sh drive "$p" "$RUN/bm25-runs/$p.jsonl"; done
# score any arm output with the same page rule as the mda rows (a missing question is a miss, listed; no latency column)
"$MDA" eval --dataset docsqa --data "$RUN/docsqa-data" --project prisma --root "$RUN/prisma" --split dev --arm-output "$RUN/qmd-runs/prisma-full.jsonl" --arm-name "qmd full (MCP query, rerank)" --out "$RUN/out/prisma-qmd-full"
# latency through each arm's MCP server, cold first call separate
scripts/eval/mcp-time.sh mda prisma "$RUN/latency/mda-prisma.jsonl"; scripts/eval/mcp-time.sh qmd prisma "$RUN/latency/qmd-prisma.jsonl"
```

**The graphify build is a model pass through your login; budget for it before starting it (it was the step that met the account's weekly limit on 2026-09-23).** Its skill makes the host agent dispatch one general-purpose subagent per 20–25 files; each subagent reads every file in full and writes an extraction JSON; the parent session then polls for completion, re-sending its own ~300K-token context on every poll turn (that is where the cache-read tokens go), and every subagent inherits the session model. Measured (`evals/results/docsqa/arms/graphify-<project>.json`, usage from the Claude Code transcript):- tailwind-css: 825 s, 83 turns, 48 subagents, 8,707,961 cache-read / 1,632,586 cache-creation / 338 uncached input / 410,586 output tokens (claude-sonnet-5, whole session per the transcript's modelUsage), $10.25 list-price equivalent
- supabase: 2053 s, 264 turns, 37 subagents, 75,038,416 cache-read / 5,523,288 cache-creation / 886 uncached input / 1,783,121 output tokens (claude-sonnet-5, whole session per the transcript's modelUsage), $47.17 list-price equivalent
- prisma: two attempts did not complete (the first killed by the headless 600 s background-wait ceiling with 30 of 31 chunks done, the second by the account's weekly usage limit after 69 turns and a $38 equivalent); github-docs: did not complete at 55 of 158 chunks (98 turns, $74 equivalent, weekly limit). On 2026-09-23 at ≈ 12:20 UTC the owner's Claude Code account reported its weekly usage limit while these two builds were running ("You've hit your weekly limit · resets Sep 28"); both ended without a graph. What fraction of a fresh weekly allowance such a build needs is not established (other sessions, including the one driving this benchmark, share the allowance). A second configuration, **`graphify-haiku`** (`graphify.sh build <project> haiku`: Haiku 4.5 as the host model of the whole build, parent session and every extraction subagent; graphify is host-model-agnostic), is being built on fresh copies for all four projects, never resuming a Sonnet cache, so that the model-matched comparison against mda's Haiku cards exists; the Sonnet-built `graphify` arm stays as its own configuration wherever it completed. Records: `arms/graphify-<project>.json` and `arms/graphify-haiku-<project>.json` (usage per resolved model and per token category: uncached input, cache creation, cache reads, output; list-price equivalents labelled as such); durable graph copies under `arms/graphs/`.

  graphify-haiku, measured (fresh copies, wakeups disallowed, Haiku 4.5 host; usage is the transcript's per-model `modelUsage`, whole session): tailwind-css: 342 s, 40 turns, 10 subagents, 4,336,851 cache-read / 867,863 cache-creation / 812 uncached input / 143,619 output tokens (claude-haiku-4-5-20251001, whole session), $2.31; prisma: 340 s, 1 turns, 6 subagents, 2,520,449 cache-read / 237,694 cache-creation / 622 uncached input / 73,155 output tokens (claude-haiku-4-5-20251001, whole session), $0.97; supabase: 451 s, 71 turns, 49 subagents, 10,692,057 cache-read / 2,213,973 cache-creation / 2,618 uncached input / 441,091 output tokens (claude-haiku-4-5-20251001, whole session), $6.14; github-docs: 454 s, 11 turns, 11 subagents, 4,809,358 cache-read / 386,512 cache-creation / 1,428 uncached input / 118,463 output tokens (claude-haiku-4-5-20251001, whole session), $1.60. Retrieval through `query_graph` is near zero on all four; whether the skill's extraction ran as prescribed is not established from the transcripts alone (see the page). Record it as its own configuration; it does not replace the Sonnet arm.

For comparison, markdownattractor's own build of the same corpora is one bounded `claude -p` call per section (Haiku 4.5, hash-keyed, unchanged sections never re-summarised, no agent loop, no polling; `cards-0.1.1-<project>.json.provenance.json`):

- tailwind-css: 1,332 cards for 1,518 sections, 3,591,960 input / 498,631 output tokens, $6.51
- supabase: 6,386 cards for 6,548 sections, 16,615,918 input / 2,675,968 output tokens, $30.48
- prisma: 8,339 cards for 10,438 sections, 21,679,828 input / 3,466,004 output tokens, $39.29
- github-docs: 20,842 cards for 23,066 sections, 55,979,737 input / 8,635,389 output tokens, $99.93

qmd's build makes no remote model call (local EmbeddingGemma-300M inference on Metal; collection add + update + embed 162 / 397 / 496 / 1,397 s, of which embedding 161 / 394 / 494 / 1,390 s). These numbers feed the T3 table (axis E). What T3 must show for any build-cost claim to stand, per the Codex discussion of 2026-09-23 (`docs/reviews/codex/2026-09-23-build-cost-claim.md`): one row per project × configuration × operation (first build, one-edit update); the resolved parent and extraction models; completion state and coverage with its population named; wall time split into active work and waiting; remote usage by category (uncached input, cache creation, cache reads, output; parent vs subagents; polling turns); the CLI's list-price equivalent labelled as such; everything also per 1,000 sections of the common section denominator (never per card, node or chunk); failed attempts kept separately with their cumulative spend; single measurements marked as one run. The accepted wording today is descriptive: mda's Haiku card generation had a lower reported list-price equivalent than the completed Sonnet graphify builds on Tailwind and Supabase but took longer; different models, different artifacts; graphify's usage included headless agent orchestration; qmd built locally without remote model usage. The one-edit update is measured from a completed index with its caches: mda's changed-section card + vector, graphify's documented `/graphify <path> --update` skill flow through the login (its docs pass needs the model; `graphify update` alone re-extracts code only), `qmd update && qmd embed`; the timer stops when the refreshed artifact answers through the normal query interface. A build that does not complete is recorded as such in the arm record (rule 0.3), never dropped. Archived rows, scorer output, the request sent and the per-question latency of the first pass live under `evals/results/docsqa/<project>/arms/<arm>.{jsonl,results.json,request.json,times.jsonl}`; the development rows are rendered on `docs/benchmarks.md` by `scripts/eval/table.sh`. What each arm's row means and where it is truncated is in the arm record and in `docs/benchmarks.md`; the published T1 recipe (frozen versions, hashes, expected metrics) lands in §7 with the table.

## 4d. Tuning (M3; development, never a published number)

The greedy loop of the execution plan §3 runs with `scripts/eval/tune.sh` against the four carded stores. Every trial is archived under `evals/results/docsqa/tuning/<trial>/`: `manifest.json` (the change, the base it was applied to, code SHA, binary sha256, `FROZEN.md` and cards hashes, the summary with denominators, the decision) and the four `results.json`; `TUNING.md` is the ledger written from those files. Two checks, never mixed: **regeneration** (exact) recomputes a trial's metrics from its archived page lists without a store, exactly like the preflight's `regenerate` check (`--arm-output` on rows built from `<project>.results.json`); a **rerun** (`scripts/eval/tune.sh baseline`, then `trial <name> <key=value>…` replaying the manifests' changes in order with `keep` after each kept one, `fetch=<n>` for the fetch-depth candidate) searches again and must give the same rows with the same stores and the binary at the logged SHA, while latency and elapsed time differ by machine; an `embedding_text` change re-embeds every card under a new model id (local, ≈ 1 h for the four corpora). The winner's settings become the defaults of the frozen binary at M4 and appear in `FROZEN.md`'s search configuration line.

**Post-stop exploration (plan §3, 2026-09-24 amendment; information only, never adoptable).** After the stop rule fired at c2, candidates 3–7 were run once each against the unchanged pre-tuning configuration, decided with Codex (`docs/reviews/codex/2026-09-24-post-stop-c3-c7.md`). The selection loop is not resumed: no combination, no new value, no adoption for this release whatever the scores; a passing candidate is a hypothesis for a future, separately declared evaluation.

```bash
cd "$REPO"
scripts/eval/tune.sh base                                   # must print an empty base: post-stop trials run only against the pre-tuning configuration
scripts/eval/tune.sh explore post-stop-c3 search_questions_weight=3
scripts/eval/tune.sh explore post-stop-c4 search_raw_weight=0.7
scripts/eval/tune.sh explore post-stop-c5 search_rrf_k=30
scripts/eval/tune.sh explore post-stop-c6 fetch=60
scripts/eval/tune.sh explore post-stop-c7 search_and_stopwords=true
# each: rerun (same rows expected with the same stores and binary), then the paired comparison, regeneration (exact) from the archived results:
target/release/mda --json eval --compare tailwind-css evals/results/docsqa/tuning/baseline/tailwind-css.results.json evals/results/docsqa/tuning/post-stop-c3/tailwind-css.results.json \
  --compare supabase … --compare prisma … --compare github-docs … --draws 5000 --seed 20260922   # byte-identical to tuning/post-stop-c3/compare.json
```

`explore` refuses to run on a moved base or under a name that is not `post-stop-*`, screens on the unrounded objective difference (≥ 0.01; the ledger prints four decimals of it) and the unrounded per-project guardrail (≥ −0.02, which at denominators 49/37/12/25 permits no net loss of one question anywhere), writes `screen-pass-not-adopted` / `screen-fail-not-adopted` / `invalid-not-adopted` (a failed evaluation or a question set that differs from the baseline's is logged with its reason), and appends the row under the "Post-stop exploratory" table of `TUNING.md` with the paired wins/losses and the 95% interval from `mda eval --compare` (within-project paired bootstrap, 5,000 draws, seed 20260922, SplitMix64 so the draws are the same on every machine; the objective is resampled jointly over the four projects). `keep` refuses these decisions, and `trial`/`keep` refuse any `post-stop-*` name, so nothing from the extension can become a base. Percentiles are nearest-rank (`⌈N·q⌉`-th draw); every project has its own generator stream keyed by the seed and its label, so `--compare` arguments can be given in any order and still regenerate `compare.json` byte for byte. The intervals are descriptive: they do not establish significance across five trials.

## 4e. Pooled labels (plan §2.3; diagnostic at M3, the published second column at M4)

```bash
cd "$REPO"
scripts/eval/pool.sh sample prisma dev 100 20260922          # 100 unlabelled top-5 pairs, seeded, round-robin over the arms → evals/results/docsqa/prisma/pool/dev-sample.jsonl (regeneration, exact)
scripts/eval/pool.sh judge prisma dev fable                  # one panel member through your login: claude -p --model claude-fable-5-1, frozen 0/1/2 rubric, JSON schema → dev-judgments-fable.jsonl (independent rerun: a different sample of judgments is expected; publish beside the original)
scripts/eval/pool.sh column prisma dev                       # every arm rescored over the judged questions with original ∪ pooled labels (a pair is relevant when the judges' mean ≥ 1) → dev-column.json (regeneration, exact, from the committed judgments)
```

The judge sees the question and the page's first 6,000 characters inside `<submission>` tags, never the arm; a judgment that is not a valid structured output is recorded as `null` and never as 0. The column is computed by the same scorer as every other number (`--extra-labels`, `--only-questions`), from the archived page lists; nothing is searched again. At M4 the sample is drawn from the final test-split runs and judged by both panel members (Astra through the Codex CLI, `panel.sh`); until then the numbers are labelled development.

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

### 7.1 T1 — axis A on DocsQA, test split (M4, 2026-09-24)

**Freeze.** `evals/results/docsqa/T1/FROZEN.md` (protocol final, table T1; written by `scripts/eval/freeze.sh --protocol final --table T1 --note "…"` and committed *before* any row; source commit recorded inside it). It records the selection ("original §3 winner; post-stop diagnostics excluded from selection"), the split scored (test, once) and the interval method. Nothing below runs unless `scripts/eval/freeze.sh --protocol final --table T1 --check` passes (`t1.sh` checks first, under a lock, on every invocation).

**Arms.** The M2 builds, unchanged: the qmd indexes (`~/.cache/qmd/<project>.sqlite`, fingerprints in `FROZEN.md`), the archived graphify graphs (`arms/graphs/*.graph.json.gz`, served file hash-checked by the drivers), the BM25-over-files tables, the committed cards. Section 4c has the installs and builds; a reproduction on another machine restores the graphs from the archive before driving.

```bash
cd "$REPO"; export MDA="$REPO/target/release/mda"; cargo build --release --locked && cargo build --release --example mcp_time
# every run is resumable (an arm with results.json is skipped) and refuses to start on changed inputs
for p in tailwind-css supabase prisma github-docs; do scripts/eval/t1.sh run "$p" mda bm25-files graphify graphify-haiku; done   # CPU arms, minutes
for p in tailwind-css supabase prisma github-docs; do scripts/eval/t1.sh run "$p" qmd-full qmd-no-rerank qmd-bm25; done          # GPU: ≈ 60 s per question for qmd full (255 test questions ≈ 4.5 h); nothing else on the GPU
for p in tailwind-css supabase prisma github-docs; do scripts/eval/t1.sh latency "$p"; done                                       # afterwards, alone on the machine: cold first call + warm calls through each MCP server
# pooled labels (plan §2.3): 100 unlabelled top-5 pairs per project from the test-split runs, both judges through your logins, then the column
for p in tailwind-css supabase prisma github-docs; do TABLE=T1 scripts/eval/pool.sh sample "$p" test 100 20260922; TABLE=T1 scripts/eval/pool.sh judge "$p" test fable; TABLE=T1 scripts/eval/pool.sh judge "$p" test astra; TABLE=T1 scripts/eval/pool.sh column "$p" test; done
# the preflight, per project, against T1/FROZEN.md and the T1 rows (15 checks incl. regeneration, replay, reconstruction, probes)
for p in tailwind-css supabase prisma github-docs; do env -u OPENAI_API_KEY -u GEMINI_API_KEY -u ANTHROPIC_API_KEY scripts/eval/preflight.sh T1 "$p"; done
# the page tables, never retyped
scripts/eval/t1.sh table; scripts/eval/t1.sh target; scripts/eval/t1.sh latency-table
```

**Regeneration (the gate, exact).** Every T1 row recomputes from its archived page lists without a store, byte-identical on the metrics and per-question results: the preflight's `regenerate` check does it for the store rows and every external arm file (`T1/<project>/results.json`, `T1/<project>/arms/<arm>.results.json` from `<arm>.jsonl`); the intervals and the target comparison recompute from the same files (`mda eval --interval`, `mda eval --compare`, seed 20260922, SplitMix64, order-independent). Expected hashes are listed at the end of this subsection once the table is published.

**Independent rerun.** T1 is deterministic retrieval; a rerun (`t1.sh run` after moving the T1 rows aside) must reproduce the store rows exactly (the preflight's `replay` check) and the external arms' rows given the same indexes and graphs (qmd's reranker is deterministic on the same model files; graphify's `query_graph` is deterministic on the same graph). Latency differs by machine and is reported with hardware. A rerun is published beside the original, never in its place (plan §2.0b).

## 8. Report

Write `evals/results/reproductions/<date>-<who>.md`: hardware, OS, `mda --version` and SHA, `claude --version`, the model file hashes, the checksums of §2, every attempt (timestamp, step, outcome) including failed ones, the tables of §3–§5 with your numbers next to the expected ones marked *regeneration* (exact match: yes/no) or *rerun* (paired difference), and every deviation. A regeneration that does not match is the most valuable outcome this file can produce: open an issue with the report.
