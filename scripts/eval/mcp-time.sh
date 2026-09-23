#!/usr/bin/env bash
# Latency through each arm's MCP server with one client (execution plan §2.7): the server is
# started cold, the first query is the cold number (process start and model load included),
# the rest are warm. The client is `crates/mda-cli/examples/mcp_time.rs` (rmcp, stdio).
#
# Usage: scripts/eval/mcp-time.sh <arm: mda|qmd|graphify> <project> <out.jsonl> [split=dev]
# Writes one JSON line per question: {id, ms, ok, bytes} (+ startup_ms and cold on the first).
# The request each arm receives is recorded in <out>.request.json (the tool and the argument
# template with {query} for the question text), so the table can quote it verbatim.
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
arm="${1:?mda|qmd|graphify}"; project="${2:?project}"; out="${3:?out.jsonl}"; split="${4:-dev}"
ident "$arm"; ident "$project"
safe_target "$out"; [ ! -e "$out" ] || die "$out exists"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"
client="${MCP_TIME:-$REPO/target/release/examples/mcp_time}"
[ -x "$client" ] || die "no client at $client: cargo build --release --example mcp_time"
queries="$out.queries.jsonl"
jq -r --arg s "$split" '.runs[0].results[] | select(.split == $s) | .id' "$RESULTS/$project/results.json" | while IFS= read -r qid; do
  jq -nc --arg id "$qid" --arg q "$(question_text "$project" "$qid" | tr '\n\r\t' '   ' | sed 's/  */ /g')" '{id: $id, q: $q}'
done > "$queries"
case "$arm" in
  mda) tool=mda_search; template='{"query":"{query}","k":10}'; server=("$MDA" mcp --root "$corpus") ;;
  qmd) tool=query; template='{"query":"{query}","limit":20}'; server=(qmd --index "$project" mcp) ;;
  graphify) tool=query_graph; template='{"query":"{query}"}'; server=(graphify-mcp) ;;
  *) die "unknown arm $arm" ;;
esac
jq -n --arg arm "$arm" --arg tool "$tool" --argjson template "$template" --args '{arm: $arm, tool: $tool, arguments: $template, server: $ARGS.positional}' -- "${server[@]}" > "$out.request.json"
( cd "$corpus" && "$client" "$tool" "$template" "$queries" -- "${server[@]}" ) > "$out" 2>"$out.err"
[ -s "$out.err" ] || rm -f "$out.err"
n="$(grep -c . "$out")"; ok="$(jq -s 'map(select(.ok)) | length' "$out")"
echo "$arm/$project: $n queries, $ok ok; cold $(jq -r 'select(.cold) | "\(.startup_ms) ms startup + \(.ms) ms"' "$out") · warm median $(jq -s 'map(select(.cold | not) | .ms) | sort | if length == 0 then null elif length % 2 == 1 then .[length/2|floor] else (.[length/2-1] + .[length/2]) / 2 end' "$out") ms → $out"
