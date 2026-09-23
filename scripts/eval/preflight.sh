#!/usr/bin/env bash
# The machine-checked preflight a table must pass before it runs (execution plan §2.0):
# frozen inputs unchanged, the binary built from the frozen commit (the executable Cargo
# reports, hashed), the embedding model files hashed, the store complete (every section
# carded and embedded), the committed rows regenerated from their archived page lists
# without a store (plan §2.0b), the committed store replayed exactly, a clean reconstruction
# from the committed cards that scores identically on the whole split, coverage per arm,
# three activation probes per arm with their traces, and the timing boundaries recorded.
# Every required check is listed in the report with its outcome; a check that did not run
# is recorded as such; the report is written first as "in progress" and `passed` is true
# only when the script ran to completion and every required check passed (Codex M1 F1).
#
# Usage: scripts/eval/preflight.sh <table> <project> [--frozen FILE] [--split dev]
#                                  [--skip-probes] [--skip-reconstruction]
# The arms come from the frozen file's "### Arms" section. Writes
# evals/results/docsqa/preflight/<table>-<project>.json (+ probes/ and coverage-*.json under
# evals/results/docsqa/preflight/<table>-<project>/); the large artifacts (the reconstructed
# checkout, its store, the regenerated reports, a copy of the report and traces) stay under
# $RUN/preflight/<table>/<project>/<timestamp>/, one directory per attempt, never overwritten.
# Env: REPO, RUN, MDA_MODEL_DIR (scripts/eval/lib.sh). Exit 0 when passed, 1 otherwise.
set -euo pipefail
# shellcheck source=scripts/eval/lib.sh
. "$(dirname "$0")/lib.sh"
table="${1:?table (T1, …, or development)}"; project="${2:?project}"; shift 2
ident "$table"; ident "$project"
frozen="$RESULTS/FROZEN.md"; split=dev; skip_probes=0; skip_recon=0
while [ $# -gt 0 ]; do
  case "$1" in
    --frozen) frozen="$2"; shift 2 ;;
    --split) split="$2"; shift 2 ;;
    --skip-probes) skip_probes=1; shift ;;
    --skip-reconstruction) skip_recon=1; shift ;;
    *) die "unknown argument: $1" ;;
  esac
done
[ -f "$frozen" ] || die "no frozen file $frozen"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"; data="$RUN/docsqa-data"
committed="$RESULTS/$project"
arms="$(sed -n '/^### Arms/,/^$/p' "$frozen" | sed -n 's/^- \([A-Za-z0-9._-]*\):.*/\1/p' | tr '\n' ' ')"
[ -n "$arms" ] || die "$frozen lists no arms"
# Activation probes exist for the arms an agent drives through a tool (rule 0.5); a control
# scored from its own ranked lists (BM25-over-files) has no agent interface and no probe.
PROBE_ARMS="mda grep qmd graphify graphify-haiku"
required="env build frozen model store regenerate artifacts replay reconstruction coverage-grep"
probe_arms=""
for a in $arms; do case " $PROBE_ARMS " in *" $a "*) required="$required probes-$a"; probe_arms="$probe_arms $a" ;; esac; done
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
A="$RUN/preflight/$table/$project/$stamp"
[ ! -e "$A" ] || die "$A exists"
OUT="$RESULTS/preflight/$table-$project"
report="$RESULTS/preflight/$table-$project.json"
safe_target "$report"; safe_target "$OUT/probes/summary.jsonl"; safe_target "$A/attempt"
mkdir -p "$A" "$OUT/probes"
checks='[]'; failed=0; skipped=0; completed=0
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*"; }
record() { # name status detail [extra-json]
  local extra="${4:-}"
  [ -n "$extra" ] || extra='{}'
  checks="$(jq -c --arg n "$1" --arg s "$2" --arg d "$3" --argjson x "$extra" '. + [{name: $n, status: $s, detail: $d} + $x]' <<<"$checks")"
  case "$2" in FAIL) failed=$((failed + 1)) ;; skipped) skipped=$((skipped + 1)) ;; esac
  log "$2 $1: $3"
}
write_report() { # status passed
  jq -n --arg table "$table" --arg project "$project" --arg split "$split" --arg stamp "$stamp" --arg attempt "${A/#$HOME/\~}" \
     --arg frozen "${frozen#"$REPO"/}" --arg mda "${MDA_VERSION:-}" --arg mda_sha "${MDA_SHA:-}" --arg src "$(git -C "$REPO" rev-parse HEAD)" --arg arms "$arms" \
     --arg hw "$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m) · $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 )) GB · $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)" \
     --arg status "$1" --argjson passed "$2" --argjson checks "$checks" --argjson failed "$failed" --argjson skipped "$skipped" --arg required "$required" \
     '{table: $table, project: $project, split: $split, run_at: $stamp, status: $status, passed: $passed, attempt_dir: $attempt, frozen: $frozen,
       mda: $mda, mda_sha256: $mda_sha, source_commit: $src, arms: ($arms | split(" ") | map(select(. != ""))), required: ($required | split(" ")),
       timing: {build_profile: "release", hardware: $hw, boundary: "adapter in-process for the store rows (release build); MCP-server latency for the arm columns is measured by scripts/eval/mcp-time.sh (M2), cold first query reported separately"},
       failed: $failed, skipped: $skipped, checks: $checks}' > "$report"
}
write_report "in progress" false
finish() {
  local rc=$? passed=false name
  trap - EXIT
  for name in $required; do
    jq -e --arg n "$name" '.[] | select(.name == $n)' <<<"$checks" >/dev/null || record "$name" FAIL "did not run (the preflight stopped before it)"
  done
  if [ "$rc" = 0 ] && [ "$completed" = 1 ] && [ "$failed" = 0 ] && [ "$skipped" = 0 ]; then passed=true; fi
  write_report "$([ "$completed" = 1 ] && echo completed || echo "interrupted (exit $rc)")" "$passed"
  cp "$report" "$A/report.json" 2>/dev/null || true
  cp -R "$OUT" "$A/published" 2>/dev/null || true
  log "report: ${report#"$REPO"/} · passed=$passed failed=$failed skipped=$skipped"
  [ "$passed" = true ]
}
trap 'finish' EXIT

