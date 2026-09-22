#!/usr/bin/env bash
# Grade every answer in <out>/runs.jsonl against its reference with Sonnet through the
# Messages API (rubric: correctness 0-3, completeness 0-3), then apply the parity gate:
# token savings count only for questions where the with-index score >= the baseline score.
#
# Usage: scripts/eval/grade.sh <questions.jsonl> <out-dir>
# Requires: ANTHROPIC_API_KEY (and ANTHROPIC_WORKSPACE_ID when the key needs it), jq, curl.
set -euo pipefail
questions="${1:?questions.jsonl}"; out="${2:?out dir}"
: "${ANTHROPIC_API_KEY:?}"
model="${GRADER_MODEL:-claude-sonnet-5}"
ws_header=()
[ -n "${ANTHROPIC_WORKSPACE_ID:-}" ] && ws_header=(-H "anthropic-workspace-id: $ANTHROPIC_WORKSPACE_ID")
: > "$out/grades.jsonl"

grade_one() { # id arm run question reference answer
  local prompt
  prompt="$(jq -n --arg q "$4" --arg ref "$5" --arg ans "$6" -r '
    "You grade an answer to a question about a team knowledge base against a reference answer.\n" +
    "Score correctness 0-3 (3: every claim agrees with the reference; 0: wrong or fabricated) and " +
    "completeness 0-3 (3: covers every point of the reference; 0: none). Judge only against the reference.\n\n" +
    "Question: " + $q + "\n\nReference: " + $ref + "\n\nAnswer: " + $ans + "\n\n" +
    "Reply with JSON only: {\"correctness\": n, \"completeness\": n, \"note\": \"one line\"}"')"
  curl -sS https://api.anthropic.com/v1/messages \
    -H "x-api-key: $ANTHROPIC_API_KEY" -H "anthropic-version: 2023-06-01" -H "content-type: application/json" "${ws_header[@]}" \
    -d "$(jq -n --arg m "$model" --arg p "$prompt" '{model: $m, max_tokens: 200, messages: [{role: "user", content: $p}]}')" \
    | jq -r '.content[0].text' | sed -n 's/.*\({.*}\).*/\1/p' | head -1
}

jq -c '.' "$out/runs.jsonl" | while IFS= read -r run; do
  id="$(jq -r .id <<<"$run")"
  ref="$(jq -r --arg id "$id" 'select(.id==$id) | .reference' "$questions")"
  g="$(grade_one "$id" "$(jq -r .arm <<<"$run")" "$(jq -r .run <<<"$run")" "$(jq -r .q <<<"$run")" "$ref" "$(jq -r .answer <<<"$run")")"
  [ -n "$g" ] || g='{"correctness":null,"completeness":null,"note":"grader returned no JSON"}'
  jq -c --argjson g "$g" '. + {grade: $g, score: (($g.correctness // 0) + ($g.completeness // 0))}' <<<"$run" >> "$out/grades.jsonl"
done

# Parity table: per question, median over runs of each arm.
jq -s -r '
  group_by(.id) | map({
    id: .[0].id,
    base: (map(select(.arm=="baseline"))),
    idx: (map(select(.arm=="index")))
  } | . + {
    base_score: (.base | map(.score) | sort | .[length/2|floor]),
    idx_score: (.idx | map(.score) | sort | .[length/2|floor]),
    base_tokens: (.base | map(.input_tokens) | sort | .[length/2|floor]),
    idx_tokens: (.idx | map(.input_tokens) | sort | .[length/2|floor]),
    base_source: (.base | map(.source_tokens) | sort | .[length/2|floor]),
    idx_source: (.idx | map(.source_tokens) | sort | .[length/2|floor]),
    base_calls: (.base | map(.tool_calls) | sort | .[length/2|floor]),
    idx_calls: (.idx | map(.tool_calls) | sort | .[length/2|floor]),
    base_cost: (.base | map(.cost_usd) | sort | .[length/2|floor]),
    idx_cost: (.idx | map(.cost_usd) | sort | .[length/2|floor])
  } | . + {parity: (.idx_score >= .base_score)}
  | del(.base, .idx)) |
  ["| id | baseline score | index score | parity | source tokens baseline | source tokens index | input tokens baseline | input tokens index | calls baseline | calls index |", "|---|---|---|---|---|---|---|---|---|---|"] +
  map("| \(.id) | \(.base_score)/6 | \(.idx_score)/6 | \(if .parity then "yes" else "**no**" end) | \(.base_source) | \(.idx_source) | \(.base_tokens) | \(.idx_tokens) | \(.base_calls) | \(.idx_calls) |")
  + ["", "Parity gate: \(map(select(.parity)) | length) of \(length) questions at parity. On parity questions, median source tokens read: baseline \(map(select(.parity)) | map(.base_source) | sort | .[length/2|floor]) vs index \(map(select(.parity)) | map(.idx_source) | sort | .[length/2|floor]); median total input tokens: baseline \(map(select(.parity)) | map(.base_tokens) | sort | .[length/2|floor]) vs index \(map(select(.parity)) | map(.idx_tokens) | sort | .[length/2|floor])."]
  | .[]' "$out/grades.jsonl" | tee "$out/parity.md"
