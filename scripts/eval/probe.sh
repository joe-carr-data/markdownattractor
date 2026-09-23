#!/usr/bin/env bash
# One activation probe (strategy rule 0.5, execution plan §2.0): run a dataset question
# through headless `claude -p` with one arm's tool set and keep the transcript as the trace
# proving the arm's tool was actually used. Three passing probes per arm and project are
# required before a table runs; a probe whose trace shows no call of the arm's tool fails.
#
# Usage: scripts/eval/probe.sh <arm: mda|grep> <project> <question_id> <trace.jsonl> [model=sonnet]
# Prints one JSON line (arm, project, question, tools called, activated, turns, cost) and
# exits 0 when the arm activated, 3 when it did not, 1 on a run error.
# Every model call goes through the owner's own Claude Code login (plan §0a.3).
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
arm="${1:?arm (mda|grep)}"; project="${2:?project}"; qid="${3:?question_id}"; trace="${4:?trace.jsonl}"
model="${5:-sonnet}"
corpus="$RUN/$(project_dir "$project")"
[ -d "$corpus" ] || { echo "no checkout at $corpus" >&2; exit 1; }
q="$(question_text "$project" "$qid")"
[ -n "$q" ] || { echo "question $qid not in the dataset" >&2; exit 1; }
mkdir -p "$(dirname "$trace")"; trace="$(cd "$(dirname "$trace")" && pwd)/$(basename "$trace")"
[ ! -L "$trace" ] || { echo "$trace is a symlink" >&2; exit 1; }
unset_nested_session
common=(--print --setting-sources "" --no-session-persistence --model "$model" --max-turns 12
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
  *) echo "unknown arm $arm (mda|grep; qmd and graphify probes come with their arms)" >&2; exit 2 ;;
esac
preamble="Answer from the documents in the current directory. Be concise (at most 6 lines). Cite the file and section you used."
t0=$(date +%s); rc=0
(cd "$corpus" && claude "${common[@]}" "${args[@]}" -- "$preamble $q" </dev/null > "$trace" 2>"$trace.err") || rc=$?
t1=$(date +%s)
[ "$arm" != mda ] || rm -f "$mcp_cfg"
summary="$(jq -c -s --arg arm "$arm" --arg project "$project" --arg qid "$qid" --arg want "$want" --argjson rc "$rc" --argjson wall "$((t1 - t0))" --arg trace "${trace#"$REPO"/}" '
  (map(select(.type=="result")) | last) as $res |
  (map(select(.type=="assistant")) | map(.message.content[]? | select(.type=="tool_use") | .name)) as $tools |
  (map(select(.type=="assistant")) | map(.message.model // empty) | unique) as $models |
  {arm: $arm, project: $project, question_id: $qid, tools: $tools,
   activated: (($tools | map(select(test($want))) | length) > 0),
   error: (($res == null) or ($res.is_error // false) or ($rc != 0) or (($res.result // "") | length == 0)),
   exit_code: $rc, turns: ($res.num_turns // null), cost_usd_list_price: ($res.total_cost_usd // null),
   models: $models, wall_s: $wall, trace: $trace}' "$trace")"
echo "$summary"
if [ "$(jq -r .error <<<"$summary")" = true ]; then
  echo "probe $arm/$project/$qid: run error (exit $rc): $(tail -c 300 "$trace.err" 2>/dev/null | tr '\n' ' ')" >&2; exit 1
fi
[ "$(jq -r .activated <<<"$summary")" = true ] || { echo "probe $arm/$project/$qid: the trace shows no call of the arm's tool ($want); tools: $(jq -c .tools <<<"$summary")" >&2; exit 3; }
