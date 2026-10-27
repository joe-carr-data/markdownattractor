#!/usr/bin/env bash
# Write (or check) FROZEN.md, the record of every input a benchmark number depends on
# (execution plan §2.0, strategy rule 0.1). Every run's preflight calls `freeze.sh --check`
# so a run against changed inputs refuses to start.
#
# Usage: scripts/eval/freeze.sh --protocol development|final [--table T1] [--out FILE]
#   --table T6          our own corpora (M9): the public corpora's freeze under evals/results/own/T6/ (RESULTS=evals/results/own)
#   --table T6-private  the private corpus's freeze, written under $RUN only (its sha256 is an input of the T6 freeze);
#                       needs T6_PRIVATE_COMMITMENT (the salted inventory commitment, computed by t6.sh)
#                               [--note "…"] [--check]
#   development: the freeze under which development numbers (dev split, tuning) are produced;
#                default FILE evals/results/docsqa/FROZEN.md
#   final:       one per published table; default FILE evals/results/docsqa/<table>/FROZEN.md
#   --check      recompute the "Inputs" section and diff it against FILE; verify the code
#                paths that affect the numbers are unchanged since the frozen commit; exit 1
#                on any difference. Writes nothing but a temporary diff.
# Every value in Inputs is computed fail-closed: a missing file, a dirty checkout or a broken
# model link aborts the freeze instead of recording an empty field.
# Env: REPO, RUN, MDA, MDA_MODEL_DIR (scripts/eval/lib.sh).
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
protocol=""; table=""; out=""; note=""; check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --protocol) protocol="$2"; shift 2 ;;
    --table) ident "$2"; table="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --note) note="$2"; shift 2 ;;
    --check) check=1; shift ;;
    *) die "unknown argument: $1" ;;
  esac
done
case "$protocol" in
  development) out="${out:-$RESULTS/FROZEN.md}" ;;
  final) [ -n "$table" ] || die "--protocol final needs --table"; out="${out:-$RESULTS/$table/FROZEN.md}" ;;
  *) die "--protocol must be development or final" ;;
esac
# The code paths that change a number. A freeze records HEAD; a check requires no diff on
# these paths between the frozen commit and HEAD, and a clean tree on them.
CODE_PATHS=(crates prompts skills scripts/eval Cargo.lock Cargo.toml rust-toolchain.toml .cargo)
cd "$REPO"
[ -z "$(git status --porcelain -- "${CODE_PATHS[@]}")" ] || { git status --short -- "${CODE_PATHS[@]}" >&2; die "uncommitted changes under ${CODE_PATHS[*]}: commit before freezing or checking"; }
head_sha="$(git rev-parse HEAD)"
if [ "$check" = 1 ]; then
  [ -f "$out" ] || die "no $out to check"
  src_sha="$(sed -n 's/^- source commit: \([0-9a-f]\{40\}\).*/\1/p' "$out" | head -1)"
  [ -n "$src_sha" ] || die "$out has no source commit line"
  git merge-base --is-ancestor "$src_sha" "$head_sha" || die "frozen commit $src_sha is not an ancestor of HEAD $head_sha"
  if ! git diff --quiet "$src_sha" "$head_sha" -- "${CODE_PATHS[@]}"; then
    git diff --stat "$src_sha" "$head_sha" -- "${CODE_PATHS[@]}" >&2
    die "code paths changed since the frozen commit $src_sha"
  fi
else
  src_sha="$head_sha"
fi
[ -x "$MDA" ] || die "no binary at $MDA (cargo build --release -p mda-cli)"

# A checkout's tracked content must be exactly the pinned commit: nothing modified, nothing
# untracked but the pin marker and the store (Codex M1 F2).
checkout_clean() { # dir
  local extra
  extra="$(git -C "$1" status --porcelain --untracked-files=all | grep -vE '^\?\? (\.mda-pinned|\.markdownattractor/)' || true)"
  [ -z "$extra" ] || { printf '%s\n' "$extra" | head -5 >&2; die "$1: tracked content differs from the pinned commit, or untracked evidence is present"; }
}
# `.git/info/exclude` rules would change what the walker indexes, and a reconstruction
# carries no `.git`: the file must hold no rules (comments and blank lines only).
exclude_rules() { # dir
  local f="$1/.git/info/exclude"
  [ -f "$f" ] || { echo none; return; }
  if grep -qvE '^\s*(#|$)' "$f"; then echo "sha256 $(sha256 "$f") (RULES PRESENT: a reconstruction cannot honour them)"; else echo none; fi
}

