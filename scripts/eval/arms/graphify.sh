#!/usr/bin/env bash
# The graphify arm (execution plan §1.2, §2.1): build the knowledge graph of a project's
# checkout with graphify's own skill from a headless Claude Code session (its docs pass is a
# model pass through the owner's login: the subagents graphify dispatches read the pages),
# archive graph.json with its hash, record build time, tokens and the resolved models, and
# drive the questions through graphify's MCP server (`query_graph`).
#
# Usage:
#   scripts/eval/arms/graphify.sh build <project> [model=sonnet]     # copy of the checkout under $RUN/graphify/<project>[-model]/src, graph archived
#   scripts/eval/arms/graphify.sh record <project> [model=sonnet]    # rewrite the record from an existing build directory's transcript
#   GRAPHIFY_MODEL=<model> scripts/eval/arms/graphify.sh drive <project> <out.jsonl> [split=dev]
# One configuration per host model (Codex 2026-09-23): the Sonnet-built graphs are the
# `graphify` arm, a Haiku-built graph is the `graphify-haiku` arm, never mixed, each with
# its own copy, cache, record and rows. The build runs on a COPY of the checkout (graphify
# writes graphify-out/ into its input path; the pinned checkout must stay clean for the
# freeze) with graphify's skill, CLAUDE.md nudge and PreToolUse hooks installed at PROJECT
# level inside that copy (`graphify install --project --platform claude`) and the session
# started with `--setting-sources project`, so the arm runs with its own hook active and
# nothing is installed into the owner's Claude Code configuration (an isolated
# CLAUDE_CONFIG_DIR has no login). Provider keys are unset (plan §0a.3), the headless
# background-wait ceiling is lifted and wakeup/cron tools are disallowed (both ended
# sessions before the graph was built on 2026-09-23). The skill asks the user to narrow
# corpora above 500 files; the headless prompt answers that in advance (whole path): every
# arm indexes the whole corpus.
set -euo pipefail
# shellcheck source=../lib.sh
. "$(dirname "$0")/../lib.sh"
cmd="${1:?build|record|drive}"; project="${2:?project}"; shift 2
ident "$project"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"
[ -d "$corpus" ] || die "no checkout at $corpus"
model="${GRAPHIFY_MODEL:-sonnet}"
case "$cmd" in build|record) model="${1:-sonnet}" ;; esac
ident "$model"
arm="graphify"; [ "$model" = sonnet ] || arm="graphify-$model"
G="$RUN/graphify/$project"; [ "$model" = sonnet ] || G="$RUN/graphify/$project-$model"
ARMS="$RESULTS/arms"; mkdir -p "$ARMS" "$ARMS/graphs"

# One routine for every graph source path: the copy's absolute prefix removed, `./` removed.
canon_path() { sed -e "s|^$G/src/||" -e 's|^\./||'; }

