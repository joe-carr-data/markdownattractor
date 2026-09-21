#!/bin/bash
# Spike harness: summarize one markdown chunk with claude -p. usage: run-section.sh <chunk> <outfile> <promptfile> <stdin|arg>; honours MAX_THINKING_TOKENS
S="$(cd "$(dirname "$0")/../.." && pwd)"   # repo root
cd "$(mktemp -d)"
start=$(date +%s.%N)
common=(--model haiku --system-prompt "$(cat "$3")" --output-format json --json-schema "$(cat $S/prompts/section.schema.v1.json)" --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence)
if [ "$4" = "arg" ]; then
  MARKDOWNATTRACTOR_WORKER=1 claude "${common[@]}" -p -- "$(cat "$1")" > "$2" 2> "$2.err"
else
  MARKDOWNATTRACTOR_WORKER=1 claude "${common[@]}" -p < "$1" > "$2" 2> "$2.err"
fi
rc=$?; end=$(date +%s.%N)
echo "rc=$rc wall=$(echo "$end - $start" | bc)s api=$(jq -r .duration_api_ms "$2" 2>/dev/null)ms turns=$(jq -r .num_turns "$2" 2>/dev/null) in=$(jq -r .usage.input_tokens "$2" 2>/dev/null) out=$(jq -r .usage.output_tokens "$2" 2>/dev/null) think=$(jq -r .usage.output_tokens_details.thinking_tokens "$2" 2>/dev/null) so=$(jq -r '.structured_output != null' "$2" 2>/dev/null) err=$(head -c 80 "$2.err" | tr '\n' ' ')"
