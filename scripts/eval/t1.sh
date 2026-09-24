#!/usr/bin/env bash
# T1, axis A on DocsQA, the test split once (execution plan §4, §5 M4): every arm driven and
# scored into evals/results/docsqa/T1/<project>/ under the final freeze (T1/FROZEN.md must
# check clean before anything runs), the latency of the arms that have an MCP server, the
# page table with per-row bootstrap intervals, and the product target (mda hybrid vs qmd
# full, paired) per project. Resumable: an arm whose results.json exists is skipped, so a
# run can be split across sessions; a rerun of an existing arm needs its files removed by
# hand (the test split is scored once; a second run is a published independent rerun, never
# a replacement — plan §2.0b).
#
# Usage:
#   scripts/eval/t1.sh run <project> [arm ...]     # arms: mda qmd-full qmd-no-rerank qmd-bm25 graphify graphify-haiku bm25-files (default: all)
#   scripts/eval/t1.sh latency <project> [arm ...] # arms with an MCP server: mda qmd graphify graphify-haiku (default: all built) → T1/<project>/arms/<arm>.times.jsonl
#   scripts/eval/t1.sh table                       # metrics with 95% intervals (mda eval --interval), one row per project and run
#   scripts/eval/t1.sh latency-table               # cold first call and warm median per arm and project, from the times files
#   scripts/eval/t1.sh target                      # mda hybrid vs qmd full per project, paired (mda eval --compare)
#   scripts/eval/t1.sh lose [results-dir]          # "where we lose": per project, hybrid's misses split by who finds the page, and how deep hybrid ranked it
# Env: REPO, RUN, MDA (scripts/eval/lib.sh). Every driver unsets provider keys itself.
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
cmd="${1:?run|latency|table|latency-table|target}"; shift
TABLE=T1; T="$(results_dir $TABLE)"; SPLIT="test"
HYBRID="hybrid (cards + raw + vectors)"
ALL_ARMS="mda qmd-full qmd-no-rerank qmd-bm25 graphify graphify-haiku bm25-files"
data="$RUN/docsqa-data"
# One freeze check at a time on this machine (two at once contended on qmd's sqlite once).
frozen_ok() {
  local lock="$RUN/t1-runs/.freeze-check.lock" i=0
  mkdir -p "$RUN/t1-runs"
  until mkdir "$lock" 2>/dev/null; do i=$((i + 1)); [ "$i" -lt 600 ] || die "freeze-check lock $lock held for 10 min"; sleep 1; done
  trap 'rmdir "$lock" 2>/dev/null' EXIT
  "$REPO/scripts/eval/freeze.sh" --protocol final --table $TABLE --check >/dev/null || die "$T/FROZEN.md does not check clean: nothing runs against changed inputs"
  rmdir "$lock"; trap - EXIT
}
arm_name() { # arm -> the run name used on the development rows (one name per arm across tables)
  case "$1" in
    qmd-full) echo "qmd full (MCP query, rerank)" ;;
    qmd-no-rerank) echo "qmd no-rerank (MCP query, rerank off)" ;;
    qmd-bm25) echo "qmd BM25 (MCP lex-only, rerank off)" ;;
    graphify) echo "graphify (query_graph)" ;;
    graphify-haiku) echo "graphify-haiku (query_graph)" ;;
    bm25-files) echo "BM25-over-files" ;;
    *) die "no run name for arm $1" ;;
  esac
}
built() { # arm project -> 0 when the arm's build completed on the project
  local rec="$RESULTS/arms/$1-$2.json"
  [ -f "$rec" ] || return 1
  [ "$(jq -r 'if .build.completed == false then "false" else "true" end' "$rec")" = true ]
}
case "$cmd" in
  run)
    project="${1:?project}"; shift; ident "$project"; arms="${*:-$ALL_ARMS}"
    frozen_ok
    dir="$(project_dir "$project")"; corpus="$RUN/$dir"; out="$T/$project"; safe_target "$out/results.json"; mkdir -p "$out/arms"
    work="$RUN/t1-runs/$project"; mkdir -p "$work"
    for arm in $arms; do
      case " $ALL_ARMS " in *" $arm "*) ;; *) die "unknown arm $arm" ;; esac
      if [ "$arm" = mda ]; then
        if [ -f "$out/results.json" ]; then echo "mda: $out/results.json exists, skipped"; continue; fi
        t0=$(date +%s)
        "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split $SPLIT --out "$out" > "$work/mda.report.json" 2>"$work/mda.err" || die "mda eval failed on $project (see $work/mda.err)"
        [ "$(jq -r '.card_coverage.complete' "$out/results.json")" = true ] || die "$project: card coverage incomplete"
        jq -e --arg h "$HYBRID" '.runs[] | select(.run == $h) | select(.metrics.questions > 0)' "$out/results.json" >/dev/null || die "no hybrid row on $project"
        echo "mda: $(jq -r --arg h "$HYBRID" '.runs[] | select(.run == $h) | "\(.metrics.questions) questions · success@5 \(.metrics.success_at_5)"' "$out/results.json") in $(( $(date +%s) - t0 )) s"
        continue
      fi
      res="$out/arms/$arm.results.json"
      if [ -f "$res" ]; then echo "$arm: $res exists, skipped"; continue; fi
      case "$arm" in
        graphify|graphify-haiku) if ! built "$arm" "$project"; then echo "$arm: build did not complete on $project (arms/$arm-$project.json), no rows (rule 0.3)"; continue; fi ;;
      esac
      rows="$work/$arm.jsonl"; rm -rf "$rows" "$rows.work" "$rows.err" "$rows.request.json" "$rows.ids"
      t0=$(date +%s)
      case "$arm" in
        qmd-full) "$REPO/scripts/eval/arms/qmd.sh" drive "$project" full "$rows" $SPLIT ;;
        qmd-no-rerank) "$REPO/scripts/eval/arms/qmd.sh" drive "$project" no-rerank "$rows" $SPLIT ;;
        qmd-bm25) "$REPO/scripts/eval/arms/qmd.sh" drive "$project" bm25 "$rows" $SPLIT ;;
        graphify) "$REPO/scripts/eval/arms/graphify.sh" drive "$project" "$rows" $SPLIT ;;
        graphify-haiku) GRAPHIFY_MODEL=haiku "$REPO/scripts/eval/arms/graphify.sh" drive "$project" "$rows" $SPLIT ;;
        bm25-files) "$REPO/scripts/eval/bm25-files.sh" drive "$project" "$rows" $SPLIT ;;
      esac || die "$arm drive failed on $project"
      t1=$(date +%s)
      name="$(arm_name "$arm")"; sdir="$work/$arm.score"; rm -rf "$sdir"; mkdir -p "$sdir"
      "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split $SPLIT --arm-output "$rows" --arm-name "$name" --out "$sdir" > "$sdir/report.json" 2>"$sdir/err" || die "scoring $arm failed on $project (see $sdir/err)"
      # the committed observations: rows, the request template, first-pass latency, the scorer's output
      jq -c '{question_id, paths, truncated}' "$rows" > "$out/arms/$arm.jsonl"
      [ ! -f "$rows.request.json" ] || cp "$rows.request.json" "$out/arms/$arm.request.json"
      [ ! -f "$rows.work/times1.jsonl" ] || cp "$rows.work/times1.jsonl" "$out/arms/$arm.times.jsonl"
      cp "$sdir/results.json" "$res"
      echo "$arm: $(jq -r '.runs[0] | "\(.metrics.questions) questions · success@5 \(.metrics.success_at_5) · missing \(.missing | length)"' "$res") · drive $((t1 - t0)) s"
    done ;;
  latency)
    project="${1:?project}"; shift; ident "$project"; arms="${*:-mda qmd graphify graphify-haiku}"
    frozen_ok
    out="$T/$project/arms"; mkdir -p "$out"
    for arm in $arms; do
      f="$out/$arm.mcp-times.jsonl"
      if [ -f "$f" ]; then echo "$arm: $f exists, skipped"; continue; fi
      case "$arm" in graphify|graphify-haiku) built "$arm" "$project" || { echo "$arm: not built on $project, skipped"; continue; } ;; esac
      "$REPO/scripts/eval/mcp-time.sh" "$arm" "$project" "$f" $SPLIT
    done ;;
  table)
    echo "| Project | test scored | run | success@5 [95%] | MRR@5 [95%] | nDCG@10 [95%] | truncated |"
    echo "|---|---|---|---|---|---|---|"
    for p in $PROJECTS; do
      files=("$T/$p/results.json"); for a in "$T/$p"/arms/*.results.json; do [ -f "$a" ] && files+=("$a"); done
      [ -f "$T/$p/results.json" ] || continue
      args=(); for f in "${files[@]}"; do args+=(--interval "$f"); done
      "$MDA" --json eval "${args[@]}" --draws 5000 --seed "$SEED" | jq -r --arg p "$p" --slurpfile all <(for f in "${files[@]}"; do jq -c '{runs: [.runs[] | {run, truncated: ([.results[] | select(.truncated)] | length)}]}' "$f"; done) '
        def r3: . * 1000 | round / 1000 | tostring;
        def ci(x; c): (x | r3) + " [" + (c[0] | r3) + ", " + (c[1] | r3) + "]";
        [.files[].runs[]] | .[] | . as $r |
        ([$all[] | .runs[] | select(.run == $r.run) | .truncated] | first // 0) as $tr |
        "| \($p) | \($r.n) | \($r.run) | \(ci($r.success_at_5; $r.success_ci95)) | \(ci($r.mrr_at_5; $r.mrr_ci95)) | \(ci($r.ndcg_at_10; $r.ndcg_ci95)) | \($tr) |"'
    done
    echo
    echo "Generated by \`scripts/eval/t1.sh table\` from \`evals/results/docsqa/T1/<project>/results.json\` and \`T1/<project>/arms/*.results.json\` (split: test, scored once; mda $(jq -r .mda_version "$T/tailwind-css/results.json" 2>/dev/null || echo '?'); intervals: 95% bootstrap over the row's questions, 5,000 draws, seed $SEED, \`mda eval --interval\`)." ;;
  latency-table)
    echo "| Project | arm | queries | cold first call (startup + call) | warm median ms | warm p90 ms | failed |"
    echo "|---|---|---|---|---|---|---|"
    for p in $PROJECTS; do for f in "$T/$p"/arms/*.mcp-times.jsonl; do
      [ -f "$f" ] || continue; a="$(basename "$f" .mcp-times.jsonl)"
      jq -s -r --arg p "$p" --arg a "$a" 'def pct(q): sort | if length == 0 then null else .[((length - 1) * q | floor)] end;
        (map(select(.ok and (.cold | not)) | .ms)) as $w |
        "| \($p) | \($a) | \(length) | \((map(select(.cold)) | first) as $c | if $c == null then "n/a" else "\($c.startup_ms // "?") + \($c.ms) ms" end) | \($w | pct(0.5)) | \($w | pct(0.9)) | \(map(select(.ok | not)) | length) |"' "$f"
    done; done
    echo
    echo "Generated by \`scripts/eval/t1.sh latency-table\` from \`T1/<project>/arms/<arm>.mcp-times.jsonl\` (\`scripts/eval/mcp-time.sh\`: one rmcp stdio client, the server started cold, the first query includes process start and model load; plan §2.7)." ;;
  target)
    echo "| Project | n | qmd full success@5 | mda hybrid success@5 | Δ (hybrid − qmd full), 95% paired | wins/losses | target (match qmd full) |"
    echo "|---|---|---|---|---|---|---|"
    for p in $PROJECTS; do
      b="$T/$p/arms/qmd-full.results.json"; c="$T/$p/results.json"; [ -f "$b" ] && [ -f "$c" ] || continue
      "$MDA" --json eval --compare "$p" "$b" "$c" --run-name "$(arm_name qmd-full)" --candidate-run "$HYBRID" --draws 5000 --seed "$SEED" | jq -r --arg p "$p" '
        def r3: . * 1000 | round / 1000 | tostring; def r4: . * 10000 | round / 10000 | tostring;
        .report.projects[0] as $r | .report as $o |
        "| \($p) | \($r.n) | \($r.baseline | r3) | \($r.candidate | r3) | \($r.delta | r4) [\($r.ci95[0] | r4), \($r.ci95[1] | r4)] | \($r.wins)/\($r.losses) | \(if $r.delta >= 0 then "met (point estimate ≥)" else "not met (point estimate <)" end)\(if $r.ci95[0] <= 0 and $r.ci95[1] >= 0 then "; interval includes 0" else "" end) |"'
    done
    echo
    echo "Generated by \`scripts/eval/t1.sh target\` (\`mda eval --compare\`: within-project paired bootstrap, 5,000 draws, seed $SEED; the product target of plan §4 is reported as met or not per project on the point estimate, with the interval beside it)." ;;
  lose)
    # For every project: hybrid's success@5 misses, split by whether qmd full / BM25-over-files /
    # graphify found the page in their top five (the same archived rows), and hybrid's own rank of
    # the first relevant page on those misses (6–10 = on the page but below the cutoff; > 10 or
    # absent = not in the ten pages archived). Then the questions only hybrid gets.
    R="${1:-$T}"
    echo "| Project | n | hybrid misses | … found by qmd full | … by BM25-over-files | … by graphify | … by none of the three | hybrid rank 6–10 on its misses | rank > 10 / absent | only hybrid gets |"
    echo "|---|---|---|---|---|---|---|---|---|---|"
    for p in $PROJECTS; do
      f="$R/$p/results.json"; [ -f "$f" ] || continue
      hit() { jq -c --arg h "$1" '[.runs[] | select($h == "" or .run == $h)][0] // {results: []} | [.results[] | {id, hit: (.rank != null and .rank <= 5), rank}]' "$2"; }
      q="$R/$p/arms/qmd-full.results.json"; b="$R/$p/arms/bm25-files.results.json"; g="$R/$p/arms/graphify.results.json"
      jq -n -r --arg p "$p" --argjson hy "$(hit "$HYBRID" "$f")" \
        --argjson qm "$([ -f "$q" ] && hit "" "$q" || echo '[]')" \
        --argjson bm "$([ -f "$b" ] && hit "" "$b" || echo '[]')" \
        --argjson gr "$([ -f "$g" ] && hit "" "$g" || echo '[]')" '
        def hits(a): [a[] | select(.hit) | .id];
        ($hy | map(select(.hit | not))) as $miss | (hits($qm)) as $Q | (hits($bm)) as $B | (hits($gr)) as $G |
        ($miss | map(.id)) as $M |
        "| \($p) | \($hy | length) | \($M | length) | \([$M[] | select(. as $i | $Q | index([$i]))] | length) | \([$M[] | select(. as $i | $B | index([$i]))] | length) | \(if ($gr | length) == 0 then "n/a" else ([$M[] | select(. as $i | $G | index([$i]))] | length | tostring) end) | \([$M[] | select(. as $i | (($Q + $B + $G) | index([$i])) == null)] | length) | \([$miss[] | select(.rank != null and .rank >= 6 and .rank <= 10)] | length) | \([$miss[] | select(.rank == null or .rank > 10)] | length) | \([$hy[] | select(.hit) | .id | select(. as $i | (($Q + $B + $G) | index([$i])) == null)] | length) |"'
    done
    echo
    echo "Generated by \`scripts/eval/t1.sh lose\` from the archived rows (success@5 misses of the hybrid row; \"found by\" = that arm's first relevant page within its top five on the same question)." ;;
  *) die "unknown command $cmd" ;;
esac
