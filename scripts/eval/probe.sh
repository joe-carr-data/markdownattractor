#!/usr/bin/env bash
# One activation probe (strategy rule 0.5, execution plan §2.0): run a dataset question
# through headless `claude -p` with one arm's tool set and keep the transcript as the trace
# proving the arm's tool was actually used. "Used" means a call of the arm's tool that
# returned a non-error result (a request that was denied or failed does not count, Codex M1
# F5). Three passing probes per arm and project are required before a table runs.
#
# Usage: scripts/eval/probe.sh <arm: mda|grep> <project> <question_id> <trace.jsonl> [model=sonnet]
# Prints one JSON line (arm, project, question, tools called with their outcome, activated,
# turns, cost, the effective launch configuration) and exits 0 when the arm activated, 3 when
# it did not, 1 on a run error. Holdout questions are refused (rule 0.2).
# Every model call goes through the owner's own Claude Code login (plan §0a.3).
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
arm="${1:?arm (mda|grep)}"; project="${2:?project}"; qid="${3:?question_id}"; trace="${4:?trace.jsonl}"
model="${5:-sonnet}"
ident "$arm"; ident "$project"
corpus="$RUN/$(project_dir "$project")"
[ -d "$corpus" ] || die "no checkout at $corpus"
[ "$(question_split "$project" "$qid")" != holdout ] || die "question $qid is in the sealed holdout (rule 0.2)"
q="$(question_text "$project" "$qid")"
[ -n "$q" ] || die "question $qid of $project has no text"
safe_target "$trace"; safe_target "$trace.err"
trace="$(cd "$(dirname "$trace")" && pwd)/$(basename "$trace")"
unset_nested_session; unset_provider_keys
mcp_cfg=""
skill=""
trap '[ -z "$mcp_cfg" ] || rm -f "$mcp_cfg"; [ -z "$skill" ] || rm -f "$skill"' EXIT
setting_sources=""
common=(--print --no-session-persistence --model "$model" --max-turns 12
        --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config)
case "$arm" in
  mda)
    mcp_cfg="$(mktemp -t mda-probe-mcp.XXXXXX)"
    jq -n --arg cmd "$MDA" --arg root "$corpus" --arg models "$MDA_MODEL_DIR" \
      '{mcpServers: {markdownattractor: {command: $cmd, args: ["mcp"], env: {MDA_ROOT: $root, MDA_MODEL_DIR: $models}}}}' > "$mcp_cfg"
    args=(--mcp-config "$mcp_cfg" --tools Read Grep Glob --allowedTools Read Grep Glob "mcp__markdownattractor__*"
          --append-system-prompt-file "$REPO/skills/search-first/SKILL.md")
    want='^mcp__markdownattractor__mda_search$' ;;
  grep)
    args=(--tools Read Grep Glob --allowedTools Read Grep Glob)
    want='^(Grep|Read|Glob)$' ;;
  qmd)
    # qmd's own MCP server on the project's index, with qmd's own agent skill as the
    # instructions (`qmd skills get qmd --full`, archived beside the trace), the counterpart
    # of mda's search-first rules; the tools are the ones the skill allows.
    [ -f "$HOME/.config/qmd/$project.yml" ] || die "no qmd index for $project (scripts/eval/arms/qmd.sh build)"
    mcp_cfg="$(mktemp -t mda-probe-mcp.XXXXXX)"
    jq -n --arg project "$project" '{mcpServers: {qmd: {command: "qmd", args: ["--index", $project, "mcp"]}}}' > "$mcp_cfg"
    skill="$(mktemp -t qmd-skill.XXXXXX)"; qmd skills get qmd --full > "$skill" 2>/dev/null || die "qmd skills get failed"
    args=(--mcp-config "$mcp_cfg" --tools Read Grep Glob --allowedTools Read Grep Glob "mcp__qmd__*"
          --append-system-prompt-file "$skill")
    want='^mcp__qmd__query$' ;;
  graphify)
    # graphify's MCP server on the archived graph, run from the checkout COPY the graph was
    # built on (its project-level .claude/ holds graphify's skill, PreToolUse hooks and
    # CLAUDE.md nudge: --setting-sources project loads them, so the hook is live).
    G="$RUN/graphify/$project"
    [ -f "$G/graph.json" ] && [ -f "$G/src/.claude/settings.json" ] || die "no graphify build for $project (scripts/eval/arms/graphify.sh build)"
    corpus="$G/src"
    mcp_cfg="$(mktemp -t mda-probe-mcp.XXXXXX)"
    jq -n --arg graph "$G/graph.json" '{mcpServers: {graphify: {command: "graphify-mcp", args: [$graph]}}}' > "$mcp_cfg"
    args=(--mcp-config "$mcp_cfg" --tools Read Grep Glob --allowedTools Read Grep Glob "mcp__graphify__*")
    setting_sources=project
    want='^mcp__graphify__(query_graph|get_node|get_neighbors|get_community|god_nodes|shortest_path)$' ;;
  *) die "unknown arm $arm (mda|grep|qmd|graphify)" ;;
