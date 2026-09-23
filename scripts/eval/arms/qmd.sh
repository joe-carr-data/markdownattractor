#!/usr/bin/env bash
# The qmd arm (execution plan §1.1, §2.1): build one qmd index per project from the pinned
# checkout, record its effective configuration and models, and drive the dataset's questions
# through `qmd query` (full: expansion + vectors + reranker), `qmd query --no-rerank`
# (ablation) or `qmd search` (BM25 only) into the rows `mda eval --arm-output` scores:
# {"question_id", "paths": [repository-relative, best first], "truncated"} plus the request
# actually sent, so a reader can replay one question by hand.
#
# Usage:
#   scripts/eval/arms/qmd.sh build <project>                      # collection add + update + embed; times, coverage, arm.json
#   scripts/eval/arms/qmd.sh drive <project> <full|no-rerank|bm25> <out.jsonl> [split=dev]
#   scripts/eval/arms/qmd.sh status <project>
#   scripts/eval/arms/qmd.sh record <project> [add_s update_s embed_s]   # rewrite the arm record without rebuilding
# One index per project (`qmd --index <project>`, its collection registered in
# ~/.config/qmd/<project>.yml and its data in ~/.cache/qmd/<project>.sqlite), so a query can
# never see another project's collection. The collection mask includes .mdx and .markdown
# (qmd's default is **/*.md only; three of the four corpora are MDX); this is qmd's own
# documented option and is published with the table. Every query runs from the checkout
# directory with --full-path, so paths come back ./-prefixed and repository-relative.
set -euo pipefail
# shellcheck source=../lib.sh
. "$(dirname "$0")/../lib.sh"
cmd="${1:?build|drive|status}"; project="${2:?project}"; shift 2
ident "$project"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"
[ -d "$corpus" ] || die "no checkout at $corpus"
MASK='**/*.{md,mdx,markdown}'
ARMS="$RESULTS/arms"; mkdir -p "$ARMS"
qmd_version() { qmd --version 2>/dev/null | head -1; }
models_sha() { ( cd "$HOME/.cache/qmd/models" 2>/dev/null && find . -type f | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r f; do printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -c1-64)" "$f"; done ); }

# The arm record (evals/results/docsqa/arms/qmd-<project>.json): version, install, effective
# configuration, build times, coverage against the dataset corpus (which of its pages the
# index lists), the model files' hashes, the status text.
record_arm() { # add_s update_s embed_s
  local status files on_disk total present missing listed p
  status="$(qmd --index "$project" status 2>&1)"
  listed="$(qmd --index "$project" ls "$project" 2>/dev/null | sed -n 's|.*qmd://'"$project"'/||p')"
  files="$(printf '%s\n' "$listed" | grep -c . || true)"
  on_disk="$(find "$corpus" -type f \( -name '*.md' -o -name '*.mdx' -o -name '*.markdown' \) -not -path '*/.markdownattractor/*' -not -path '*/.git/*' | wc -l | tr -d ' ')"
  total=0; present=0; missing='[]'
  while IFS= read -r p; do
    total=$((total + 1))
    if grep -qxF "$p" <<<"$listed"; then present=$((present + 1)); else missing="$(jq -c --arg p "$p" '. + [$p]' <<<"$missing")"; fi
  done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
  jq -n --arg arm qmd --arg project "$project" --arg version "$(qmd_version)" --arg node "$(node --version)" --arg mask "$MASK" \
    --argjson add_s "$1" --argjson update_s "$2" --argjson embed_s "$3" \
    --arg status "$status" --argjson files "${files:-0}" --argjson on_disk "$on_disk" --argjson total "$total" --argjson present "$present" --argjson missing "$missing" \
    --arg models "$(models_sha)" \
    '{arm: $arm, project: $project, version: $version, node: $node,
      install: "npm i -g @tobilu/qmd@2.8.3", index: ("~/.cache/qmd/" + $project + ".sqlite"), collection: ("~/.config/qmd/" + $project + ".yml"),
      config: {mask: $mask, candidate_limit: 40, results_default: 5, rerank: "Qwen3-Reranker-0.6B (default on)", expansion: "qmd-query-expansion-1.7B (default on)", embed: "EmbeddingGemma-300M", gpu: "metal (default)",
               mcp_request: {tool: "query", arguments: {query: "<question text>", limit: "<20, then 40 when fewer than ten distinct pages came back>", rerank: "<true for the full row, false for the no-rerank row>"}}},
      build: {collection_add_s: $add_s, update_s: $update_s, embed_s: $embed_s, total_s: ($add_s + $update_s + $embed_s)},
      coverage: {files_indexed: $files, markdown_files_on_disk: $on_disk, corpus_pages: $total, corpus_pages_indexed: $present, coverage: (if $total == 0 then 0 else $present / $total end), missing_pages: $missing},
      models_sha256: ($models | split("\n") | map(select(. != ""))), status: $status}' > "$ARMS/qmd-$project.json"
  echo "qmd $project: $present of $total corpus pages indexed ($files files) · $ARMS/qmd-$project.json"
}