# 1. Environment.
keys="$( (env | grep -oE '^[A-Z0-9_]*(API_KEY)=' || true) | sed 's/=$//' | tr '\n' ' ')"   # no match is the good case, not an error
if [ -n "$keys" ]; then record env FAIL "provider keys in the environment ($keys): every model call goes through the Claude Code login; unset them (the arm scripts unset them themselves, the preflight refuses to certify with them present)"; else
  if command -v claude >/dev/null && command -v jq >/dev/null && command -v rsync >/dev/null; then record env ok "ANTHROPIC_API_KEY unset · claude $(claude --version 2>/dev/null | head -1) · jq $(jq --version)"; else record env FAIL "claude, jq or rsync missing from PATH"; fi
fi

# 2. The binary: built from this checkout with the lockfile, the executable Cargo reports
#    (not whatever $MDA pointed at), version as frozen, sha256 recorded (Codex M1 F4).
MDA_VERSION=""; MDA_SHA=""
if (cd "$REPO" && cargo build --release --locked -p mda-cli --message-format=json > "$A/build.json" 2>"$A/build.err"); then
  exe="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "mda" and .executable != null) | .executable' "$A/build.json" | tail -1)"
  if [ -n "$exe" ] && [ -x "$exe" ]; then
    MDA="$exe"; MDA_VERSION="$("$MDA" --version)"; MDA_SHA="$(sha256 "$MDA")"
    if grep -q "^- version: $MDA_VERSION\$" "$frozen"; then record build ok "$MDA_VERSION at $(git -C "$REPO" rev-parse --short HEAD) · $exe · sha256 $MDA_SHA" "{\"executable\": \"$exe\", \"sha256\": \"$MDA_SHA\"}"
    else record build FAIL "$MDA_VERSION is not the frozen version ($(sed -n 's/^- version: //p' "$frozen"))"; fi
  else record build FAIL "cargo reported no mda executable"; fi
else record build FAIL "cargo build failed: $(tail -3 "$A/build.err" | tr '\n' ' ')"; fi
export MDA

# 3. Frozen inputs unchanged (and the code paths unchanged since the frozen commit).
if out="$("$REPO/scripts/eval/freeze.sh" --protocol development --out "$frozen" --check 2>&1)"; then record frozen ok "$out"; else record frozen FAIL "$out"; fi

# 4. Embedding model files (regular files and the snapshot links the loader opens).
if model_sha > "$A/model.sha" && diff "$A/model.sha" "$RESULTS/model.sha" >"$A/model.diff" 2>&1; then record model ok "$(wc -l <"$RESULTS/model.sha" | tr -d ' ') entries match evals/results/docsqa/model.sha"; else record model FAIL "model files differ: $(head -3 "$A/model.diff" | tr '\n' ' ')"; fi

