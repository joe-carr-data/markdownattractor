#!/usr/bin/env bash
# The machine-checked preflight a table must pass before it runs (execution plan §2.0):
# frozen inputs unchanged, the binary built from the frozen commit, the embedding model
# files hashed, the store complete (every section carded and embedded), the committed rows
# regenerated exactly from the store, a clean reconstruction from the committed cards that
# scores identically on the whole split, coverage per arm, three activation probes per arm
# with their traces, and the timing boundaries recorded. Every check is listed in the
# report with its outcome; the report is written whatever happens and `passed` is true only
# when every check ran and passed.
#
# Usage: scripts/eval/preflight.sh <table> <project> [--frozen FILE] [--split dev]
#                                  [--arms "mda grep"] [--skip-probes] [--skip-reconstruction]
# Writes evals/results/docsqa/preflight/<table>-<project>.json (+ probes/ and coverage-grep.json
# under evals/results/docsqa/preflight/<table>-<project>/); the large artifacts (the
# reconstructed checkout, its store, the regenerated reports) stay under
# $RUN/preflight/<table>/<project>/<timestamp>/, one directory per attempt, never overwritten.
# Env: REPO, RUN, MDA, MDA_MODEL_DIR (scripts/eval/lib.sh). Exit 0 when passed, 1 otherwise.
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
table="${1:?table (T1, …, or development)}"; project="${2:?project}"; shift 2
frozen="$RESULTS/FROZEN.md"; split=dev; arms="mda grep"; skip_probes=0; skip_recon=0
while [ $# -gt 0 ]; do
  case "$1" in
    --frozen) frozen="$2"; shift 2 ;;
    --split) split="$2"; shift 2 ;;
    --arms) arms="$2"; shift 2 ;;
    --skip-probes) skip_probes=1; shift ;;
    --skip-reconstruction) skip_recon=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
dir="$(project_dir "$project")"; corpus="$RUN/$dir"; data="$RUN/docsqa-data"
committed="$RESULTS/$project"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
A="$RUN/preflight/$table/$project/$stamp"
[ ! -e "$A" ] || { echo "$A exists" >&2; exit 1; }
mkdir -p "$A"
OUT="$RESULTS/preflight/$table-$project"; mkdir -p "$OUT/probes"
report="$RESULTS/preflight/$table-$project.json"
checks='[]'; failed=0; skipped=0
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*"; }
record() { # name status detail [extra-json]
  checks="$(jq -c --arg n "$1" --arg s "$2" --arg d "$3" --argjson x "${4:-{\}}" '. + [{name: $n, status: $s, detail: $d} + $x]' <<<"$checks")"
  case "$2" in FAIL) failed=$((failed + 1)) ;; skipped) skipped=$((skipped + 1)) ;; esac
  log "$2 $1: $3"
}
finish() {
  local passed=false
  [ "$failed" = 0 ] && [ "$skipped" = 0 ] && passed=true
  jq -n --arg table "$table" --arg project "$project" --arg split "$split" --arg stamp "$stamp" --arg attempt "${A/#$HOME/\~}" \
     --arg frozen "${frozen#"$REPO"/}" --arg mda "$("$MDA" --version 2>/dev/null || echo n/a)" --arg src "$(git -C "$REPO" rev-parse HEAD)" \
     --arg hw "$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m) · $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 )) GB · $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)" \
     --argjson checks "$checks" --argjson passed "$passed" --argjson failed "$failed" --argjson skipped "$skipped" \
     '{table: $table, project: $project, split: $split, run_at: $stamp, attempt_dir: $attempt, frozen: $frozen, mda: $mda, source_commit: $src,
       timing: {build_profile: "release", hardware: $hw, boundary: "adapter in-process for the store rows (release build); MCP-server latency for the arm columns is measured by scripts/eval/mcp-time.sh (M2), cold first query reported separately"},
       passed: $passed, failed: $failed, skipped: $skipped, checks: $checks}' > "$report"
  log "report: ${report#"$REPO"/} · passed=$passed failed=$failed skipped=$skipped"
  [ "$passed" = true ]
}
trap 'finish' EXIT