case "$cmd" in
  record) record_arm "${1:-0}" "${2:-0}" "${3:-0}" ;;
  build)
    [ ! -f "$HOME/.config/qmd/$project.yml" ] || die "index $project already registered (~/.config/qmd/$project.yml): remove it first, a build is done once per freeze"
    log="$RUN/qmd-build-$project.log"; : > "$log"
    t0=$(date +%s)
    ( cd "$corpus" && qmd --index "$project" collection add "$corpus" --name "$project" --mask "$MASK" ) >>"$log" 2>&1
    t1=$(date +%s)
    ( cd "$corpus" && qmd --index "$project" update ) >>"$log" 2>&1
    t2=$(date +%s)
    ( cd "$corpus" && qmd --index "$project" embed --timeout 0 ) >>"$log" 2>&1
    t3=$(date +%s)
    record_arm "$((t1 - t0))" "$((t2 - t1))" "$((t3 - t2))"
    echo "built qmd index $project: add $((t1 - t0)) s · update $((t2 - t1)) s · embed $((t3 - t2)) s"
    ;;
  drive)
    mode="${1:?full|no-rerank|bm25}"; out="${2:?out.jsonl}"; split="${3:-dev}"
    safe_target "$out"; [ ! -e "$out" ] || die "$out exists: one directory per attempt, nothing is overwritten"
    client="${MCP_TIME:-$REPO/target/release/examples/mcp_time}"
    [ -x "$client" ] || die "no MCP client at $client: cargo build --release --example mcp_time"
    # The MCP `query` tool is the interface the table scores (plan §2.1): full = qmd's default
    # (expansion, vectors, reranker); no-rerank = the same request with rerank:false (the
    # configuration diff is that one field); bm25 = a lex-only sub-query with rerank:false
    # (qmd's BM25 engine through the same tool; qmd's lex form ANDs every term and has no OR
    # fallback, which is its documented behaviour). Two passes: limit 20, then limit 40 for
    # the questions that came back with fewer than ten distinct pages and a full page of
    # hits; a question still short of ten pages after a full 40 is `truncated` (the
    # reranker's candidate limit, qmd's default, caps what a query can return).
    case "$mode" in
      full) template='{"query":"{query}","limit":LIMIT,"rerank":true}' ;;
      no-rerank) template='{"query":"{query}","limit":LIMIT,"rerank":false}' ;;
      bm25) template='{"searches":[{"type":"lex","query":"{query}"}],"limit":LIMIT,"rerank":false}' ;;
      *) die "mode must be full, no-rerank or bm25" ;;
    esac
    work="$out.work"; mkdir -p "$work"
    [ "$split" != holdout ] || die "the holdout is sealed (rule 0.2)"
    unset_provider_keys
    # Question ids: the frozen split (the scorer applies eligibility; an ineligible id costs one unused query).
    jq -r --arg s "$split" '.questions[] | select(.split == $s) | .id' "$RESULTS/$project/split.json" | while IFS= read -r qid; do
      jq -nc --arg id "$qid" --arg q "$(question_text "$project" "$qid" | tr '\n\r\t' '   ' | sed 's/  */ /g')" '{id: $id, q: $q}'
    done > "$work/pass1.jsonl"
    [ -s "$work/pass1.jsonl" ] || die "no questions in split $split for $project"
    pages_of() { # dump line -> distinct repository paths in rank order
      jq -c --arg pre "$project/" '[.result.structuredContent.results[]? | .file | sub("^" + $pre; "")] | reduce .[] as $p ([]; if index([$p]) then . else . + [$p] end)'
    }
    hits_of() { jq -r '.result.structuredContent.results | length' ; }
    run_pass() { # queries.jsonl limit dump.jsonl times.jsonl
      local t="${template//LIMIT/$2}"
      ( cd "$corpus" && MCP_TIME_DUMP="$3" "$client" query "$t" "$1" -- qmd --index "$project" mcp ) > "$4" 2>>"$out.err"
      jq -n --arg tool query --argjson args "$t" '{tool: $tool, arguments: $args, server: ["qmd", "--index", "'"$project"'", "mcp"]}' > "$work/request-limit$2.json"
    }
    run_pass "$work/pass1.jsonl" 20 "$work/dump1.jsonl" "$work/times1.jsonl"
    : > "$work/pass2.jsonl"
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"; n="$(jq -c '.' <<<"$line" | pages_of | jq 'length')"; h="$(hits_of <<<"$line")"
      if [ "$n" -lt 10 ] && [ "${h:-0}" -ge 20 ]; then jq -c --arg id "$id" 'select(.id == $id)' "$work/pass1.jsonl" >> "$work/pass2.jsonl"; fi
    done < "$work/dump1.jsonl"
    if [ -s "$work/pass2.jsonl" ]; then run_pass "$work/pass2.jsonl" 40 "$work/dump2.jsonl" "$work/times2.jsonl"; else : > "$work/dump2.jsonl"; : > "$work/times2.jsonl"; fi
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"
      final="$line"; limit=20
      if l2="$(jq -c --arg id "$id" 'select(.id == $id)' "$work/dump2.jsonl" | head -1)" && [ -n "$l2" ]; then final="$l2"; limit=40; fi
      paths="$(pages_of <<<"$final")"; h="$(hits_of <<<"$final")"; ok="$(jq -r .ok <<<"$final")"
      n="$(jq 'length' <<<"$paths")"; truncated=false
      if [ "$n" -lt 10 ] && [ "$limit" = 40 ] && [ "${h:-0}" -ge 40 ]; then truncated=true; fi
      ms="$(jq -r --arg id "$id" 'select(.id == $id) | .ms' "$work/times1.jsonl" | head -1)"
      jq -nc --arg id "$id" --argjson paths "$paths" --argjson truncated "$truncated" --argjson limit "$limit" --argjson hits "${h:-0}" --argjson ok "$ok" --arg mode "$mode" --argjson ms "${ms:-null}" \
        '{question_id: $id, paths: $paths, truncated: $truncated, request: {tool: "query", mode: $mode, limit: $limit}, hits: $hits, ok: $ok, first_pass_ms: $ms}' >> "$out"
    done < "$work/dump1.jsonl"
    n_rows="$(grep -c . "$out")"; n_err="$(jq -s 'map(select(.ok | not)) | length' "$out")"; n_tr="$(jq -s 'map(select(.truncated)) | length' "$out")"
    cp "$work/request-limit20.json" "$out.request.json"
    echo "wrote $out ($n_rows rows, mode $mode, $n_err failed calls, $n_tr truncated; requests in $work/, first-pass latency in $work/times1.jsonl)"
    [ "$n_err" = 0 ] || echo "WARNING: $n_err question(s) had a failed MCP call (scored as empty lists, rule 0.3); see $out.err" >&2
    ;;
  status) qmd --index "$project" status ;;
  *) die "unknown command $cmd" ;;
esac