# 5. Store complete: every section carded, every card embedded.
status="$("$MDA" --json status --root "$corpus" 2>/dev/null || echo '{}')"
pend="$(jq -r '.counts.pending // "?"' <<<"$status")"; fail="$(jq -r '.counts.failed // "?"' <<<"$status")"
carded="$(jq -r '.embeddings.counts.carded // "?"' <<<"$status")"; embedded="$(jq -r '.embeddings.counts.embedded // "?"' <<<"$status")"
if [ "$pend" = 0 ] && [ "$fail" = 0 ] && [ "$carded" = "$embedded" ] && [ "$carded" != "?" ]; then record store ok "pending 0 · failed 0 · $embedded of $carded carded hashes embedded"; else record store FAIL "pending $pend · failed $fail · embedded $embedded of $carded"; fi

# 6. Regeneration from archived observations (plan §2.0b): each committed run's page lists,
#    fed back as an arm, must give the same metrics and per-question results, no store search.
#    The archive holds ten pages per question; a rank the store found beyond ten (from its
#    deeper fetch) is kept in results.json for the reader but no metric depends on it, so
#    ranks are compared up to ten.
regen_norm() { jq -S --argjson i "${2:-0}" '{run: .runs[$i].run, questions: .runs[$i].metrics.questions, success_at_5: .runs[$i].metrics.success_at_5, mrr_at_5: .runs[$i].metrics.mrr_at_5, ndcg_at_10: .runs[$i].metrics.ndcg_at_10, results: [.runs[$i].results[] | {id, split, rank: (if .rank != null and .rank <= 10 then .rank else null end), ndcg_at_10, pages, truncated}]}' "$1"; }
nruns="$(jq '.runs | length' "$committed/results.json")"; bad=""
for ((i = 0; i < nruns; i++)); do
  name="$(jq -r --argjson i "$i" '.runs[$i].run' "$committed/results.json")"
  archived_rows "$committed/results.json" "$i" > "$A/archived-$i.jsonl"
  if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split "$split" --arm-output "$A/archived-$i.jsonl" --arm-name "$name" > "$A/regen-$i.json" 2>"$A/regen-$i.err" \
     && regen_norm "$A/regen-$i.json" 0 > "$A/regen-$i.norm" && regen_norm "$committed/results.json" "$i" > "$A/committed-$i.norm" \
     && diff "$A/regen-$i.norm" "$A/committed-$i.norm" > "$A/regen-$i.diff"; then :; else bad="$bad '$name'"; fi
