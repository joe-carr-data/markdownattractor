#!/usr/bin/env bash
# A/B protocol runner (plan §11): every question through headless `claude -p`, once WITHOUT
# the index (Read/Grep/Glob over the corpus) and once WITH it (the mda MCP server plus the
# search-first rules), N runs each. Records tokens, tool calls, wall-clock and cost per run
# into <out>/runs.jsonl, plus "source tokens": the size of everything the tools returned
# (file contents in the baseline, cards and sections with the index), which is what G3 is
# about; total input tokens also count the system prompt and tool schemas. Grading is
# scripts/eval/grade.sh.
#
# Usage: scripts/eval/ab.sh <corpus-dir> <questions.jsonl> <out-dir> [runs=1] [model=sonnet]
# The corpus must already be indexed with cards (`mda index <corpus>`); the daemon may run.
# Requires: claude on PATH (this is the owner's own Claude Code session, ordinary use), jq.
set -euo pipefail
corpus="$(cd "${1:?corpus dir}" && pwd)"
questions="${2:?questions.jsonl}"
out="${3:?out dir}"
runs="${4:-1}"
model="${5:-sonnet}"
mkdir -p "$out"
mda="${MDA_BIN:-$(command -v mda || echo "$(dirname "$0")/../../target/release/mda")}"
[ -x "$mda" ] || { echo "mda binary not found (set MDA_BIN)" >&2; exit 1; }

# Nested-session markers would make claude refuse to start from inside Claude Code.
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done

mcp_cfg="$out/mcp.json"
jq -n --arg cmd "$mda" --arg root "$corpus" \
  '{mcpServers: {markdownattractor: {command: $cmd, args: ["mcp"], env: {MDA_ROOT: $root, MDA_MODEL_DIR: (env.HOME + "/.cache/markdownattractor/models")}}}}' > "$mcp_cfg"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
rules="${MDA_RULES:-$repo/skills/search-first/SKILL.md}"
[ -f "$rules" ] || { echo "rules file not found: $rules" >&2; exit 1; }

common=(--print --setting-sources "" --no-session-persistence --model "$model" --max-turns 12
        --output-format stream-json --verbose --permission-mode dontAsk)
baseline=(--strict-mcp-config --tools Read Grep Glob --allowedTools Read Grep Glob)
withindex=(--mcp-config "$mcp_cfg" --strict-mcp-config --tools Read Grep Glob
           --allowedTools Read Grep Glob "mcp__markdownattractor__*"
           --append-system-prompt-file "$rules")
preamble="Answer from the documents in the current directory. Be concise (at most 6 lines). Cite the file and section you used."

jq -c '.' "$questions" | while IFS= read -r qline; do
  id="$(jq -r .id <<<"$qline")"; q="$(jq -r .q <<<"$qline")"
  for arm in baseline index; do
    for ((r = 1; r <= runs; r++)); do
      log="$out/$id-$arm-$r.jsonl"
      if [ "$arm" = baseline ]; then args=("${baseline[@]}"); else args=("${withindex[@]}"); fi
      t0=$(date +%s.%N)
      rc=0
      (cd "$corpus" && claude "${common[@]}" "${args[@]}" -- "$preamble $q" </dev/null > "$log" 2>"$log.err") || rc=$?
      t1=$(date +%s.%N)
      # Fail loud (plan rule 0.3): a run without a result event is an error row, never a silent zero.
      stderr_tail="$(tail -c 300 "$log.err" 2>/dev/null | tr '\n' ' ')"
      jq -c --arg id "$id" --arg arm "$arm" --argjson run "$r" --arg q "$q" --argjson rc "$rc" --arg stderr "$stderr_tail" \
         --argjson wall "$(echo "$t1 - $t0" | bc)" \
         -s '
        (map(select(.type=="result")) | last) as $res |
        (map(select(.type=="assistant")) | map(.message.content[]? | select(.type=="tool_use") | .name)) as $tools |
        # Source tokens read: what the tools handed back (file contents, cards, sections), ~4 chars per token.
        ((map(select(.type=="user")) | map(.message.content[]? | select(type=="object" and .type=="tool_result") | .content
             | if type=="string" then . else (map(.text? // "") | join("")) end | length) | add // 0) / 4 | floor) as $source |
        {id: $id, arm: $arm, run: $run, q: $q, wall_s: $wall,
         answer: ($res.result // ""), turns: ($res.num_turns // null), cost_usd: ($res.total_cost_usd // null),
         input_tokens: (($res.usage.input_tokens // 0) + ($res.usage.cache_read_input_tokens // 0) + ($res.usage.cache_creation_input_tokens // 0)),
         output_tokens: ($res.usage.output_tokens // 0),
         source_tokens: $source, tool_calls: ($tools | length), tools: $tools,
         error: (($res == null) or ($res.is_error // false) or ($rc != 0)), exit_code: $rc,
         stderr: (if (($res == null) or ($rc != 0)) then $stderr else null end)}' "$log" >> "$out/runs.jsonl"
      last="$(tail -1 "$out/runs.jsonl")"
      if [ "$(jq -r .error <<<"$last")" = true ]; then
        printf '%s %s run %s: ERROR (exit %s): %s\n' "$id" "$arm" "$r" "$rc" "$(jq -r '.stderr // ""' <<<"$last")" >&2
      else
        printf '%s %s run %s: %s tool calls, $%s\n' "$id" "$arm" "$r" "$(jq .tool_calls <<<"$last")" "$(jq .cost_usd <<<"$last")"
      fi
    done
  done
done
errors="$(jq -s 'map(select(.error)) | length' "$out/runs.jsonl")"
echo "runs written to $out/runs.jsonl ($errors error row(s))"
[ "$errors" = 0 ] || exit 2
