#!/usr/bin/env bash
# The graphify arm (execution plan §1.2, §2.1): build the knowledge graph of a project's
# checkout with graphify's own skill from a headless Claude Code session (its docs pass is a
# model pass through the owner's login: the subagents graphify dispatches read the pages),
# archive graph.json with its hash, record build time, tokens and the resolved model, and
# drive the questions through graphify's MCP server (`query_graph`).
#
# Usage:
#   scripts/eval/arms/graphify.sh build <project> [model=sonnet]   # copy of the checkout under $RUN/graphify/<project>/src, graph archived
#   scripts/eval/arms/graphify.sh drive <project> <out.jsonl> [split=dev]
# The build runs on a COPY of the checkout (graphify writes graphify-out/ into its input
# path; the pinned checkout must stay clean for the freeze) with graphify's skill, CLAUDE.md
# nudge and PreToolUse hooks installed at PROJECT level inside that copy (`graphify install
# --project --platform claude` writes .claude/{settings.json,skills/graphify,CLAUDE.md} and a
# CLAUDE.md section) and the session started with `--setting-sources project`, so the arm
# runs with its own hook active and nothing is installed into the owner's Claude Code
# configuration (an isolated CLAUDE_CONFIG_DIR has no login). The skill asks the user to
# narrow corpora above 500 files; the headless prompt answers that in advance (whole path,
# no narrowing): every arm indexes the whole corpus.
set -euo pipefail
# shellcheck source=../lib.sh
. "$(dirname "$0")/../lib.sh"
cmd="${1:?build|drive}"; project="${2:?project}"; shift 2
ident "$project"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"
G="$RUN/graphify/$project"
ARMS="$RESULTS/arms"; mkdir -p "$ARMS"
case "$cmd" in
  build)
    model="${1:-sonnet}"
    [ ! -e "$G" ] || die "$G exists: a build is done once per freeze (move it aside to rebuild)"
    mkdir -p "$G/src"
    rsync -a --exclude .markdownattractor --exclude .git "$corpus/" "$G/src/"
    ( cd "$G/src" && graphify install --project --platform claude ) > "$G/install.log" 2>&1 || die "graphify install failed: $G/install.log"
    [ -f "$G/src/.claude/skills/graphify/SKILL.md" ] && [ -f "$G/src/.claude/settings.json" ] || die "graphify install wrote no project skill/hooks"
    cp -R "$G/src/.claude" "$G/installed-dot-claude"   # archived: the skill, hooks and nudge the arm ran with
    unset_nested_session; unset_provider_keys
    prompt="/graphify $G/src --no-viz
