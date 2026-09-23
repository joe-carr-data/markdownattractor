#!/usr/bin/env bash
# Write (or check) FROZEN.md, the record of every input a benchmark number depends on
# (execution plan §2.0, strategy rule 0.1). Every run's preflight calls `freeze.sh --check`
# so a run against changed inputs refuses to start.
#
# Usage: scripts/eval/freeze.sh --protocol development|final [--table T1] [--out FILE]
#                               [--note "…"] [--check]
#   development: the freeze under which development numbers (dev split, tuning) are produced;
#                default FILE evals/results/docsqa/FROZEN.md
#   final:       one per published table; default FILE evals/results/docsqa/<table>/FROZEN.md
#   --check      recompute the "Inputs" section and diff it against FILE; verify the code
#                paths that affect the numbers are unchanged since the frozen commit; exit 1
#                on any difference. Nothing is written.
# Env: REPO, RUN, MDA, MDA_MODEL_DIR (scripts/eval/lib.sh).
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
protocol=""; table=""; out=""; note=""; check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --protocol) protocol="$2"; shift 2 ;;
    --table) table="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --note) note="$2"; shift 2 ;;
    --check) check=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$protocol" in
  development) out="${out:-$RESULTS/FROZEN.md}" ;;
  final) [ -n "$table" ] || { echo "--protocol final needs --table" >&2; exit 2; }; out="${out:-$RESULTS/$table/FROZEN.md}" ;;
  *) echo "--protocol must be development or final" >&2; exit 2 ;;
esac
# The code paths that change a number. A freeze records HEAD; a check requires no diff on
# these paths between the frozen commit and HEAD, and a clean tree on them.
CODE_PATHS=(crates prompts skills scripts/eval Cargo.lock Cargo.toml rust-toolchain.toml)
cd "$REPO"
[ -z "$(git status --porcelain -- "${CODE_PATHS[@]}")" ] || { echo "uncommitted changes under ${CODE_PATHS[*]}: commit before freezing or checking" >&2; git status --short -- "${CODE_PATHS[@]}" >&2; exit 1; }
head_sha="$(git rev-parse HEAD)"
if [ "$check" = 1 ]; then
  [ -f "$out" ] || { echo "no $out to check" >&2; exit 1; }
  src_sha="$(sed -n 's/^- source commit: \([0-9a-f]\{40\}\).*/\1/p' "$out" | head -1)"
  [ -n "$src_sha" ] || { echo "$out has no source commit line" >&2; exit 1; }
  git merge-base --is-ancestor "$src_sha" "$head_sha" || { echo "frozen commit $src_sha is not an ancestor of HEAD $head_sha" >&2; exit 1; }
  if ! git diff --quiet "$src_sha" "$head_sha" -- "${CODE_PATHS[@]}"; then
    echo "code paths changed since the frozen commit $src_sha:" >&2
    git diff --stat "$src_sha" "$head_sha" -- "${CODE_PATHS[@]}" >&2
    exit 1
  fi
else
  src_sha="$head_sha"
fi
[ -x "$MDA" ] || { echo "no binary at $MDA (cargo build --release -p mda-cli)" >&2; exit 1; }

