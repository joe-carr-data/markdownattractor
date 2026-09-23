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

case "$cmd" in
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
    status="$(qmd --index "$project" status 2>&1)"
    files="$(qmd --index "$project" ls 2>/dev/null | grep -cE '^\s+\S' || true)"
    on_disk="$(find "$corpus" -type f \( -name '*.md' -o -name '*.mdx' -o -name '*.markdown' \) -not -path '*/.markdownattractor/*' -not -path '*/.git/*' | wc -l | tr -d ' ')"
    # Coverage against the dataset corpus: which of its pages the index lists.
    total=0; present=0; missing='[]'
    listed="$(qmd --index "$project" ls "$project" 2>/dev/null | sed -n 's|^\s*qmd://'"$project"'/||p' | sed 's/\s.*$//')"
    while IFS= read -r p; do
      total=$((total + 1))
      if grep -qxF "$p" <<<"$listed"; then present=$((present + 1)); else missing="$(jq -c --arg p "$p" '. + [$p]' <<<"$missing")"; fi
    done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
    jq -n --arg arm qmd --arg project "$project" --arg version "$(qmd_version)" --arg node "$(node --version)" --arg mask "$MASK" \
      --argjson add_s "$((t1 - t0))" --argjson update_s "$((t2 - t1))" --argjson embed_s "$((t3 - t2))" \
      --arg status "$status" --argjson files "${files:-0}" --argjson on_disk "$on_disk" --argjson total "$total" --argjson present "$present" --argjson missing "$missing" \
      --arg models "$(models_sha)" \
      '{arm: $arm, project: $project, version: $version, node: $node,
        install: "npm i -g @tobilu/qmd@2.8.3", index: ("~/.cache/qmd/" + $project + ".sqlite"), collection: ("~/.config/qmd/" + $project + ".yml"),
        config: {mask: $mask, candidate_limit: 40, results_default: 5, rerank: "Qwen3-Reranker-0.6B (default on)", expansion: "qmd-query-expansion-1.7B (default on)", embed: "EmbeddingGemma-300M", gpu: "metal (default)"},
        build: {collection_add_s: $add_s, update_s: $update_s, embed_s: $embed_s, total_s: ($add_s + $update_s + $embed_s)},
        coverage: {files_indexed: $files, markdown_files_on_disk: $on_disk, corpus_pages: $total, corpus_pages_indexed: $present, coverage: (if $total == 0 then 0 else $present / $total end), missing_pages: $missing},
        models_sha256: ($models | split("\n") | map(select(. != ""))), status: $status}' > "$ARMS/qmd-$project.json"
    echo "built qmd index $project: add $((t1 - t0)) s · update $((t2 - t1)) s · embed $((t3 - t2)) s · $present of $total corpus pages indexed · $ARMS/qmd-$project.json"
    ;;
  drive)
    mode="${1:?full|no-rerank|bm25}"; out="${2:?out.jsonl}"; split="${3:-dev}"
    safe_target "$out"; [ ! -e "$out" ] || die "$out exists: one directory per attempt, nothing is overwritten"
    case "$mode" in
      full) sub=(query); extra=() ;;
      no-rerank) sub=(query); extra=(--no-rerank) ;;
      bm25) sub=(search); extra=() ;;
      *) die "mode must be full, no-rerank or bm25" ;;
    esac
    # Every eligible question of the split, in dataset order, from the committed results
    # (the raw row lists exactly the scored questions).
    ids="$(jq -r --arg s "$split" '.runs[0].results[] | select(.split == $s) | .id' "$RESULTS/$project/results.json")"
    n_total="$(printf '%s\n' "$ids" | grep -c .)"; i=0
    while IFS= read -r qid; do
      i=$((i + 1))
      q="$(question_text "$project" "$qid" | tr '\n\r\t' '   ' | sed 's/  */ /g')"
      n=20; pages='[]'; hits=0; truncated=false; t0=$(date +%s%N)
      while :; do
        raw="$( (cd "$corpus" && qmd --index "$project" "${sub[@]}" "$q" --format json -n "$n" --full-path "${extra[@]}" 2>>"$out.err") || echo '[]')"
        hits="$(jq 'length' <<<"$raw" 2>/dev/null || echo 0)"
        # Distinct files in rank order (chunks of one page collapse onto its first chunk).
        pages="$(jq -c '[.[] | .file | sub("^\\./"; "")] | reduce .[] as $p ([]; if index([$p]) then . else . + [$p] end)' <<<"$raw" 2>/dev/null || echo '[]')"
        npages="$(jq 'length' <<<"$pages")"
        if [ "$npages" -ge 10 ] || [ "$hits" -lt "$n" ]; then break; fi
        if [ "$n" -ge 320 ]; then truncated=true; break; fi
        n=$((n * 2))
      done
      t1=$(date +%s%N)
      jq -nc --arg id "$qid" --argjson paths "$pages" --argjson truncated "$truncated" --argjson n "$n" --argjson hits "$hits" --arg q "$q" --arg mode "$mode" --argjson ms "$(( (t1 - t0) / 1000000 ))" \
        '{question_id: $id, paths: $paths, truncated: $truncated, request: {command: (if $mode == "bm25" then "qmd search" else "qmd query" end), mode: $mode, n: $n, full_path: true, format: "json", query: $q}, hits: $hits, wall_ms: $ms}' >> "$out"
      printf '%s/%s %s: %s pages from %s hits (n=%s, %s ms)%s\n' "$i" "$n_total" "$qid" "$npages" "$hits" "$n" "$(( (t1 - t0) / 1000000 ))" "$([ "$truncated" = true ] && echo ' TRUNCATED')"
    done <<<"$ids"
    echo "wrote $out ($n_total rows, mode $mode)"
    ;;
  status) qmd --index "$project" status ;;
  *) die "unknown command $cmd" ;;
esac
