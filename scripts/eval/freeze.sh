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
  h="$(git -C "$RUN/docsqa-data" rev-parse HEAD)"
  echo "- PowderXu/docsqa-data commit: $h"
  for f in manifest.json corpus.jsonl questions.jsonl answers.jsonl; do
    h="$(sha256 "$RUN/docsqa-data/data/$f")"
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
  echo "- scorer: \`mda eval --dataset docsqa\` at the source commit (the store's own rows and \`--arm-output\` for external arms, one page rule for all: sections deduplicated by path in rank order, a truncated list is scored and counted)"
  echo "- regeneration (plan §2.0b): \`scripts/eval/preflight.sh\` feeds the archived page lists of \`evals/results/docsqa/<project>/results.json\` back through \`--arm-output\` and requires the same metrics and per-question results; the store replay and the clean reconstruction from the committed cards are separate checks, each compared on every row and question (latency excluded)"
  echo
  echo "### Arms"
  echo "- mda: this binary through \`mda mcp\` (\`mda_search\`, default k 5, up to 50; \`mda_open\`); the search-first rules \`skills/search-first/SKILL.md\`; store per checkout under \`.markdownattractor/\` (\`backend = claude-cli\`, \`claude_cli_policy_ack = true\`, \`embeddings = local-small\`); the adapter scores the store directly"
  echo "- grep: Claude Code's own Read, Grep and Glob over the checkout, no MCP server, no extra instructions beyond the probe preamble (\`scripts/eval/probe.sh\`)"
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
      line="$line $(basename "$f" .json) (version $(jq -r '.version // "?"' "$f") · $(jq -r 'if .build.completed == false then "did not complete" else "coverage \(.coverage.coverage // "?")" end' "$f") · sha256 $h);"
    done
    echo "$line"
  done
}

runtime() {
  echo "- frozen_at: $(date -u +%FT%TZ)"
  echo "- hardware: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m) · $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 )) GB · $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "- toolchain: $(rustc --version 2>/dev/null || echo 'rustc n/a')"
  echo "- claude: $(claude --version 2>/dev/null || echo 'n/a') (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)"
}

tmp="$(mktemp -d -t mda-freeze.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
# A plain statement, so errexit applies inside `inputs` and a failed value aborts the freeze.
inputs > "$tmp/inputs"
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
    echo "# FROZEN — DocsQA-Repo, protocol: $protocol${table:+, table $table}"
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
