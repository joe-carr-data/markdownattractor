#!/usr/bin/env bash
# The failure matrix of the T2 harness (M5 plan F1; strategy rule 0.3): every kind of failed
# run is a result that scores 0, fails grounding, is counted in every denominator, contributes
# to no saving, and never yields a zero-versus-zero parity. Synthetic grade rows go through
# `t2.sh status` (completeness against the manifest) and `mda eval --analysis`; the assertions
# are on the analysis output. Run by `make check` (target t2-tests) with the debug binary.
#
# Usage: scripts/eval/tests/failure-matrix.sh [mda-binary]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../../.." && pwd)"
MDA="${1:-${MDA:-$repo/target/debug/mda}}"; [ -x "$MDA" ] || { echo "no mda binary at $MDA (cargo build)" >&2; exit 1; }
tmp="$(mktemp -d -t mda-failure-matrix.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/out/rows"
fail() { echo "FAIL: $*" >&2; exit 1; }

# The manifest: two questions, two arms, two runs each = 8 expected observations.
cat > "$tmp/out/manifest.json" <<'EOF'
{"project":"synthetic","model":"none","runs":2,"arms":["mda","grep"],"question_ids":["q-ok","q-fail"],"questions_file":"none","questions_sha256":"none"}
EOF
# rows: q-ok — both arms complete on both runs (mda better); q-fail — every failure kind
row() { printf '{"id":"%s","arm":"%s","run":%s,%s}\n' "$1" "$2" "$3" "$4"; }
{
  row q-ok mda 1 '"error":false,"score":6,"grounded":true,"source_tokens":100,"tool_calls":2,"cost_usd":0.01'
  row q-ok mda 2 '"error":false,"score":6,"grounded":true,"source_tokens":120,"tool_calls":2,"cost_usd":0.01'
  row q-ok grep 1 '"error":false,"score":4,"grounded":true,"source_tokens":500,"tool_calls":6,"cost_usd":0.05'
  row q-ok grep 2 '"error":false,"score":4,"grounded":true,"source_tokens":600,"tool_calls":6,"cost_usd":0.05'
  row q-fail mda 1 '"error":true,"exit_code":124,"score":null,"grounded":null,"source_tokens":999,"tool_calls":9,"cost_usd":9'     # timed out (its tokens must not count)
  row q-fail mda 2 '"error":false,"score":null,"grounded":null'                                                                      # completed but ungraded (grader gave nothing)
  row q-fail grep 1 '"error":true,"exit_code":1,"score":null,"grounded":null'                                                        # errored
  # q-fail grep 2: missing from the file (the manifest expects it)
} > "$tmp/out/grades.jsonl"

a="$("$MDA" --json eval --analysis "$tmp/out/grades.jsonl" --manifest "$tmp/out/manifest.json" --comparators grep --draws 500 --seed 1)"
ok() { jq -e "$1" <<<"$a" >/dev/null; }   # a jq boolean over the analysis (numbers compared as numbers)

# 1. every failure kind scores 0 for its question, fails grounding, is counted
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "mda") | .score] == [0]' || fail "q-fail mda: median of [timed out, ungraded] must be 0"
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "mda") | .failed] == [2]' || fail "q-fail mda: two failed runs"
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "grep") | .failed] == [2]' || fail "q-fail grep: errored + missing = two failed runs"
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "grep") | .runs] == [2]' || fail "q-fail grep: the manifest's two runs are the denominator"
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "mda") | .grounded] == [0]' || fail "q-fail mda: failed runs fail grounding"
# 2. a failed run's tokens, calls and cost contribute to no saving
ok '[.analysis.questions[] | select(.id == "q-fail" and .arm == "mda") | .source_tokens] == [null]' || fail "a timed-out run's source tokens must not count"
# 3. arm summaries count every run
ok '[.analysis.arms[] | select(.arm == "mda") | [.runs, .failed]] == [[4, 2]]' || fail "mda arm: 4 runs, 2 failed"
ok '[.analysis.arms[] | select(.arm == "grep") | .failed] == [2]' || fail "grep arm: 2 failed (errored + missing)"
ok '[.analysis.arms[] | select(.arm == "mda") | .grounding_rate] == [0.5]' || fail "mda grounding rate = 2 of 4 runs"
# 4. zero-versus-zero is a tie, never parity or a saving; the gates fail on this sample
p="$(jq -c '.analysis.pairs[0]' <<<"$a")"
jq -e '.ties == 1 and .wins == 1' <<<"$p" >/dev/null || fail "q-fail is a 0-vs-0 tie, q-ok a win: $p"
jq -e '.gates.pass == false' <<<"$p" >/dev/null || fail "gates must fail: mean 3.0 < 4.0 and grounding 50%"
jq -e '.savings == null' <<<"$p" >/dev/null || fail "no saving may be claimed when a gate fails"
# 5. the completed-only view exists but does not gate
ok '[.analysis.arms[] | select(.arm == "mda") | .mean_score_completed] == [6]' || fail "completed-only mean over q-ok is 6"
# 6. a duplicate observation is refused (the runner double-counted)
cp "$tmp/out/grades.jsonl" "$tmp/dup.jsonl"; row q-ok mda 1 '"error":false,"score":1' >> "$tmp/dup.jsonl"
if "$MDA" --json eval --analysis "$tmp/dup.jsonl" --no-manifest --comparators grep --draws 10 >/dev/null 2>&1; then fail "a duplicate (question, arm, run) must be refused"; fi
# 6b. rows the manifest does not expect are refused: a run number beyond the run count, an unknown arm
cp "$tmp/out/grades.jsonl" "$tmp/extra.jsonl"; row q-ok mda 3 '"error":false,"score":6' >> "$tmp/extra.jsonl"
if "$MDA" --json eval --analysis "$tmp/extra.jsonl" --manifest "$tmp/out/manifest.json" --comparators grep --draws 10 >/dev/null 2>&1; then fail "run 3 of 2 must be refused"; fi
cp "$tmp/out/grades.jsonl" "$tmp/arm.jsonl"; row q-ok qmd 1 '"error":false,"score":6' >> "$tmp/arm.jsonl"
if "$MDA" --json eval --analysis "$tmp/arm.jsonl" --manifest "$tmp/out/manifest.json" --comparators grep --draws 10 >/dev/null 2>&1; then fail "an arm outside the manifest must be refused"; fi
# 6c. the analysis refuses to run without a manifest unless told it is diagnostic
if "$MDA" --json eval --analysis "$tmp/out/grades.jsonl" --comparators grep --draws 10 >/dev/null 2>&1; then fail "--analysis without --manifest must be refused"; fi
# 7. t2.sh status reports missing and error rows against the manifest
jq -c . "$tmp/out/grades.jsonl" | while IFS= read -r l; do printf '%s' "$l" > "$tmp/out/rows/$(jq -r '"\(.id)-\(.arm)-\(.run)"' <<<"$l").json"; done
st="$(bash "$repo/scripts/eval/t2.sh" status "$tmp/out")"
jq -e '.missing == 1 and .error == 2 and .ok == 5' <<<"$st" >/dev/null || fail "status must report 5 ok, 2 error, 1 missing: $st"
echo "failure matrix: ok (10 checks)"