You are running headless: there is no user to answer questions. Run the pipeline on the whole path exactly as given, never narrow to a subfolder even if the corpus is large (every arm of this benchmark indexes the whole corpus), never ask for confirmation. There is no API key of any kind and you must not look for one, install packages, or change the graphify installation: you and the subagents you dispatch are the model, exactly as the skill's 'host agent itself is the LLM' path says. Dispatch the semantic subagents as the skill says. When the pipeline is complete, print the final report."
    t0=$(date +%s)
    ( cd "$G/src" && claude --print --setting-sources project --no-session-persistence --model "$model" --max-turns 600 \
        --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config \
        --allowedTools Bash Read Write Edit Glob Grep Agent -- "$prompt" ) > "$G/build.jsonl" 2>"$G/build.err" || echo "claude exited $?" >> "$G/build.err"
    t1=$(date +%s)
    graph="$G/src/graphify-out/graph.json"
    if [ ! -f "$graph" ]; then
      jq -n --arg project "$project" --argjson s "$((t1 - t0))" '{arm: "graphify", project: $project, build: {wall_s: $s, completed: false}, note: "did not complete: no graphify-out/graph.json (rule 0.3: recorded, not dropped)"}' > "$ARMS/graphify-$project.json"
      die "graphify build of $project did not produce graph.json after $((t1 - t0)) s (see $G/build.err, $G/build.jsonl)"
    fi
    cp "$graph" "$G/graph.json"
    res="$(jq -c -s '(map(select(.type=="result")) | last) as $r | {turns: ($r.num_turns // null), cost_usd_list_price: ($r.total_cost_usd // null), input_tokens: (($r.usage.input_tokens // 0) + ($r.usage.cache_read_input_tokens // 0) + ($r.usage.cache_creation_input_tokens // 0)), output_tokens: ($r.usage.output_tokens // 0), is_error: ($r.is_error // false), models: (map(select(.type=="assistant")) | map(.message.model // empty) | unique)}' "$G/build.jsonl")"
    nodes="$(jq '.nodes | length' "$G/graph.json")"; edges="$(jq '(.links // .edges) | length' "$G/graph.json")"
    # Coverage: which dataset pages appear as a node source file (the frozen node → file mapping
    # reads the same fields, see `drive`).
    jq -r '[.nodes[] | (.source_file // .file // .path // empty), (.source_files[]? // empty)] | unique | .[]' "$G/graph.json" | sed "s|^$G/src/||; s|^\./||" | LC_ALL=C sort -u > "$G/node-files.txt"
    total=0; present=0
    while IFS= read -r p; do total=$((total + 1)); grep -qxF "$p" "$G/node-files.txt" && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
    jq -n --arg project "$project" --arg version "$(graphify --version 2>/dev/null | head -1)" --arg model "$model" --argjson res "$res" --argjson s "$((t1 - t0))" \
      --argjson nodes "$nodes" --argjson edges "$edges" --arg sha "$(sha256 "$G/graph.json")" --argjson total "$total" --argjson present "$present" \
      --arg skill_sha "$(sha256 "$G/src/.claude/skills/graphify/SKILL.md")" --arg hooks_sha "$(sha256 "$G/src/.claude/settings.json")" --arg graph "${G/#$HOME/\~}/graph.json" \
      '{arm: "graphify", project: $project, version: $version, install: "uv tool install graphifyy[mcp]==0.9.66; graphify install --project --platform claude inside the checkout copy (skill, PreToolUse hooks, CLAUDE.md nudge; archived as installed-dot-claude/)",
        build: ({wall_s: $s, completed: (($res.is_error | not)), model_alias: $model, command: "claude -p \"/graphify <copy of checkout> --no-viz\" (headless, whole path)"} + $res),
        graph: {path: $graph, sha256: $sha, nodes: $nodes, edges: $edges}, skill_sha256: $skill_sha, hooks_settings_sha256: $hooks_sha,
        coverage: {corpus_pages: $total, corpus_pages_with_a_node: $present, coverage: (if $total == 0 then 0 else $present / $total end)},
        query_interface: {mcp: "graphify-mcp <graph.json>", tool: "query_graph", node_to_file: "each node of the response, its source file(s) in the order the tool lists them, deduplicated; the response size limit, if any, is the truncation"}}' > "$ARMS/graphify-$project.json"
    echo "graphify $project: $nodes nodes, $edges edges in $((t1 - t0)) s · $present of $total corpus pages have a node · $ARMS/graphify-$project.json"
    ;;
  drive)
    out="${1:?out.jsonl}"; split="${2:-dev}"
    safe_target "$out"; [ ! -e "$out" ] || die "$out exists"
    [ -f "$G/graph.json" ] || die "no $G/graph.json: build first"
    client="${MCP_TIME:-$REPO/target/release/examples/mcp_time}"
    [ -x "$client" ] || die "no MCP client at $client"
    work="$out.work"; mkdir -p "$work"
    jq -r --arg s "$split" '.runs[0].results[] | select(.split == $s) | .id' "$RESULTS/$project/results.json" | while IFS= read -r qid; do
      jq -nc --arg id "$qid" --arg q "$(question_text "$project" "$qid" | tr '\n\r\t' '   ' | sed 's/  */ /g')" '{id: $id, q: $q}'
    done > "$work/queries.jsonl"
    # `query_graph` answers with text: one `NODE <label> [src=<file> …]` line per node in the
    # tool's order, cut to its token budget (2,000 by default) with a `[!] TRUNCATED` marker.
    # The frozen node → file mapping takes every NODE line's src in order, deduplicated.
    # Two passes like every driver: the default budget first (what an agent gets), then
    # token_budget 8000 for the questions still short of ten distinct pages under the cut;
    # a question still short at 8000 is `truncated` (the response size limit, plan §2.1).
    client_run() { # queries.jsonl template dump times
      ( cd "$G/src" && MCP_TIME_DUMP="$3" "$client" query_graph "$2" "$1" -- graphify-mcp "$G/graph.json" ) > "$4" 2>>"$out.err"
    }
    pages_of() { jq -c '[(.result.content[]? | select(.type=="text") | .text) // "" | scan("NODE [^\\n]*?\\[src=([^ \\]]+)") | .[0]] | map(sub("^\\./"; "")) | reduce .[] as $p ([]; if index([$p]) then . else . + [$p] end)'; }
    cut_of() { jq -r '[(.result.content[]? | select(.type=="text") | .text) // ""] | join("") | test("\\[!\\] TRUNCATED")'; }
    t1='{"question":"{query}"}'; t2='{"question":"{query}","token_budget":8000}'
    client_run "$work/queries.jsonl" "$t1" "$work/dump1.jsonl" "$work/times1.jsonl"
    : > "$work/pass2.jsonl"
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"; n="$(pages_of <<<"$line" | jq 'length')"; cut="$(cut_of <<<"$line")"
      if [ "$n" -lt 10 ] && [ "$cut" = true ]; then jq -c --arg id "$id" 'select(.id == $id)' "$work/queries.jsonl" >> "$work/pass2.jsonl"; fi
    done < "$work/dump1.jsonl"
    if [ -s "$work/pass2.jsonl" ]; then client_run "$work/pass2.jsonl" "$t2" "$work/dump2.jsonl" "$work/times2.jsonl"; else : > "$work/dump2.jsonl"; fi
    jq -n --arg tool query_graph --argjson a1 "$t1" --argjson a2 "$t2" --arg graph "$G/graph.json" '{tool: $tool, arguments: $a1, second_pass: $a2, server: ["graphify-mcp", $graph]}' > "$out.request.json"
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"; final="$line"; budget=2000
      if l2="$(jq -c --arg id "$id" 'select(.id == $id)' "$work/dump2.jsonl" | head -1)" && [ -n "$l2" ]; then final="$l2"; budget=8000; fi
      paths="$(pages_of <<<"$final")"; ok="$(jq -r .ok <<<"$final")"; n="$(jq 'length' <<<"$paths")"; cut="$(cut_of <<<"$final")"
      truncated=false; [ "$n" -ge 10 ] || [ "$cut" != true ] || truncated=true
      ms="$(jq -r --arg id "$id" 'select(.id == $id) | .ms' "$work/times1.jsonl" | head -1)"
      jq -nc --arg id "$id" --argjson paths "$paths" --argjson truncated "$truncated" --argjson ok "$ok" --argjson budget "$budget" --argjson ms "${ms:-null}" '{question_id: $id, paths: $paths, truncated: $truncated, request: {tool: "query_graph", token_budget: $budget}, ok: $ok, first_pass_ms: $ms}' >> "$out"
    done < "$work/dump1.jsonl"
    echo "wrote $out ($(grep -c . "$out") rows)"
    ;;
  *) die "unknown command $cmd" ;;
esac