done
# External arms too (Codex M2 F1): every committed <project>/arms/<arm>.jsonl scored again must
# give its committed <arm>.results.json (metrics and per-question results).
n_ext=0
for rows in "$committed"/arms/*.jsonl; do
  [ -f "$rows" ] || continue
  a="$(basename "$rows" .jsonl)"; case "$a" in *.times) continue ;; esac   # latency files are not rows
  [ -f "$committed/arms/$a.results.json" ] || { bad="$bad '$a (no results.json)'"; continue; }
  n_ext=$((n_ext + 1))
  name="$(jq -r '.runs[0].run' "$committed/arms/$a.results.json")"
  if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split "$split" --arm-output "$rows" --arm-name "$name" > "$A/regen-$a.json" 2>"$A/regen-$a.err" \
     && regen_norm "$A/regen-$a.json" 0 > "$A/regen-$a.norm" && regen_norm "$committed/arms/$a.results.json" 0 > "$A/committed-$a.norm" \
     && diff "$A/regen-$a.norm" "$A/committed-$a.norm" > "$A/regen-$a.diff"; then :; else bad="$bad '$a'"; fi
done
hashes="$(jq -n --arg r "$(sha256 "$committed/results.json")" --arg c "$(sha256 "$committed/coverage.json")" --arg s "$(sha256 "$committed/split.json")" --argjson n "$n_ext" '{results_sha256: $r, coverage_sha256: $c, split_sha256: $s, external_arm_files: $n}')"
if [ -z "$bad" ]; then record regenerate ok "$nruns committed run(s) and $n_ext external arm file(s) regenerate from their archived page lists (results.json sha256 $(jq -r .results_sha256 <<<"$hashes"))" "$hashes"; else record regenerate FAIL "do not regenerate:$bad (see $A/regen-*.diff)" "$hashes"; fi

# Artifact identity (Codex M2 F1): the live artifacts the arms score from are the frozen
# ones: qmd's index (checkpointed, then hashed) and each graphify configuration's graph
# (the served file equals the archived copy), as FROZEN.md records them.
art_bad=""
for a in $arms; do
  case "$a" in
    qmd) if [ -f "$HOME/.cache/qmd/$project.sqlite" ]; then
           sqlite3 "$HOME/.cache/qmd/$project.sqlite" 'PRAGMA wal_checkpoint(TRUNCATE);' >/dev/null 2>&1 || true
           h="$(sha256 "$HOME/.cache/qmd/$project.sqlite")"; grep -q "qmd-$project.*index sha256 $h" "$frozen" || art_bad="$art_bad qmd(index $h)"
         else art_bad="$art_bad qmd(no index)"; fi ;;
    graphify|graphify-*) rec="$RESULTS/arms/$a-$project.json"
         if [ -f "$rec" ] && [ "$(jq -r '.build.completed' "$rec")" = true ]; then
           g="$RUN/graphify/$project"; [ "$a" = graphify ] || g="$RUN/graphify/$project-${a#graphify-}"
           [ -f "$g/graph.json" ] && [ "$(sha256 "$g/graph.json")" = "$(jq -r .graph.sha256 "$rec")" ] && [ "$(gunzip -c "$RESULTS/arms/graphs/$a-$project.graph.json.gz" | shasum -a 256 | cut -c1-64)" = "$(jq -r .graph.sha256 "$rec")" ] || art_bad="$art_bad $a(graph)"
         fi ;;
  esac
done
if [ -z "$art_bad" ]; then record artifacts ok "live qmd index and graphify graphs match the frozen records"; else record artifacts FAIL "live artifacts differ from the frozen records:$art_bad"; fi

# 7. Replay: the committed rows come back exactly from the committed store.
t0=$(date +%s)
if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$corpus" --split "$split" --out "$A/replay" >"$A/replay.json" 2>"$A/replay.err"; then
  d=""
  jq -S . "$A/replay/coverage.json" > "$A/cov.a"; jq -S . "$committed/coverage.json" > "$A/cov.b"; diff "$A/cov.a" "$A/cov.b" >"$A/coverage.diff" || d="coverage "
  jq -S . "$A/replay/split.json" > "$A/split.a"; jq -S . "$committed/split.json" > "$A/split.b"; diff "$A/split.a" "$A/split.b" >"$A/split.diff" || d="${d}split "
  normalize_runs "$A/replay/results.json" "$A/runs.a"; normalize_runs "$committed/results.json" "$A/runs.b"; diff "$A/runs.a" "$A/runs.b" >"$A/results.diff" || d="${d}results"
  complete="$(jq -r '.card_coverage.complete' "$A/replay.json")"
  rows="$(jq -r '[.runs[] | "\(.run): success@5 \(.metrics.success_at_5 | tostring) n \(.metrics.questions)"] | join(" · ")' "$A/replay.json")"
  if [ -z "$d" ] && [ "$complete" = true ]; then record replay ok "coverage, split and every row/question identical to evals/results/docsqa/$project ($rows) in $(( $(date +%s) - t0 )) s"
  else record replay FAIL "differs: ${d:-card coverage incomplete} (see $A/*.diff); rows: $rows"; fi
  cp "$A/replay/coverage.json" "$OUT/coverage-mda.json"
else record replay FAIL "mda eval failed: $(tail -c 300 "$A/replay.err" | tr '\n' ' ')"; fi

# 8. Reconstruction: a clean copy of the checkout (tracked files only, the frozen config
#    applied, no `.git`), raw-indexed, the committed cards attached, re-embedded with the
#    hashed model files, must score identically on the whole split and index the same
#    documents and sections.
cards="$RESULTS/cards-${MDA_VERSION#mda }-$project.json"
if [ "$skip_recon" = 1 ]; then record reconstruction skipped "--skip-reconstruction"
elif [ ! -f "$cards" ]; then record reconstruction FAIL "no committed cards file $cards"
elif grep -q "(\`$dir/\`).*exclude rules: sha256" "$frozen"; then record reconstruction FAIL "$dir has .git/info/exclude rules; a reconstruction without .git cannot honour them"
else
  mkdir -p "$A/reconstruct/.markdownattractor"
  if rsync -a --exclude .markdownattractor --exclude .git "$corpus/" "$A/reconstruct/" 2>"$A/rsync.err" && cp "$corpus/.markdownattractor/config.toml" "$A/reconstruct/.markdownattractor/config.toml"; then
    t1=$(date +%s)
    if "$MDA" index --no-summarize --root "$A/reconstruct" >"$A/recon-index.log" 2>&1; then
      t2=$(date +%s)
      if "$MDA" --json eval --dataset docsqa --data "$data" --project "$project" --root "$A/reconstruct" --split "$split" --cards "$cards" --out "$A/recon" >"$A/recon.json" 2>"$A/recon.err"; then
        t3=$(date +%s)
        attached="$(jq -r .cards_attached "$A/recon.json")"; complete="$(jq -r '.card_coverage.complete' "$A/recon.json")"
        inv_a="$(jq -c '{docs: .store.docs, sections: .store.sections, carded: .card_coverage.sections_carded}' "$A/recon.json")"
        inv_b="$(jq -c '{docs: .store.docs, sections: .store.sections, carded: .card_coverage.sections_carded}' "$A/replay.json" 2>/dev/null || echo null)"
        normalize_runs "$A/recon/results.json" "$A/recon.a"
        extra="{\"index_s\": $((t2 - t1)), \"attach_embed_score_s\": $((t3 - t2)), \"cards_attached\": $attached, \"inventory\": $inv_a}"
        if diff "$A/recon.a" "$A/runs.b" >"$A/recon.diff" 2>&1 && [ "$complete" = true ] && [ "$inv_a" = "$inv_b" ]; then
          record reconstruction ok "identical on every row and question · same inventory $inv_a · $attached cards attached · raw index $((t2 - t1)) s · attach+embed+score $((t3 - t2)) s" "$extra"
        else record reconstruction FAIL "differs from the committed rows (complete=$complete, inventory $inv_a vs $inv_b, $attached attached; $A/recon.diff: $(head -c 400 "$A/recon.diff" | tr '\n' ' '))" "$extra"; fi
      else record reconstruction FAIL "mda eval on the reconstruction failed: $(tail -c 300 "$A/recon.err" | tr '\n' ' ')"; fi
    else record reconstruction FAIL "raw index of the copy failed: $(tail -3 "$A/recon-index.log" | tr '\n' ' ')"; fi
  else record reconstruction FAIL "copy failed: $(tail -3 "$A/rsync.err" | tr '\n' ' ')"; fi
fi

# 9. Coverage of the grep arm: the pages of the dataset corpus that exist as files on disk.
total=0; present=0
while IFS= read -r p; do total=$((total + 1)); [ -f "$corpus/$p" ] && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$data/data/corpus.jsonl")
jq -n --arg project "$project" --argjson total "$total" --argjson present "$present" '{arm: "grep", project: $project, corpus_pages: $total, pages_on_disk: $present, coverage: (if $total == 0 then 0 else ($present / $total) end), note: "grep sees every file in the checkout; coverage is the dataset pages present at the pinned commit"}' > "$OUT/coverage-grep.json"
if [ "$present" = "$total" ] && [ "$total" -gt 0 ]; then record coverage-grep ok "$present of $total corpus pages on disk"; else record coverage-grep FAIL "$present of $total corpus pages on disk"; fi

# 10. Three activation probes per frozen arm, traces kept (a successful call of the arm's tool).
if [ "$skip_probes" = 1 ]; then for arm in $probe_arms; do record "probes-$arm" skipped "--skip-probes"; done
else
  : > "$OUT/probes/summary.jsonl"
  for arm in $probe_arms; do
    # An arm whose build did not complete on this project has no interface to probe: its
    # record is the evidence (rule 0.3), the probe is not applicable rather than failed.
    if [ -f "$RESULTS/arms/$arm-$project.json" ] && [ "$(jq -r 'if .build.completed == false then "false" else "true" end' "$RESULTS/arms/$arm-$project.json")" = false ]; then
      record "probes-$arm" ok "not applicable: the arm's build did not complete on $project (recorded in arms/$arm-$project.json)"; continue
    fi
    n_ok=0; n=0
    for qid in $(probe_ids "$project"); do
      n=$((n + 1))
      rc=0; line="$("$REPO/scripts/eval/probe.sh" "$arm" "$project" "$qid" "$OUT/probes/$arm-${qid//[^A-Za-z0-9_.-]/_}.jsonl" 2>>"$A/probes.err")" || rc=$?
      [ -n "$line" ] && echo "$line" >> "$OUT/probes/summary.jsonl"
      [ "$rc" = 0 ] && n_ok=$((n_ok + 1))
      log "probe $arm $qid: rc=$rc $(jq -c '.calls' <<<"${line:-null}" 2>/dev/null)"
    done
    if [ "$n_ok" = 3 ] && [ "$n" = 3 ]; then record "probes-$arm" ok "3 of 3 probes made a successful call of the arm's tool (traces under ${OUT#"$REPO"/}/probes/)"; else record "probes-$arm" FAIL "$n_ok of $n probes activated ($(tail -c 300 "$A/probes.err" 2>/dev/null | tr '\n' ' '))"; fi
  done
fi
completed=1