# The arm record from a build directory's transcript and outputs (Codex M2 F2, F4).
# Completed means all of: claude exited 0, the terminal result is not an error, the session
# never called a wakeup or cron tool, and graphify-out/graph.json exists; anything else is
# did-not-complete with the graph quarantined as graph.json.partial (never scored). Usage:
# the transcript's `modelUsage` (per resolved model, whole session: parent and subagents)
# and the terminal result's `usage` (labelled with its scope). Attempts kept aside as
# <dir>.attempt* are listed with their own turns, usage, cost and last words.
record_build() { # rc wall_s
  local rc="$1" wall="$2" graph="$G/src/graphify-out/graph.json" res ended_early completed=false attempts='[]' a
  res="$(jq -c -s '(map(select(.type=="result")) | last) as $r | {turns: ($r.num_turns // null), cost_usd_list_price: ($r.total_cost_usd // null), is_error: (if $r == null then true else ($r.is_error == true) end), result_tail: (($r.result // "") | .[-160:]), terminal_result_usage: {scope: "the terminal result event as Claude Code reports it", uncached_input: ($r.usage.input_tokens // 0), cache_creation: ($r.usage.cache_creation_input_tokens // 0), cache_read: ($r.usage.cache_read_input_tokens // 0), output: ($r.usage.output_tokens // 0)}, model_usage: ($r.modelUsage // {} | with_entries(.value |= {uncached_input: (.inputTokens // 0), cache_creation: (.cacheCreationInputTokens // 0), cache_read: (.cacheReadInputTokens // 0), output: (.outputTokens // 0), cost_usd_list_price: (.costUSD // null)})), assistant_messages: (map(select(.type=="assistant")) | length), models: (map(select(.type=="assistant")) | map(.message.model // empty) | unique), subagents_dispatched: (map(select(.type=="assistant")) | map(.message.content[]? | select(.type=="tool_use" and .name=="Agent")) | length)}' "$G/build.jsonl")"
  # A wakeup or cron call ends the session only when it is the session's last action (a
  # print session can resume after a wakeup: the Supabase Sonnet build did); the wakeups used
  # along the way are recorded either way.
  ended_early="$(jq -c -s '[.[] | select(.type=="assistant")] | (last.message.content // []) | [.[] | select(.type=="tool_use" and (.name | test("^(ScheduleWakeup|Cron)"))) | .name] | unique' "$G/build.jsonl")"
  wakeups="$(jq -c -s '[.[] | select(.type=="assistant") | .message.content[]? | select(.type=="tool_use" and (.name | test("^(ScheduleWakeup|Cron)"))) | .name] | length' "$G/build.jsonl")"
  if [ "$rc" = 0 ] && [ "$(jq -r .is_error <<<"$res")" = false ] && [ "$ended_early" = "[]" ] && [ -f "$graph" ]; then completed=true; fi
  for a in "$G".attempt*; do
    [ -f "$a/build.jsonl" ] || continue
    attempts="$(jq -c --arg dir "${a/#$HOME/\~}" --argjson r "$(jq -c -s '(map(select(.type=="result")) | last) as $r | {turns: ($r.num_turns // null), cost_usd_list_price: ($r.total_cost_usd // null), models: (map(select(.type=="assistant")) | map(.message.model // empty) | unique), model_usage: ($r.modelUsage // {} | with_entries(.value |= {cache_read: (.cacheReadInputTokens // 0), cache_creation: (.cacheCreationInputTokens // 0), output: (.outputTokens // 0)})), result_tail: (($r.result // "") | .[-160:])}' "$a/build.jsonl")" '. + [{attempt_dir: $dir} + $r]' <<<"$attempts")"
  done
  if [ "$completed" = true ]; then
    cp "$graph" "$G/graph.json"; rm -f "$G/graph.json.partial"
    gzip -9 -n -c "$G/graph.json" > "$ARMS/graphs/$arm-$project.graph.json.gz"   # durable copy (plan §2.0: archived under evals/results/)
    local nodes edges total=0 present=0 sha p
    nodes="$(jq '.nodes | length' "$G/graph.json")"; edges="$(jq '(.links // .edges) | length' "$G/graph.json")"
    # Coverage of the dataset corpus pages by the nodes' source_file, the field the MCP
    # renderer exposes as `src` (the same routine the driver uses).
    jq -r '[.nodes[] | .source_file // empty] | unique | .[]' "$G/graph.json" | canon_path | LC_ALL=C sort -u > "$G/node-files.txt"
    while IFS= read -r p; do total=$((total + 1)); grep -qxF "$p" "$G/node-files.txt" && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
    sha="$(sha256 "$G/graph.json")"
    jq -n --arg arm "$arm" --arg project "$project" --arg version "$(graphify --version 2>/dev/null | head -1)" --arg model "$model" --argjson res "$res" --argjson s "$wall" --argjson rc "$rc" \
      --argjson nodes "$nodes" --argjson edges "$edges" --arg sha "$sha" --argjson total "$total" --argjson present "$present" --argjson attempts "$attempts" \
      --arg skill_sha "$(sha256 "$G/src/.claude/skills/graphify/SKILL.md")" --arg hooks_sha "$(sha256 "$G/src/.claude/settings.json")" --arg graph "${G/#$HOME/\~}/graph.json" \
      --arg nodes_with_source "$(jq '[.nodes[] | select((.source_file // "") != "")] | length' "$G/graph.json")" --arg distinct_files "$(wc -l < "$G/node-files.txt" | tr -d ' ')" \
      --argjson wakeups "$wakeups" '{arm: $arm, project: $project, version: $version, install: "uv tool install graphifyy[mcp]==0.9.66; graphify install --project --platform claude inside the checkout copy (skill, PreToolUse hooks, CLAUDE.md nudge; archived as installed-dot-claude/)",
        build: ({wall_s: $s, completed: true, claude_exit: $rc, host_model_alias: $model, wakeup_calls: $wakeups, command: "claude -p \"/graphify <copy of checkout> --no-viz\" (headless, whole path, --setting-sources project)"} + $res),
        failed_attempts: $attempts,
        graph: {path: $graph, archived: ("evals/results/docsqa/arms/graphs/" + $arm + "-" + $project + ".graph.json.gz"), sha256: $sha, nodes: $nodes, edges: $edges, nodes_with_source_file: ($nodes_with_source | tonumber), distinct_source_files: ($distinct_files | tonumber)}, skill_sha256: $skill_sha, hooks_settings_sha256: $hooks_sha,
        coverage: {population: "dataset corpus pages (repository_source_path); source files on disk are a larger population, see the qmd record", corpus_pages: $total, corpus_pages_with_a_node: $present, coverage: (if $total == 0 then 0 else $present / $total end)},
        query_interface: {mcp: "graphify-mcp <graph.json>", tool: "query_graph", arguments: {question: "<question text>", token_budget: "default 2000, then 8000 when fewer than ten distinct pages came back"}, node_to_file: "each NODE line of the response, its src in the order the tool lists them, deduplicated; the [!] TRUNCATED marker at 8000 is the truncation"}}' > "$ARMS/$arm-$project.json"
    echo "$arm $project: $nodes nodes, $edges edges in $wall s · $present of $total corpus pages have a node · $ARMS/$arm-$project.json"
  else
    [ ! -f "$graph" ] || { mv "$graph" "$graph.partial"; echo "graph.json quarantined as graph.json.partial (session did not complete: exit $rc, early end $ended_early)" >> "$G/build.err"; }
    rm -f "$G/graph.json"
    jq -n --arg arm "$arm" --arg project "$project" --arg model "$model" --argjson s "$wall" --argjson rc "$rc" --argjson res "$res" --argjson early "$ended_early" --argjson attempts "$attempts" --arg attempt "${G/#$HOME/\~}" \
      '{arm: $arm, project: $project, build: ({wall_s: $s, completed: false, claude_exit: $rc, host_model_alias: $model, attempt_dir: $attempt, ended_through: $early} + $res), failed_attempts: $attempts, note: "did not complete (rule 0.3: recorded, not dropped; the result_tail says why the session ended; any partial graph is quarantined and never scored)"}' > "$ARMS/$arm-$project.json"
    echo "$arm $project: did not complete after $wall s (exit $rc, early end $ended_early) · $ARMS/$arm-$project.json"
    return 1
  fi
}