inputs() {
  local p dir ver f s h pinned head n ids
  echo "### Dataset"
  h="$(git -C "$DOCSQA_DATA" rev-parse HEAD)"
  echo "- PowderXu/docsqa-data commit: $h"
  for f in manifest.json corpus.jsonl questions.jsonl answers.jsonl; do
    h="$(sha256 "$DOCSQA_DATA/data/$f")"
    echo "- data/$f sha256: $h"
  done
  echo
  echo "### Repositories at their pinned commits (\`.mda-pinned\` = \`git rev-parse HEAD\`; tracked content verified unchanged; untracked: only \`.mda-pinned\` and \`.markdownattractor/\`)"
  for p in $PROJECTS; do
    dir="$(project_dir "$p")"
    pinned="$(cat "$RUN/$dir/.mda-pinned")"; head="$(git -C "$RUN/$dir" rev-parse HEAD)"
    [ "$pinned" = "$head" ] || die "$dir: .mda-pinned $pinned != HEAD $head"
    checkout_clean "$RUN/$dir"
    n="$(find "$RUN/$dir" -type f \( -name '*.md' -o -name '*.mdx' -o -name '*.markdown' \) -not -path '*/.markdownattractor/*' -not -path '*/.git/*' | wc -l | tr -d ' ')"
    h="$(sha256 "$RUN/$dir/.markdownattractor/config.toml")"
    echo "- $p (\`$dir/\`): $head · $n markdown/MDX files on disk · effective config.toml sha256 $h · .git/info/exclude rules: $(exclude_rules "$RUN/$dir")"
  done
  echo
  echo "### mda"
  ver="$("$MDA" --version)"
  echo "- version: $ver"
  echo "- source commit: $src_sha (the code paths that change a number: ${CODE_PATHS[*]}; a check requires them unchanged since this commit; the preflight builds with \`cargo build --release --locked\` and uses the executable Cargo reports, recording its sha256)"
  echo "- build profile: release"
  h="$(model_sha | shasum -a 256 | cut -c1-64)"
  echo "- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision $(cat "$MDA_MODEL_DIR"/models--Qdrant--bge-small-en-v1.5-onnx-Q/refs/main) · model files: evals/results/docsqa/model.sha (regular files and the snapshot links the loader opens, each with the sha256 of its content; sha256 of the file $h)"
  echo "- search configuration: the code defaults at the source commit (adapter fetch 30, doubled until ten distinct pages; RRF k 60; cards_fts weights heading 3 / tldr 3 / summary 1 / keywords 1 / questions_answered 2 / entities 1; sections_raw_fts heading 3 / text 1; recency off in the adapter; OR fallback on)"
  echo "- rows: lexical (raw only) · lexical (cards + raw) · hybrid (cards + raw + vectors); page-level success@5, MRR@5, nDCG@10 (\`mda_core::eval\`); every question's first ten distinct pages are archived in results.json, from which the metrics regenerate without a store"
  echo
  echo "### Cards (committed, rule 0.9)"
  for p in $PROJECTS; do
    f="$RESULTS/cards-${ver#mda }-$p.json"
    h="$(sha256 "$f")"; s="$(sha256 "$f.provenance.json")"
    echo "- $(basename "$f"): sha256 $h · $(jq -r '"\(.cards) cards for \(.sections_carded) of \(.sections) sections (complete: \(.complete)) · backend \(.by_backend | keys | join(",")) · model \(.by_model | keys | join(",")) · prompt \(.by_prompt_version | keys | join(",")) · schema \(.by_schema_version | keys | join(",")) · truncated \(.truncated) · \(.summarized_from[:10])..\(.summarized_to[:10])"' "$f.provenance.json") · provenance sha256 $s"
  done
  echo
  echo "### Prompts, rubrics, harness"
  for f in prompts/section.v2.txt prompts/section.schema.v1.json skills/search-first/SKILL.md scripts/eval/ab.sh scripts/eval/grade.sh scripts/eval/probe.sh scripts/eval/preflight.sh scripts/eval/freeze.sh scripts/eval/lib.sh scripts/eval/table.sh; do
    h="$(sha256 "$REPO/$f")"
    echo "- $f sha256: $h"
  done
  echo
  echo "### Split and question sample (seed $SEED; \`blake3(seed ‖ id)\` order; dev 30% / test 55% / holdout 15%, sealed)"
  for p in $PROJECTS; do
    s="$RESULTS/$p/split.json"; h="$(sha256 "$s")"
    ids="$(jq -r '[.questions[] | select(.split=="dev") | .id] | sort | .[]' "$s" | shasum -a 256 | cut -c1-64)"
    n="$(jq -r '[.questions[] | select(.split=="test") | .id] | sort | .[]' "$s" | shasum -a 256 | cut -c1-64)"
    echo "- $p: split.json sha256 $h · dev $(jq '[.questions[] | select(.split=="dev")] | length' "$s") ids sha256 $ids · test $(jq '[.questions[] | select(.split=="test")] | length' "$s") ids sha256 $n · holdout $(jq '[.questions[] | select(.split=="holdout")] | length' "$s") (never scored before 1.0)"
  done
  echo
  echo "### Analysis"
  local rows_path="evals/results/docsqa/<project>/results.json"
  if [ "$protocol" = final ] && [ "$table" = T2 ]; then
    echo "- table: T2 · protocol final · axis B, answer quality (plan §2.4–2.7, §4): 25 test questions per project (\`mda eval --export-questions --sample 25 --sample-seed $SEED\`, committed as ids + sha256 of the texts under evals/results/docsqa/T2/<project>/questions.jsonl), 3 runs per question and arm, arms grep · mda · qmd · graphify (graphify only where its build completed: see the arm records)"
    for p in $PROJECTS; do f="$RESULTS/T2/$p/questions.jsonl"; [ -f "$f" ] || die "no frozen sample $f"; echo "- sample $p: $(grep -c . "$f") questions · sha256 $(sha256 "$f")"; done
    echo "- answering and grading model: requested \`sonnet\` through the owner's Claude Code login; resolved id as the pilot's transcripts report it: claude-sonnet-5 (every run's row records its resolved ids under \`models\`); panel: Fable \`claude-fable-5-1\` through \`claude -p\`, Astra \`gpt-6-astra\` through \`codex exec\`"
    echo "- harness: scripts/eval/t2.sh sha256 $(sha256 "$REPO/scripts/eval/t2.sh") · scripts/eval/panel.sh sha256 $(sha256 "$REPO/scripts/eval/panel.sh") · grade rubric sha256 $(grep -m1 "^    grubric=" "$REPO/scripts/eval/t2.sh" | shasum -a 256 | cut -c1-64) · grounding rubric sha256 $(grep -m1 "^    hrubric=" "$REPO/scripts/eval/t2.sh" | shasum -a 256 | cut -c1-64) (as amended 2026-09-25, plan §2.4: factual claims about the product) · preamble sha256 $(grep -m1 '^PREAMBLE=' "$REPO/scripts/eval/t2.sh" | shasum -a 256 | cut -c1-64)"
    echo "- run parameters: max turns 12 · timeout 600 s · one retry · one job (T2_JOBS=1) · every run starts its MCP server cold (rule 0.9 as amended, plan §2.7)"
    echo "- analysis: \`mda eval --analysis grades.jsonl --manifest manifest.json\` — question median with failed runs as 0, 10,000 paired bootstrap draws, seed $SEED, gates (a) lower bound ≥ −0.25 (b) mda mean ≥ 4.0 (c) mda grounding ≥ 95%, savings per metric only where the gates pass; the adjudicated grades (panel trigger: either member differs from Sonnet by > 1 → panel mean) analysed beside the originals"
    echo "- panel: 30 answers per project re-graded by both members; 100 cards per corpus audited; every individual score kept"
    echo "- regeneration (plan §2.0b): the analysis regenerates byte-identically from the archived grades.jsonl and manifest.json; grading reruns are independent reruns, published beside the original"
  elif [ "$protocol" = final ] && [ "$table" = T5 ]; then
    local pr5 t5="$RUN/t5"
    echo "- table: T5 · protocol final · axis D, simulated historical replay (plan §4 T5, docs/plans/2026-09-m8-t5-replay.md): per project a fixed first-parent commit range of its documentation repository replayed one step per UTC day under the test-only clock \`MDA_NOW\` (ADR-0006; committer dates), stored timestamps validated against git before any answer run; 40 generated questions per project with git-computed ground truth; arms git (Read/Grep/Glob + \`git\` in Bash, no index) · mda · qmd · graphify (Prisma: the Haiku-built graph), every arm on its own copy of the final checkout with \`.git\` and \`git\` in Bash; 3 runs per question and arm; structured answers scored deterministically (set F1 ≥ 0.8; dates within one day)"
    for pr5 in ${T5_PROJECTS:-prisma tailwind-css}; do
      [ -f "$RESULTS/T5/$pr5/range.json" ] && [ -f "$RESULTS/T5/$pr5/steps.jsonl" ] && [ -f "$RESULTS/T5/$pr5/questions.jsonl" ] || die "T5/$pr5: range.json, steps.jsonl and questions.jsonl must exist before the freeze (t5.sh setup, questions)"
      echo "- $pr5: range $(jq -r '"\(.start_day) → \(.end_day) (\(.range_days) days, \(.first_parent_commits) first-parent commits, \(.steps) steps; end \(.end_sha[:12]); clone --shallow-since \(.shallow_since))"' "$RESULTS/T5/$pr5/range.json") · range.json sha256 $(sha256 "$RESULTS/T5/$pr5/range.json") · steps.jsonl sha256 $(sha256 "$RESULTS/T5/$pr5/steps.jsonl") · questions.jsonl ($(grep -c . "$RESULTS/T5/$pr5/questions.jsonl") questions: $(jq -r .type "$RESULTS/T5/$pr5/questions.jsonl" | sort | uniq -c | awk '{printf "%s %s · ", $2, $1}' | sed 's/ · $//')) sha256 $(sha256 "$RESULTS/T5/$pr5/questions.jsonl") · replay config.toml sha256 $(sha256 "$t5/$pr5/src/.markdownattractor/config.toml")"
    done
    echo "- graphify graphs: tailwind-css $(gunzip -c "$RESULTS/arms/graphs/graphify-tailwind-css.graph.json.gz" | shasum -a 256 | cut -c1-64) · prisma (graphify-haiku) $(gunzip -c "$RESULTS/arms/graphs/graphify-haiku-prisma.graph.json.gz" | shasum -a 256 | cut -c1-64)"
    echo "- answering model: requested \`sonnet\` through the owner's login (resolved ids recorded per row); summariser of the replay: the pinned config (backend claude-cli, Haiku 4.5), inline \`mda index\` under \`MDA_NOW\`; harness scripts/eval/t5.sh sha256 $(sha256 "$REPO/scripts/eval/t5.sh") · preamble sha256 $(grep -m1 '^PREAMBLE=' "$REPO/scripts/eval/t5.sh" | shasum -a 256 | cut -c1-64) · answer schema sha256 $(grep -m1 '^SCHEMA=' "$REPO/scripts/eval/t5.sh" | shasum -a 256 | cut -c1-64)"
    echo "- run parameters: max turns 12 · timeout 600 s · one retry · one job · every run starts its MCP server cold (rule 0.9)"
    echo "- analysis: \`mda eval --replay-analysis grades.jsonl --manifest manifest.json\` — question medians over 3 runs with failed runs as 0 correct, 10,000 paired bootstrap draws per (pair, metric), seed $SEED, nearest-rank percentiles; the claim \"correct with less effort at equal evidence\" only when Δcorrect's lower bound ≥ −0.05 and both effort differences' upper bounds (tool calls, source tokens) are < 0; every pair published either way"
    echo "- regeneration (plan §2.0b): the analyses and \`scripts/eval/t5.sh table\` regenerate from the archived grades.jsonl, manifest.json, replay.json and validation.json; a new replay or new answer runs are an independent rerun, published beside the original"
  elif [ "$protocol" = final ] && [ "$table" = T4 ]; then
    local t4="$RUN/t4"
    echo "- table: T4 · protocol final · axis C, update latency on Prisma (plan §4, docs/plans/2026-09-m7-t4-harness.md): 20 seeded edits (evals/results/docsqa/T4/edits.jsonl sha256 $(sha256 "$RESULTS/T4/edits.jsonl")), arms mda · qmd · graphify (the Haiku-built graph, the only completed graphify build on Prisma) · grep, each on its own copy of the pinned checkout; endpoints raw-searchable, card (mda), correct grounded answer; 1 s polling, 300 s timeout"
    echo "- mda copy: config.toml sha256 $(sha256 "$t4/mda/src/.markdownattractor/config.toml") · daemon defaults (debounce 1 s) · summariser backend claude-cli, model $(grep -E '^summarization_model' "$t4/mda/src/.markdownattractor/config.toml" | cut -d'"' -f2) (the store's hash is recorded under Runtime: every counted edit changes it)"
    echo "- qmd copy: trigger \`qmd --index t4-qmd update && qmd --index t4-qmd embed\` (the index fingerprint is recorded under Runtime)"
    echo "- graphify copy: the graphify-haiku build of Prisma (archived graph sha256 $(gunzip -c "$RESULTS/arms/graphs/graphify-haiku-prisma.graph.json.gz" | shasum -a 256 | cut -c1-64)) · trigger \`/graphify <copy> --update --no-viz\` through the login, Haiku host (the served graph's hash at freeze time is recorded under Runtime)"
    echo "- answering model: requested \`sonnet\` through the owner's login (resolved ids recorded per row's trace); harness scripts/eval/t4.sh sha256 $(sha256 "$REPO/scripts/eval/t4.sh")"
    echo "- regeneration (plan §2.0b): \`scripts/eval/t4.sh table\` renders the page table from the archived rows (evals/results/docsqa/T4/rows/); a rerun (new copies, new edits) is an independent rerun, published beside the original"
  elif [ "$protocol" = final ]; then
    rows_path="evals/results/docsqa/$table/<project>/results.json"
    echo "- table: $table · protocol final · split scored: test, once (plan §2.0; reuse of the test split by a later release is stated on the page)"
    echo "- selection: original §3 winner (the pre-tuning configuration, \`evals/results/docsqa/TUNING.md\`); post-stop diagnostics excluded from selection (plan §3, 2026-09-24 amendment)"
    echo "- intervals: per row, 95% bootstrap over the row's questions, 5,000 draws, seed $SEED (\`mda eval --interval\`); mda hybrid vs qmd full per project: within-project paired bootstrap (\`mda eval --compare\`), reported as the product target met or not"
  fi
  [ "$table" = T2 ] || [ "$table" = T4 ] || [ "$table" = T5 ] || echo "- scorer: \`mda eval --dataset docsqa\` at the source commit (the store's own rows and \`--arm-output\` for external arms, one page rule for all: sections deduplicated by path in rank order, a truncated list is scored and counted)"
  [ "$table" = T2 ] || [ "$table" = T4 ] || [ "$table" = T5 ] || echo "- regeneration (plan §2.0b): \`scripts/eval/preflight.sh\` feeds the archived page lists of \`$rows_path\` back through \`--arm-output\` and requires the same metrics and per-question results; the store replay and the clean reconstruction from the committed cards are separate checks, each compared on every row and question (latency excluded)"
  echo
  echo "### Arms"
  echo "- mda: this binary through \`mda mcp\` (\`mda_search\`, default k 5, up to 50; \`mda_open\`); the search-first rules \`skills/search-first/SKILL.md\`; store per checkout under \`.markdownattractor/\` (\`backend = claude-cli\`, \`claude_cli_policy_ack = true\`, \`embeddings = local-small\`); the adapter scores the store directly"
  echo "- grep: Claude Code's own Read, Grep and Glob over the checkout, no MCP server, no extra instructions beyond the probe preamble (\`scripts/eval/probe.sh\`)"
  # Every published arm needs a record on every project (a record may say the build did not
  # complete; a missing record is not "not built", Codex M4 F5).
  local req
  for req in qmd graphify graphify-haiku bm25-files; do for pr in $PROJECTS; do [ -f "$RESULTS/arms/$req-$pr.json" ] || die "no arm record $RESULTS/arms/$req-$pr.json: every arm is recorded on every project"; done; done
  # Competitor arms: one line per arm, listing its per-project records
  # (arms/<arm>-<project>.json: version, effective configuration, build, coverage, hashes).
  local arm pr
  # arm name = file name minus "-<project>.json" (arm names may carry dashes: graphify-haiku)
  local names=""
  for f in "$RESULTS"/arms/*.json; do
    [ -f "$f" ] || continue
    local n; n="$(basename "$f" .json)"
    for pr in $PROJECTS; do [ "${n%-$pr}" = "$n" ] || names="$names ${n%-$pr}"; done
  done
  for arm in $(printf '%s\n' $names | LC_ALL=C sort -u); do
    local line="- $arm:"
    for pr in $PROJECTS; do
      f="$RESULTS/arms/$arm-$pr.json"; [ -f "$f" ] || continue
      h="$(sha256 "$f")"
      # The live artifact the arm scores from, hashed (Codex M2 F1): qmd's index after a WAL
      # checkpoint; a graphify graph as recorded (the preflight requires the served file to match).
      local art=""
      case "$arm" in
        qmd) art=" · index fingerprint $(qmd_fingerprint "$pr")" ;;
        bm25-files) art=" · table fingerprint $(bm25_fingerprint "$pr")" ;;
        graphify|graphify-*) [ "$(jq -r '.build.completed' "$f")" != true ] || art=" · graph sha256 $(jq -r .graph.sha256 "$f")" ;;
      esac
      line="$line $(basename "$f" .json) (version $(jq -r '.version // "?"' "$f") · $(jq -r 'if .build.completed == false then "did not complete" else "coverage \(.coverage.coverage // "?")" end' "$f") · record sha256 $h$art);"
    done
    echo "$line"
  done
}

runtime() {
  echo "- frozen_at: $(date -u +%FT%TZ)"
  echo "- hardware: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m) · $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 )) GB · $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "- toolchain: $(rustc --version 2>/dev/null || echo 'rustc n/a')"
  if [ "${table:-}" = T5 ]; then
    local pr5
    for pr5 in ${T5_PROJECTS:-prisma tailwind-css}; do
      echo "- T5 $pr5 state at freeze time (the counted replay runs after the freeze and changes it; recorded, not checked): replay store $([ -f "$RUN/t5/$pr5/src/.markdownattractor/index.sqlite" ] && sha256 "$RUN/t5/$pr5/src/.markdownattractor/index.sqlite" || echo none) · clone HEAD $(git -C "$RUN/t5/$pr5/src" rev-parse HEAD 2>/dev/null || echo none) · replayed steps $(find "$RUN/t5/$pr5/replay" -maxdepth 1 -name '[0-9]*.json' 2>/dev/null | wc -l | tr -d ' ')"
    done
  fi
  if [ "${table:-}" = T4 ]; then
    echo "- T4 state at freeze time (mutable during the run, so recorded here and not checked): mda copy store sha256 $(sha256 "$RUN/t4/mda/src/.markdownattractor/index.sqlite") · qmd t4-qmd index fingerprint $(qmd_fingerprint t4-qmd) · served graph.json sha256 $(sha256 "$RUN/t4/graphify/graph.json")"
  fi
  echo "- claude: $(claude --version 2>/dev/null || echo 'n/a') (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)"
}

# T6, our own corpora (docs/plans/2026-10-m9-t6-page-readme.md, amendments F13–F15): the public
# freeze names the two public corpora as `git archive` subtrees of this repository with their tree
# hashes, the committed cards and question files, the harness, rubrics, prompts, models, arms and
# run parameters, plus the private freeze's hash; the private freeze (under $RUN) names the private
# corpus by its salted commitment and counts only. Nothing of DocsQA is read.
# Every value is computed in its own checked assignment (review F14): a missing file aborts the
# freeze instead of printing an empty field.
# `val` runs inside command substitutions, where `die` ends only the subshell (review 2 F1): every
# failure is also recorded in $VAL_FAILED, which the caller checks before writing anything.
VAL_FAILED=""   # set once $tmp exists (below), so a failure before vals_ok leaves nothing behind
val() { local v; if ! v="$("$@" 2>/dev/null)"; then printf 'cannot compute: %s\n' "$*" >> "$VAL_FAILED"; die "freeze: cannot compute: $*"; fi; if [ -z "$v" ] || [ "$v" = null ]; then printf 'empty or null value from: %s\n' "$*" >> "$VAL_FAILED"; die "freeze: empty value from: $*"; fi; printf '%s' "$v"; }
vals_ok() { if [ -s "$VAL_FAILED" ]; then cat "$VAL_FAILED" >&2; die "freeze: a frozen value could not be computed (above): nothing written"; fi; }
t6_common() {
  local f h ver bin own cal
  own="$REPO/evals/results/own"; cal="$own/T6/calibration.json"
  echo "### mda"
  ver="$(val "$MDA" --version)"; bin="$(val sha256 "$MDA")"
  echo "- version: $ver · binary sha256 $bin (the preflight rebuilds with \`cargo build --release --locked\` from the source commit and requires this hash)"
  echo "- source commit: $src_sha (the code paths that change a number: ${CODE_PATHS[*]}; a check requires them unchanged since this commit)"
  echo "- analysis: \`mda eval --analysis\` (crates/mda-core/src/eval/analysis.rs: question medians over runs with failed runs as 0, 10,000-draw paired bootstrap, seed $SEED, gates lower bound ≥ −0.25 · mda mean ≥ 4.0 · grounding ≥ 0.95)"
  h="$(model_sha | shasum -a 256 | cut -c1-64)"; [ "${#h}" = 64 ] || die "freeze: model files could not be hashed"
  [ "$(model_sha | grep -c .)" -gt 0 ] || die "freeze: empty model inventory"
  echo "- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision $(val cat "$MDA_MODEL_DIR"/models--Qdrant--bge-small-en-v1.5-onnx-Q/refs/main) · model files sha256 $h (evals/results/docsqa/model.sha)"
  echo "- store configuration template: evals/results/own/T6/config.toml sha256 $(val sha256 "$own/T6/config.toml") (backend claude-cli, summariser claude-haiku-4-5, embeddings local-small; the ignore rules inside it)"
  echo
  echo "### Calibration (the development pilot; required before the freeze, review F4/F11/F22)"
  [ -f "$cal" ] || die "freeze: no $cal (run t6.sh pilot for every corpus, then t6.sh calibration)"
  echo "- calibration.json sha256 $(val sha256 "$cal") · resolved model ids: $(val jq -r '.resolved_models | to_entries | map("\(.key) \(.value | join("/"))") | join(" · ")' "$cal") · declared resolution rule: $(val jq -r .resolution_rule "$cal")"
  echo
  echo "### Prompts, rubrics, harness"
  for f in prompts/section.v2.txt prompts/section.schema.v1.json skills/search-first/SKILL.md scripts/eval/t2.sh scripts/eval/t6.sh scripts/eval/t6_resolve.py scripts/eval/t6_private_export.py scripts/eval/panel.sh scripts/eval/probe.sh scripts/eval/freeze.sh scripts/eval/lib.sh scripts/eval/arms/qmd.sh scripts/eval/arms/graphify.sh; do
    echo "- $f sha256: $(val sha256 "$REPO/$f")"
  done
  echo "- rubrics: the T6 adaptation (T2_RUBRIC=t6 in t2.sh and panel.sh: required facts + evidence section; factual claims about the corpus's subject matter); the question writer (t6.sh candidates) and the panel's two-call question rubric (panel.sh questions) — all inside the hashed files above"
  echo
  echo "### Run and analysis parameters"
  echo "- answering model requested: sonnet · grader: sonnet · panel: claude-fable-5-1 through the owner's Claude Code login, gpt-6-astra through the owner's Codex login · question writer: sonnet (resolved ids per the calibration record above and the run rows)"
  echo "- 25 counted questions per corpus (single-section lookup; synthetic, panel-validated, one per page; a corpus that reached fewer publishes with its count) and up to 10 disjoint pilot questions (never counted), 3 runs per question and arm, max 12 turns, 600 s per run, one declared retry, every MCP server cold (rule 0.9 as amended); the arm set certified by the preflight"
  echo "- arms: grep (Read/Grep/Glob) · mda (\`mda mcp\` + search-first rules) · qmd (its MCP server on index t6-<corpus>, its skill) · graphify (its MCP server on a Sonnet-built graph of a copy; where the build did not complete the arm is absent and the record says so)"
  echo "- arm versions: qmd $(val qmd --version) · graphify $(val graphify --version) · claude $(val claude --version) · codex $(val codex --version)"
}
# the live artifacts an arm serves from (review F8): the graph, graphify's skill and hook settings
# as installed in the serving copy, qmd's collection file
t6_arm_lines() { # corpus results-dir run-dir
  local c="$1" R="$2" P="$3" f
  f="$R/arms/qmd-$c.json"; [ -f "$f" ] || die "no arm record $f"
  echo "- qmd-$c: record sha256 $(val sha256 "$f") · index t6-$c fingerprint $(val qmd_fingerprint "t6-$c") · collection file sha256 $(val sha256 "$HOME/.config/qmd/t6-$c.yml")"
  f="$R/arms/graphify-$c.json"; [ -f "$f" ] || die "no arm record $f"
  if [ "$(jq -r .build.completed "$f")" = true ]; then
    echo "- graphify-$c: record sha256 $(val sha256 "$f") · served graph sha256 $(val sha256 "$P/graphify/graph.json") · skill sha256 $(val sha256 "$P/graphify/src/.claude/skills/graphify/SKILL.md") · hook settings sha256 $(val sha256 "$P/graphify/src/.claude/settings.json")"
  else echo "- graphify-$c: record sha256 $(val sha256 "$f") · did not complete (the arm is absent)"; fi
}
inputs_t6() {
  local c sub tree src n ver cards f own srcf
  own="$REPO/evals/results/own"; srcf="$own/T6/sources.json"; [ -f "$srcf" ] || die "no $srcf (t6.sh setup)"
  src="$(val jq -r .source_commit "$srcf")"
  echo "### Corpora (public: \`git archive <commit>:<subtree>\` of this repository; a reader rebuilds them from the published commit)"
  for c in golden docs; do
    sub="$(val jq -r --arg c "$c" '.corpora[$c].subtree' "$srcf")"; tree="$(val jq -r --arg c "$c" '.corpora[$c].tree' "$srcf")"
    [ "$(git -C "$REPO" rev-parse "$src:$sub")" = "$tree" ] || die "$c: the subtree $sub at $src does not have tree $tree"
    n="$(git -C "$REPO" ls-tree -r --name-only "$src" -- "$sub" | grep -cE '\.(md|mdx|markdown)$' || true)"
    echo "- $c: $src:$sub · tree $tree · $n markdown files · page inventory sha256 $(val sha256 "$own/T6/$c/pages.txt") (committed as T6/$c/pages.txt) · store sha256 $(val sha256 "$RUN/t6/$c/src/.markdownattractor/index.sqlite") · effective config.toml sha256 $(val sha256 "$RUN/t6/$c/src/.markdownattractor/config.toml")"
  done
  # the private corpus is named by the PUBLISHED digest of its freeze (review F13): public
  # verification needs nothing from the owner's machine
  f="$own/T6/private/FROZEN.sha256"
  if [ -f "$f" ]; then echo "- private: the owner's trading-research repository, supporting evidence only; its freeze (kept under \$RUN) has sha256 $(val cut -c1-64 "$f") (committed as T6/private/FROZEN.sha256); exempt from independent reproduction"; else echo "- private: not frozen"; fi
  echo
  echo "### Cards (committed, rule 0.9)"
  ver="$(val "$MDA" --version)"
  for c in golden docs; do
    cards="$own/cards-${ver#mda }-$c.json"; [ -f "$cards" ] && [ -f "$cards.provenance.json" ] || die "no cards export $cards"
    echo "- $(basename "$cards"): sha256 $(val sha256 "$cards") · $(val jq -r '"\(.cards) cards for \(.sections_carded) of \(.sections) sections (complete: \(.complete)) · model \(.by_model | keys | join(",")) · prompt \(.by_prompt_version | keys | join(","))"' "$cards.provenance.json") · provenance sha256 $(val sha256 "$cards.provenance.json")"
  done
  echo
  echo "### Questions (generated from the corpus, panel-validated; texts committed for the public corpora)"
  for c in golden docs; do
    f="$own/T6/$c/questions.jsonl"; [ -f "$f" ] || die "no $f"
    echo "- $c: questions.jsonl sha256 $(val sha256 "$f") ($(grep -c . "$f" || true) counted) · pilot sha256 $(val sha256 "$own/T6/$c/questions.pilot.jsonl") ($(grep -c . "$own/T6/$c/questions.pilot.jsonl" || true) pilot) · candidates sha256 $(val sha256 "$own/T6/$c/questions.candidates.jsonl") · rejected sections sha256 $(val sha256 "$own/T6/$c/questions.rejected.jsonl") · summary sha256 $(val sha256 "$own/T6/$c/candidates.summary.json")"
  done
  f="$own/T6/private/questions.jsonl"; [ -f "$f" ] && echo "- private: questions.jsonl (ids and digests) sha256 $(val sha256 "$f") ($(grep -c . "$f" || true) counted)" || echo "- private: no committed digests"
  echo
  t6_common
  echo
  echo "### Arms (records under evals/results/own/arms/; the live artifacts they serve from)"
  for c in golden docs; do t6_arm_lines "$c" "$own" "$RUN/t6/$c"; done
}
inputs_t6_private() {
  local f own
  own="$REPO/evals/results/own"
  [ -n "${T6_PRIVATE_COMMITMENT:-}" ] || die "T6_PRIVATE_COMMITMENT is required for the private freeze (t6.sh freeze computes it)"
  echo "### Corpus (private; nothing content-bearing leaves this machine)"
  echo "- commitment: sha256(salt ‖ rel_path NUL sha256(file bytes) LF, in byte order over the inventory) = $T6_PRIVATE_COMMITMENT · salt under \$RUN/t6/private/salt"
  echo "- inventory: $(grep -c . "$RUN/t6/private/pages.txt" || true) markdown files · inventory sha256 $(val sha256 "$RUN/t6/private/pages.txt") · store sha256 $(val sha256 "$RUN/t6/private/src/.markdownattractor/index.sqlite") · effective config.toml sha256 $(val sha256 "$RUN/t6/private/src/.markdownattractor/config.toml") · store sections $(val sqlite3 "$RUN/t6/private/src/.markdownattractor/index.sqlite" 'select count(*) from sections')"
  echo
  echo "### Cards (not committed)"
  f="$RUN/t6/private/results/cards-$("$MDA" --version | sed 's/^mda //')-private.json"; [ -f "$f" ] || die "no private cards export $f"
  echo "- cards sha256 $(val sha256 "$f") · $(val jq -r '"\(.cards) cards for \(.sections_carded) of \(.sections) sections (complete: \(.complete)) · model \(.by_model | keys | join(","))"' "$f.provenance.json")"
  echo
  echo "### Questions (texts under \$RUN; the committed file carries ids and digests)"
  echo "- questions.full.jsonl sha256 $(val sha256 "$RUN/t6/private/questions.full.jsonl") ($(grep -c . "$RUN/t6/private/questions.full.jsonl" || true) counted) · pilot sha256 $(val sha256 "$RUN/t6/private/questions.pilot.jsonl") · candidates sha256 $(val sha256 "$RUN/t6/private/questions.candidates.jsonl") · committed digests sha256 $(val sha256 "$own/T6/private/questions.jsonl")"
  echo
  t6_common
  echo
  echo "### Arms (records under \$RUN/t6/private/results/arms/; the live artifacts they serve from)"
  t6_arm_lines private "$RUN/t6/private/results" "$RUN/t6/private"
}

tmp="$(mktemp -d -t mda-freeze.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
VAL_FAILED="$tmp/failed-values"; : > "$VAL_FAILED"
# A plain statement, so errexit applies inside `inputs` and a failed value aborts the freeze.
case "${table:-}" in
  T6) inputs_t6 > "$tmp/inputs"; vals_ok ;;
  T6-private) inputs_t6_private > "$tmp/inputs"; vals_ok ;;
  *) inputs > "$tmp/inputs" ;;
esac
# no frozen line may carry an empty field (a `· ` or `: ` at the end, or `sha256 ` followed by nothing)
case "${table:-}" in T6|T6-private) ! grep -nE '(sha256|fingerprint|commitment|=|:) *( ·|$)' "$tmp/inputs" >/dev/null || { grep -nE '(sha256|fingerprint|commitment|=|:) *( ·|$)' "$tmp/inputs" >&2; die "freeze: an empty field in the Inputs"; } ;; esac
if [ "$check" = 1 ]; then
  awk '/^## Inputs/{f=1; next} /^## Runtime/{f=0} f' "$out" | sed '/^$/d' > "$tmp/frozen"
  sed '/^$/d' "$tmp/inputs" > "$tmp/now"
  if diff "$tmp/frozen" "$tmp/now" > "$tmp/diff"; then
    echo "frozen inputs match $out (source commit $src_sha, HEAD $head_sha)"
  else
    cat "$tmp/diff" >&2; die "FROZEN inputs differ from $out"
  fi
else
  safe_target "$out"
  {
    case "${table:-}" in T6|T6-private) echo "# FROZEN — our own corpora (T6), protocol: $protocol, table $table" ;; *) echo "# FROZEN — DocsQA-Repo, protocol: $protocol${table:+, table $table}" ;; esac
    echo
    echo "Written by \`scripts/eval/freeze.sh\` (execution plan §2.0). The **Inputs** section is compared byte for byte by \`scripts/eval/preflight.sh\` before any run; the **Runtime** section is recorded for the reader and a difference there is reported, never a failure. Numbers produced under this freeze are labelled *$protocol*.${note:+ $note}"
    echo
    echo "## Inputs"
    echo
    cat "$tmp/inputs"
    echo
    echo "## Runtime"
    echo
    runtime
  } > "$out"
  echo "wrote $out (source commit $src_sha)"
fi