# 1. Environment.
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then record env FAIL "ANTHROPIC_API_KEY is set: every model call goes through the Claude Code login"; else
  if command -v claude >/dev/null && command -v jq >/dev/null; then record env ok "ANTHROPIC_API_KEY unset · claude $(claude --version 2>/dev/null | head -1) · jq $(jq --version)"; else record env FAIL "claude or jq missing from PATH"; fi
fi

# 2. Binary built from the frozen commit (an incremental build is a no-op when up to date).
ver="$("$MDA" --version 2>/dev/null || echo unknown)"
if (cd "$REPO" && cargo build --release -p mda-cli >"$A/build.log" 2>&1); then
  ver="$("$MDA" --version)"
  if grep -q "^- version: $ver\$" "$frozen"; then record build ok "$ver at $(git -C "$REPO" rev-parse --short HEAD)"; else record build FAIL "$ver is not the frozen version ($(sed -n 's/^- version: //p' "$frozen"))"; fi
else record build FAIL "cargo build failed: $(tail -3 "$A/build.log" | tr '\n' ' ')"; fi

# 3. Frozen inputs unchanged (and the code paths unchanged since the frozen commit).
if out="$("$REPO/scripts/eval/freeze.sh" --protocol development --out "$frozen" --check 2>&1)"; then record frozen ok "$out"; else record frozen FAIL "$out"; fi

# 4. Embedding model files.
if diff <(model_sha) "$RESULTS/model.sha" >"$A/model.diff" 2>&1; then record model ok "$(wc -l <"$RESULTS/model.sha" | tr -d ' ') files match evals/results/docsqa/model.sha"; else record model FAIL "model files differ: $(head -3 "$A/model.diff" | tr '\n' ' ')"; fi

# 5. Store complete: every section carded, every card embedded.
status="$("$MDA" --json status --root "$corpus" 2>/dev/null || echo '{}')"
pend="$(jq -r '.counts.pending // "?"' <<<"$status")"; fail="$(jq -r '.counts.failed // "?"' <<<"$status")"
carded="$(jq -r '.embeddings.counts.carded // "?"' <<<"$status")"; embedded="$(jq -r '.embeddings.counts.embedded // "?"' <<<"$status")"
if [ "$pend" = 0 ] && [ "$fail" = 0 ] && [ "$carded" = "$embedded" ] && [ "$carded" != "?" ]; then record store ok "pending 0 · failed 0 · $embedded of $carded carded hashes embedded"; else record store FAIL "pending $pend · failed $fail · embedded $embedded of $carded"; fi

# 6. Regeneration: the committed rows come back exactly from the committed store.
t0=$(date +%s)
if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split "$split" --out "$A/regen" >"$A/regen.json" 2>"$A/regen.err"; then
  d=""
  diff <(jq -S . "$A/regen/coverage.json") <(jq -S . "$committed/coverage.json") >"$A/coverage.diff" || d="coverage "
  diff <(jq -S . "$A/regen/split.json") <(jq -S . "$committed/split.json") >"$A/split.diff" || d="${d}split "
  diff <(normalize_runs "$A/regen/results.json") <(normalize_runs "$committed/results.json") >"$A/results.diff" || d="${d}results"
  complete="$(jq -r '.card_coverage.complete' "$A/regen.json")"
  rows="$(jq -r '[.runs[] | "\(.run): success@5 \(.metrics.success_at_5 | tostring) n \(.metrics.questions)"] | join(" · ")' "$A/regen.json")"
  if [ -z "$d" ] && [ "$complete" = true ]; then record regenerate ok "coverage, split and every row/question identical to evals/results/docsqa/$project ($rows) in $(( $(date +%s) - t0 )) s"
  else record regenerate FAIL "differs: ${d:-card coverage incomplete} (see $A/*.diff); rows: $rows"; fi
  cp "$A/regen/coverage.json" "$OUT/coverage-mda.json"
else record regenerate FAIL "mda eval failed: $(tail -c 300 "$A/regen.err" | tr '\n' ' ')"; fi