inputs() {
  local p dir ver
  echo "### Dataset"
  echo "- PowderXu/docsqa-data commit: $(git -C "$RUN/docsqa-data" rev-parse HEAD)"
  for f in manifest.json corpus.jsonl questions.jsonl answers.jsonl; do
    echo "- data/$f sha256: $(sha256 "$RUN/docsqa-data/data/$f")"
  done
  echo
  echo "### Repositories at their pinned commits (\`.mda-pinned\` = \`git rev-parse HEAD\`)"
  for p in $PROJECTS; do
    dir="$(project_dir "$p")"
    local pinned head
    pinned="$(cat "$RUN/$dir/.mda-pinned")"; head="$(git -C "$RUN/$dir" rev-parse HEAD)"
    [ "$pinned" = "$head" ] || { echo "$dir: .mda-pinned $pinned != HEAD $head" >&2; return 1; }
    echo "- $p (\`$dir/\`): $head · $(find "$RUN/$dir" -type f \( -name '*.md' -o -name '*.mdx' -o -name '*.markdown' \) -not -path '*/.markdownattractor/*' | wc -l | tr -d ' ') markdown/MDX files on disk"
  done
  echo
  echo "### mda"
  ver="$("$MDA" --version)"
  echo "- version: $ver"
  echo "- source commit: $src_sha (the code paths that change a number: ${CODE_PATHS[*]}; a check requires them unchanged since this commit)"
  echo "- build profile: release"
  echo "- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision $(cat "$MDA_MODEL_DIR"/models--Qdrant--bge-small-en-v1.5-onnx-Q/refs/main) · model files: evals/results/docsqa/model.sha (sha256 $(model_sha | shasum -a 256 | cut -c1-64))"
  echo "- search configuration: the code defaults at the source commit (adapter fetch 30, doubled until ten distinct pages; RRF k 60; cards_fts weights heading 3 / tldr 3 / summary 1 / keywords 1 / questions_answered 2 / entities 1; sections_raw_fts heading 3 / text 1; recency off in the adapter; OR fallback on)"
  echo "- rows: lexical (raw only) · lexical (cards + raw) · hybrid (cards + raw + vectors); page-level success@5, MRR@5, nDCG@10 (\`mda_core::eval\`)"
  echo
  echo "### Cards (committed, rule 0.9)"
  for p in $PROJECTS; do
    local f="$RESULTS/cards-${ver#mda }-$p.json"
    [ -f "$f" ] || { echo "- $p: NO CARDS FILE ($f)"; continue; }
    echo "- $(basename "$f"): sha256 $(sha256 "$f") · $(jq -r '"\(.cards) cards for \(.sections_carded) of \(.sections) sections (complete: \(.complete)) · backend \(.by_backend | keys | join(",")) · model \(.by_model | keys | join(",")) · prompt \(.by_prompt_version | keys | join(",")) · schema \(.by_schema_version | keys | join(",")) · truncated \(.truncated) · \(.summarized_from[:10])..\(.summarized_to[:10])"' "$f.provenance.json") · provenance sha256 $(sha256 "$f.provenance.json")"
  done
  echo
  echo "### Prompts, rubrics, harness"
  for f in prompts/section.v2.txt prompts/section.schema.v1.json skills/search-first/SKILL.md scripts/eval/ab.sh scripts/eval/grade.sh scripts/eval/probe.sh scripts/eval/preflight.sh scripts/eval/freeze.sh scripts/eval/lib.sh; do
    [ -f "$REPO/$f" ] && echo "- $f sha256: $(sha256 "$REPO/$f")"
  done
  echo
  echo "### Split and question sample (seed $SEED; \`blake3(seed ‖ id)\` order; dev 30% / test 55% / holdout 15%, sealed)"
  for p in $PROJECTS; do
    local s="$RESULTS/$p/split.json"
    echo "- $p: split.json sha256 $(sha256 "$s") · dev $(jq '[.questions[] | select(.split=="dev")] | length' "$s") ids sha256 $(jq -r '[.questions[] | select(.split=="dev") | .id] | sort | .[]' "$s" | shasum -a 256 | cut -c1-64) · test $(jq '[.questions[] | select(.split=="test")] | length' "$s") ids sha256 $(jq -r '[.questions[] | select(.split=="test") | .id] | sort | .[]' "$s" | shasum -a 256 | cut -c1-64) · holdout $(jq '[.questions[] | select(.split=="holdout")] | length' "$s") (never scored before 1.0)"
  done
  echo
  echo "### Analysis"
  echo "- scorer: \`mda eval --dataset docsqa\` at the source commit (the store's own rows and \`--arm-output\` for external arms, one page rule for all: sections deduplicated by path in rank order, a truncated list is scored and counted)"
  echo "- regeneration: \`scripts/eval/preflight.sh\` re-scores the committed store and a clean reconstruction from the committed cards and compares every row and question with \`evals/results/docsqa/<project>/results.json\` byte for byte (latency excluded)"
  echo
  echo "### Arms"
  echo "- mda: this binary through \`mda mcp\` (\`mda_search\`, default k 5, up to 50; \`mda_open\`); the search-first rules \`skills/search-first/SKILL.md\`; store per checkout under \`.markdownattractor/\` (\`backend = claude-cli\`, \`claude_cli_policy_ack = true\`, \`embeddings = local-small\`); the adapter scores the store directly"
  echo "- grep: Claude Code's own Read, Grep and Glob over the checkout, no MCP server, no extra instructions beyond the probe preamble (\`scripts/eval/probe.sh\`)"
  for a in "$RESULTS"/arms/*.json; do
    [ -f "$a" ] || continue
    echo "- $(basename "$a" .json): $(jq -c . "$a") · sha256 $(sha256 "$a")"
  done
}

runtime() {
  echo "- frozen_at: $(date -u +%FT%TZ)"
  echo "- hardware: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m) · $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 )) GB · $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "- toolchain: $(rustc --version 2>/dev/null || echo 'rustc n/a')"
  echo "- claude: $(claude --version 2>/dev/null || echo 'n/a') (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)"
}

body="$(inputs)" || { echo "freeze: inputs could not be computed" >&2; exit 1; }
if [ "$check" = 1 ]; then
  frozen="$(awk '/^## Inputs/{f=1; next} /^## Runtime/{f=0} f' "$out")"
  if diff <(printf '%s\n' "$frozen" | sed '/^$/d') <(printf '%s\n' "$body" | sed '/^$/d') > /tmp/frozen.diff 2>&1; then
    echo "frozen inputs match $out (source commit $src_sha, HEAD $head_sha)"
  else
    echo "FROZEN inputs differ from $out:" >&2; cat /tmp/frozen.diff >&2; exit 1
  fi
else
  mkdir -p "$(dirname "$out")"
  [ ! -L "$out" ] || { echo "$out is a symlink" >&2; exit 1; }
  {
    echo "# FROZEN — DocsQA-Repo, protocol: $protocol${table:+, table $table}"
    echo
    echo "Written by \`scripts/eval/freeze.sh\` (execution plan §2.0). The **Inputs** section is compared byte for byte by \`scripts/eval/preflight.sh\` before any run; the **Runtime** section is recorded for the reader and a difference there is reported, never a failure. Numbers produced under this freeze are labelled *$protocol*${note:+. $note}."
    echo
    echo "## Inputs"
    echo
    printf '%s\n' "$body"
    echo
    echo "## Runtime"
    echo
    runtime
  } > "$out"
  echo "wrote $out (source commit $src_sha)"
fi