case "$cmd" in
  build)
    [ ! -e "$G" ] || die "$G exists: a build is done once per freeze (move it aside as $G.attempt<n> to rebuild)"
    mkdir -p "$G/src"
    rsync -a --exclude .markdownattractor --exclude .git "$corpus/" "$G/src/"
    ( cd "$G/src" && graphify install --project --platform claude ) > "$G/install.log" 2>&1 || die "graphify install failed: $G/install.log"
    [ -f "$G/src/.claude/skills/graphify/SKILL.md" ] && [ -f "$G/src/.claude/settings.json" ] || die "graphify install wrote no project skill/hooks"
    cp -R "$G/src/.claude" "$G/installed-dot-claude"   # archived: the skill, hooks and nudge the arm ran with
    unset_nested_session; unset_provider_keys
    prompt="/graphify $G/src --no-viz
You are running headless: there is no user to answer questions. Run the pipeline on the whole path exactly as given, never narrow to a subfolder even if the corpus is large (every arm of this benchmark indexes the whole corpus), never ask for confirmation. There is no API key of any kind and you must not look for one, install packages, or change the graphify installation: you and the subagents you dispatch are the model, exactly as the skill's 'host agent itself is the LLM' path says. Dispatch the semantic subagents as the skill says and wait for their results inside this same run: never schedule a wakeup, a cron or a later check (a headless run ends the moment you do, and the graph is never built). When the pipeline is complete, print the final report."
    t0=$(date +%s); rc=0
    ( cd "$G/src" && CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0 claude --print --setting-sources project --no-session-persistence --model "$model" --max-turns 600 \
        --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config \
        --allowedTools Bash Read Write Edit Glob Grep Agent --disallowedTools ScheduleWakeup CronCreate CronDelete CronList -- "$prompt" ) > "$G/build.jsonl" 2>"$G/build.err" || rc=$?
    echo "claude exited $rc" >> "$G/build.err"
    t1=$(date +%s)
    record_build "$rc" "$((t1 - t0))"
    ;;
  record)
    [ -f "$G/build.jsonl" ] || die "no build transcript under $G"
    rc="$(sed -n 's/^claude exited \([0-9]*\)$/\1/p' "$G/build.err" 2>/dev/null | tail -1)"
    wall="$(jq -r '.build.wall_s // 0' "$ARMS/$arm-$project.json" 2>/dev/null || echo 0)"
    # A graph quarantined by an earlier (mistaken) record is restored before the verdict.
    [ -f "$G/src/graphify-out/graph.json" ] || { [ ! -f "$G/src/graphify-out/graph.json.partial" ] || mv "$G/src/graphify-out/graph.json.partial" "$G/src/graphify-out/graph.json"; }
    [ -f "$G/src/graphify-out/graph.json" ] || { [ ! -f "$G/graph.json" ] || cp "$G/graph.json" "$G/src/graphify-out/graph.json"; }
    record_build "${rc:-0}" "${wall:-0}"
    ;;
  drive)
    out="${1:?out.jsonl}"; split="${2:-dev}"
    safe_target "$out"; [ ! -e "$out" ] || die "$out exists"
    [ "$split" != holdout ] || die "the holdout is sealed (rule 0.2)"
    [ -f "$ARMS/$arm-$project.json" ] && [ "$(jq -r '.build.completed' "$ARMS/$arm-$project.json")" = true ] || die "$arm did not complete on $project: nothing to drive (arms/$arm-$project.json)"
    [ -f "$G/graph.json" ] || die "no $G/graph.json"
    # The graph scored is the archived one: restore it and require the same bytes.
    [ "$(sha256 "$G/graph.json")" = "$(gunzip -c "$ARMS/graphs/$arm-$project.graph.json.gz" | shasum -a 256 | cut -c1-64)" ] || die "$G/graph.json differs from the archived arms/graphs/$arm-$project.graph.json.gz"
    client="${MCP_TIME:-$REPO/target/release/examples/mcp_time}"
    [ -x "$client" ] || die "no MCP client at $client"
    unset_provider_keys
    work="$out.work"; mkdir -p "$work"
    # Question ids: the frozen split (every question of the requested split; the scorer applies
    # eligibility, so an ineligible id costs one unused query and never a score).
    jq -r --arg s "$split" '.questions[] | select(.split == $s) | .id' "$RESULTS/$project/split.json" | while IFS= read -r qid; do
      jq -nc --arg id "$qid" --arg q "$(question_text "$project" "$qid" | tr '\n\r\t' '   ' | sed 's/  */ /g')" '{id: $id, q: $q}'
    done > "$work/queries.jsonl"
    [ -s "$work/queries.jsonl" ] || die "no questions in split $split for $project"
    # `query_graph` answers with text: one `NODE <label> [src=<file> loc=… community=…]` line
    # per node in the tool's order, cut to its token budget (2,000 by default) with a
    # `[!] TRUNCATED` marker. The frozen node → file mapping takes every NODE line's src
    # (up to ` loc=`, so paths with spaces survive) in order, deduplicated. Two passes: the
    # default budget first (what an agent gets), then token_budget 8000 for the questions
    # still short of ten distinct pages under the cut; still short at 8000 is `truncated`.
    client_run() { # queries.jsonl template dump times
      ( cd "$G/src" && MCP_TIME_DUMP="$3" "$client" query_graph "$2" "$1" -- graphify-mcp "$G/graph.json" ) > "$4" 2>>"$out.err"
    }
    pages_of() { jq -c --arg pre "$G/src/" '[(.result.content[]? | select(.type=="text") | .text) // "" | scan("NODE [^\n]*?\\[src=(.*?) loc=") | .[0]] | map(sub("^" + $pre; "") | sub("^\\./"; "")) | reduce .[] as $p ([]; if index([$p]) then . else . + [$p] end)'; }
    cut_of() { jq -r '[(.result.content[]? | select(.type=="text") | .text) // ""] | join("") | test("\\[!\\] TRUNCATED")'; }
    t1='{"question":"{query}"}'; t2='{"question":"{query}","token_budget":8000}'
    client_run "$work/queries.jsonl" "$t1" "$work/dump1.jsonl" "$work/times1.jsonl"
    : > "$work/pass2.jsonl"
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"; n="$(pages_of <<<"$line" | jq 'length')"; cut="$(cut_of <<<"$line")"
      if [ "$n" -lt 10 ] && [ "$cut" = true ]; then jq -c --arg id "$id" 'select(.id == $id)' "$work/queries.jsonl" >> "$work/pass2.jsonl"; fi
    done < "$work/dump1.jsonl"
    if [ -s "$work/pass2.jsonl" ]; then client_run "$work/pass2.jsonl" "$t2" "$work/dump2.jsonl" "$work/times2.jsonl"; else : > "$work/dump2.jsonl"; fi
    jq -n --arg tool query_graph --argjson a1 "$t1" --argjson a2 "$t2" --arg graph "$G/graph.json" --arg sha "$(sha256 "$G/graph.json")" '{tool: $tool, arguments: $a1, second_pass: $a2, server: ["graphify-mcp", $graph], graph_sha256: $sha, latency_note: "first_pass_ms is the first tool call only (no startup, no second pass); cold latency is startup_ms + ms of the first line of times1.jsonl"}' > "$out.request.json"
    while IFS= read -r line; do
      id="$(jq -r .id <<<"$line")"; final="$line"; budget=2000
      if l2="$(jq -c --arg id "$id" 'select(.id == $id)' "$work/dump2.jsonl" | head -1)" && [ -n "$l2" ]; then final="$l2"; budget=8000; fi
      paths="$(pages_of <<<"$final")"; ok="$(jq -r .ok <<<"$final")"; n="$(jq 'length' <<<"$paths")"; cut="$(cut_of <<<"$final")"
      truncated=false; [ "$n" -ge 10 ] || [ "$cut" != true ] || truncated=true
      ms="$(jq -r --arg id "$id" 'select(.id == $id) | .ms' "$work/times1.jsonl" | head -1)"
      jq -nc --arg id "$id" --argjson paths "$paths" --argjson truncated "$truncated" --argjson ok "$ok" --argjson budget "$budget" --argjson ms "${ms:-null}" '{question_id: $id, paths: $paths, truncated: $truncated, request: {tool: "query_graph", token_budget: $budget}, ok: $ok, first_pass_ms: $ms}' >> "$out"
    done < "$work/dump1.jsonl"
    n_err="$(jq -s 'map(select(.ok | not)) | length' "$out")"
    echo "wrote $out ($(grep -c . "$out") rows, $n_err failed calls)"
    [ "$n_err" = 0 ] || echo "WARNING: $n_err failed MCP call(s), scored as empty lists (rule 0.3); see $out.err" >&2
    ;;
  *) die "unknown command $cmd" ;;
esac
