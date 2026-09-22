#!/usr/bin/env bash
# Grade every answer in <out>/runs.jsonl against its reference with Sonnet through the owner's
# own Claude Code login (`claude -p`, benchmark plan §0a.3: no API key anywhere in evals/),
# rubric: correctness 0-3, completeness 0-3. Then apply the parity gate: token savings count
# only for questions where the with-index score >= the baseline score. A run whose grade is
# missing is counted as ungraded (score null) and shown, never as a zero (rule 0.3).
#
# Usage: scripts/eval/grade.sh <questions.jsonl> <out-dir> [grader-model=sonnet]
# Requires: claude on PATH (logged in), jq.
set -euo pipefail
questions="${1:?questions.jsonl}"; out="${2:?out dir}"
model="${3:-${GRADER_MODEL:-sonnet}}"
: > "$out/grades.jsonl"

# Nested-session markers would make claude refuse to start from inside Claude Code.
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done

schema='{"type":"object","properties":{"correctness":{"type":"integer","minimum":0,"maximum":3},"completeness":{"type":"integer","minimum":0,"maximum":3},"note":{"type":"string"}},"required":["correctness","completeness","note"],"additionalProperties":false}'
scratch="$(mktemp -d)"; trap 'rm -rf "$scratch"' EXIT

grade_one() { # question reference answer -> JSON object or empty
  local prompt
  prompt="$(jq -n --arg q "$1" --arg ref "$2" --arg ans "$3" -r '
    "You grade an answer to a question about a documentation corpus against a reference answer.\n" +
    "Score correctness 0-3 (3: every claim agrees with the reference; 0: wrong or fabricated) and " +
    "completeness 0-3 (3: covers every point of the reference; 0: none). Judge only against the reference.\n\n" +
    "Question: " + $q + "\n\nReference: " + $ref + "\n\nAnswer: " + $ans + "\n\n" +
    "Reply through the StructuredOutput tool only: {\"correctness\": n, \"completeness\": n, \"note\": \"one line\"}"')"
  (cd "$scratch" && MAX_THINKING_TOKENS=0 claude --print --model "$model" --setting-sources "" --strict-mcp-config \
      --no-session-persistence --max-turns 2 --output-format json --json-schema "$schema" -- "$prompt" </dev/null 2>/dev/null) \
    | jq -c 'if (.structured_output? // null) != null then .structured_output else empty end' | head -1
}

jq -c '.' "$out/runs.jsonl" | while IFS= read -r run; do
  id="$(jq -r .id <<<"$run")"
  ref="$(jq -r --arg id "$id" 'select(.id==$id) | .reference' "$questions")"
  g="$(grade_one "$(jq -r .q <<<"$run")" "$ref" "$(jq -r .answer <<<"$run")" || true)"
  [ -n "$g" ] || g='{"correctness":null,"completeness":null,"note":"grader returned no structured output"}'
  jq -c --argjson g "$g" '. + {grade: $g, score: (if ($g.correctness != null and $g.completeness != null) then ($g.correctness + $g.completeness) else null end)}' <<<"$run" >> "$out/grades.jsonl"
  printf '%s %s run %s: %s\n' "$id" "$(jq -r .arm <<<"$run")" "$(jq -r .run <<<"$run")" "$(tail -1 "$out/grades.jsonl" | jq -c '.score')"
done

# Parity table: per question, conventional median over runs of each arm (rule 0.7). A question
# with an ungraded run on either side is shown and excluded from the parity count.
jq -s -r '
  def median: sort | if length == 0 then null elif length % 2 == 1 then .[length/2|floor] else (.[length/2-1] + .[length/2]) / 2 end;
  group_by(.id) | map({
    id: .[0].id,
    base: (map(select(.arm=="baseline"))),
    idx: (map(select(.arm=="index")))
  } | . + {
    ungraded: ((.base + .idx) | map(select(.score == null)) | length),
    base_score: (.base | map(.score | select(. != null)) | median),
    idx_score: (.idx | map(.score | select(. != null)) | median),
    base_tokens: (.base | map(.input_tokens) | median),
    idx_tokens: (.idx | map(.input_tokens) | median),
    base_source: (.base | map(.source_tokens) | median),
    idx_source: (.idx | map(.source_tokens) | median),
    base_calls: (.base | map(.tool_calls) | median),
    idx_calls: (.idx | map(.tool_calls) | median),
    base_cost: (.base | map(.cost_usd) | median),
    idx_cost: (.idx | map(.cost_usd) | median)
  } | . + {parity: (.ungraded == 0 and .idx_score != null and .base_score != null and .idx_score >= .base_score)}
  | del(.base, .idx)) |
  ["| id | baseline score | index score | parity | source tokens baseline | source tokens index | input tokens baseline | input tokens index | calls baseline | calls index |", "|---|---|---|---|---|---|---|---|---|---|"] +
  map("| \(.id) | \(.base_score // "ungraded")/6 | \(.idx_score // "ungraded")/6 | \(if .ungraded > 0 then "**ungraded**" elif .parity then "yes" else "**no**" end) | \(.base_source) | \(.idx_source) | \(.base_tokens) | \(.idx_tokens) | \(.base_calls) | \(.idx_calls) |")
  + ["", "Parity gate: \(map(select(.parity)) | length) of \(length) questions at parity (\(map(select(.ungraded > 0)) | length) with an ungraded run). On parity questions, median source tokens read: baseline \(map(select(.parity)) | map(.base_source) | median) vs index \(map(select(.parity)) | map(.idx_source) | median); median total input tokens: baseline \(map(select(.parity)) | map(.base_tokens) | median) vs index \(map(select(.parity)) | map(.idx_tokens) | median). Tokens here are the runner'"'"'s estimate (chars/4 for source tokens; the CLI'"'"'s usage for input tokens)."]
  | .[]' "$out/grades.jsonl" | tee "$out/parity.md"