esac
preamble="Answer from the documents in the current directory. Be concise (at most 6 lines). Cite the file and section you used."
launch="$(jq -n --arg model "$model" --arg cmd "$MDA" --arg root "$corpus" --arg rules "$REPO/skills/search-first/SKILL.md" --arg arm "$arm" --arg project "$project" --arg sources "$setting_sources" \
  --arg skill_sha "$( [ -n "$skill" ] && shasum -a 256 "$skill" | cut -c1-64 || true)" \
  --args '{claude_flags: ($ARGS.positional + ["--setting-sources", $sources]), model: $model, cwd: $root,
           mcp: (if $arm == "mda" then {server: "markdownattractor", command: $cmd, args: ["mcp"], env: {MDA_ROOT: $root, MDA_MODEL_DIR: env.MDA_MODEL_DIR}}
                 elif $arm == "qmd" then {server: "qmd", command: "qmd", args: ["--index", $project, "mcp"]}
                 elif $arm == "graphify" then {server: "graphify", command: "graphify-mcp", args: [($root + "/../graph.json")], project_settings: ($root + "/.claude")} else null end),
           system_prompt: (if $arm == "mda" then {file: $rules} elif $arm == "qmd" then {source: "qmd skills get qmd --full", sha256: $skill_sha} elif $arm == "graphify" then {source: "project .claude/ written by graphify install --project (skill, hooks, CLAUDE.md)"} else null end)}' -- "${common[@]}" "${args[@]}")"
t0=$(date +%s); rc=0
(cd "$corpus" && claude "${common[@]}" --setting-sources "$setting_sources" "${args[@]}" -- "$preamble $q" </dev/null > "$trace" 2>"$trace.err") || rc=$?
t1=$(date +%s)
[ -s "$trace.err" ] || rm -f "$trace.err"
summary="$(jq -c -s --arg arm "$arm" --arg project "$project" --arg qid "$qid" --arg want "$want" --argjson rc "$rc" --argjson wall "$((t1 - t0))" --arg trace "${trace#"$REPO"/}" --argjson launch "$launch" '
  (map(select(.type=="result")) | last) as $res |
  (map(select(.type=="assistant")) | map(.message.content[]? | select(.type=="tool_use") | {id, name})) as $uses |
  (map(select(.type=="user")) | map(.message.content[]? | select(type=="object" and .type=="tool_result") | {id: .tool_use_id, error: (.is_error // false)})) as $results |
  ($uses | map(. as $u | {name: $u.name, ok: (($results | map(select(.id == $u.id and (.error | not))) | length) > 0)})) as $calls |
  (map(select(.type=="assistant")) | map(.message.model // empty) | unique) as $models |
  {arm: $arm, project: $project, question_id: $qid, calls: $calls, tools: ($calls | map(.name)),
   activated: (($calls | map(select((.name | test($want)) and .ok)) | length) > 0),
   error: (($res == null) or ($res.is_error // false) or ($rc != 0) or (($res.result // "") | length == 0)),
   exit_code: $rc, turns: ($res.num_turns // null), cost_usd_list_price: ($res.total_cost_usd // null),
   models: $models, wall_s: $wall, trace: $trace, launch: $launch}' "$trace")"
echo "$summary"
if [ "$(jq -r .error <<<"$summary")" = true ]; then
  echo "probe $arm/$project/$qid: run error (exit $rc): $(tail -c 300 "$trace.err" 2>/dev/null | tr '\n' ' ')" >&2; exit 1
fi
[ "$(jq -r .activated <<<"$summary")" = true ] || { echo "probe $arm/$project/$qid: no successful call of the arm's tool ($want) in the trace; calls: $(jq -c .calls <<<"$summary")" >&2; exit 3; }