# 7. Reconstruction: a clean copy of the checkout, raw-indexed, the committed cards attached,
#    re-embedded with the hashed model files, must score identically on the whole split.
cards="$RESULTS/cards-${ver#mda }-$project.json"
if [ "$skip_recon" = 1 ]; then record reconstruction skipped "--skip-reconstruction"
elif [ ! -f "$cards" ]; then record reconstruction FAIL "no committed cards file $cards"
else
  mkdir -p "$A/reconstruct"
  rsync -a --exclude .markdownattractor --exclude .git "$corpus/" "$A/reconstruct/" 2>"$A/rsync.err" || true
  cp "$corpus/.markdownattractor/config.toml" "$A/config.toml.orig" 2>/dev/null || true
  t1=$(date +%s)
  if "$MDA" index --no-summarize --root "$A/reconstruct" >"$A/recon-index.log" 2>&1; then
    t2=$(date +%s)
    # The embedding setting must match the frozen one; the copy starts from the defaults.
    "$MDA" embeddings local-small --root "$A/reconstruct" >/dev/null 2>&1 || true
    if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$A/reconstruct" --split "$split" --cards "$cards" --out "$A/recon" >"$A/recon.json" 2>"$A/recon.err"; then
      t3=$(date +%s)
      attached="$(jq -r .cards_attached "$A/recon.json")"; complete="$(jq -r '.card_coverage.complete' "$A/recon.json")"
      if diff <(normalize_runs "$A/recon/results.json") <(normalize_runs "$committed/results.json") >"$A/recon.diff" 2>&1 && [ "$complete" = true ]; then
        record reconstruction ok "identical on every row and question · $attached cards attached · raw index $((t2 - t1)) s · attach+embed+score $((t3 - t2)) s" \
          "{\"index_s\": $((t2 - t1)), \"attach_embed_score_s\": $((t3 - t2)), \"cards_attached\": $attached}"
      else record reconstruction FAIL "differs from the committed rows (complete=$complete, $attached attached; $A/recon.diff: $(head -c 400 "$A/recon.diff" | tr '\n' ' '))" \
          "{\"index_s\": $((t2 - t1)), \"attach_embed_score_s\": $((t3 - t2)), \"cards_attached\": $attached}"; fi
    else record reconstruction FAIL "mda eval on the reconstruction failed: $(tail -c 300 "$A/recon.err" | tr '\n' ' ')"; fi
  else record reconstruction FAIL "raw index of the copy failed: $(tail -3 "$A/recon-index.log" | tr '\n' ' ')"; fi
fi

# 8. Coverage of the grep arm: the pages of the dataset corpus that exist as files on disk.
total=0; present=0
while IFS= read -r p; do total=$((total + 1)); [ -f "$corpus/$p" ] && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$data/data/corpus.jsonl")
jq -n --arg project "$project" --argjson total "$total" --argjson present "$present" '{arm: "grep", project: $project, corpus_pages: $total, pages_on_disk: $present, coverage: (if $total == 0 then 0 else ($present / $total) end), note: "grep sees every file in the checkout; coverage is the dataset pages present at the pinned commit"}' > "$OUT/coverage-grep.json"
if [ "$present" = "$total" ] && [ "$total" -gt 0 ]; then record coverage-grep ok "$present of $total corpus pages on disk"; else record coverage-grep FAIL "$present of $total corpus pages on disk"; fi

# 9. Three activation probes per arm, traces kept.
if [ "$skip_probes" = 1 ]; then record probes skipped "--skip-probes"
else
  : > "$OUT/probes/summary.jsonl"
  for arm in $arms; do
    n_ok=0; n=0
    for qid in $(probe_ids "$project"); do
      n=$((n + 1))
      rc=0; line="$("$REPO/scripts/eval/probe.sh" "$arm" "$project" "$qid" "$OUT/probes/$arm-${qid//[^A-Za-z0-9_.-]/_}.jsonl" 2>>"$A/probes.err")" || rc=$?
      [ -n "$line" ] && echo "$line" >> "$OUT/probes/summary.jsonl"
      [ "$rc" = 0 ] && n_ok=$((n_ok + 1))
      log "probe $arm $qid: rc=$rc $(jq -c '.tools' <<<"${line:-null}" 2>/dev/null)"
    done
    if [ "$n_ok" = 3 ] && [ "$n" = 3 ]; then record "probes-$arm" ok "3 of 3 probes activated the arm's tool (traces under ${OUT#"$REPO"/}/probes/)"; else record "probes-$arm" FAIL "$n_ok of $n probes activated ($(tail -c 300 "$A/probes.err" 2>/dev/null | tr '\n' ' '))"; fi
  done
fi
