#!/usr/bin/env bash
# The tuning loop of the execution plan §3, one trial at a time: apply one pre-declared
# single change to the four DocsQA stores' config.toml (development numbers, dev split only),
# score the raw / cards / hybrid rows on each project, append the trial to
# evals/results/docsqa/TUNING.md with the objective (mean success@5 of the hybrid row over
# the four projects, equal weights), the guardrail (no project below the reference by more
# than 0.02), latency and elapsed time, then restore the previous config. The greedy
# decision (keep if the objective improves by ≥ 0.01 and the guardrail holds) is written by
# the caller with `decide`; a kept candidate becomes the base of the next trial.
#
# Usage:
#   scripts/eval/tune.sh baseline                         # the pre-tuning reference (current config, hybrid row)
#   scripts/eval/tune.sh trial <name> <key=value>...      # one candidate on top of the current base
#   scripts/eval/tune.sh keep <name>                      # adopt the candidate's settings as the new base (winner so far)
# Keys are config.toml keys: search_rrf_k, search_raw_weight, search_questions_weight,
# search_and_stopwords, embedding_text (v1|questions-first|with-entities); the adapter's
# fetch depth is FETCH=<n> in the environment (candidate 6). An embedding_text change
# re-embeds every card under a new model id (local, no model call; ≈ 1.5 h for all four).
# Every trial's raw reports stay under $RUN/tuning/<name>/<project>/.
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
cmd="${1:?baseline|trial|keep}"; shift
T="$RUN/tuning"; mkdir -p "$T"; LOG="$RESULTS/TUNING.md"; BASE="$T/base.env"
[ -f "$BASE" ] || : > "$BASE"
cfg_get() { grep -E "^$2 = " "$RUN/$1/.markdownattractor/config.toml" | sed 's/^[^=]*= //' || true; }
cfg_set() { # dir key value
  local f="$RUN/$1/.markdownattractor/config.toml"
  if grep -qE "^$2 = " "$f"; then sed -i '' -E "s|^$2 = .*|$2 = $3|" "$f"; else printf '%s = %s\n' "$2" "$3" >> "$f"; fi
}
toml_value() { case "$1" in true|false) echo "$1" ;; *[!0-9.]*) echo "\"$1\"" ;; *) echo "$1" ;; esac; }
run_all() { # name -> writes $T/<name>/<project>/results.json and prints the summary json
  local name="$1" p dir out t0 t1
  for p in $PROJECTS; do
    dir="$(project_dir "$p")"; out="$T/$name/$p"; rm -rf "$out"; mkdir -p "$out"
    t0=$(date +%s)
    FETCH_ARG=(); [ -z "${FETCH:-}" ] || FETCH_ARG=(--fetch "$FETCH")
    "$MDA" --json eval --dataset docsqa --data "$RUN/docsqa-data" --project "$p" --root "$RUN/$dir" --split dev ${FETCH_ARG[@]+"${FETCH_ARG[@]}"} --out "$out" > "$out/report.json" 2>"$out/err.log" || die "eval failed on $p (see $out/err.log)"
    t1=$(date +%s); echo "$((t1 - t0))" > "$out/elapsed_s"
  done
  jq -n --arg name "$name" --argjson tw "$(jq '{s: .runs[-1].metrics.success_at_5, ms: .runs[-1].metrics.mean_ms, run: .runs[-1].run}' "$T/$name/tailwind-css/results.json")" \
        --argjson su "$(jq '{s: .runs[-1].metrics.success_at_5, ms: .runs[-1].metrics.mean_ms}' "$T/$name/supabase/results.json")" \
        --argjson pr "$(jq '{s: .runs[-1].metrics.success_at_5, ms: .runs[-1].metrics.mean_ms}' "$T/$name/prisma/results.json")" \
        --argjson gh "$(jq '{s: .runs[-1].metrics.success_at_5, ms: .runs[-1].metrics.mean_ms}' "$T/$name/github-docs/results.json")" \
        --argjson el "$(( $(cat "$T/$name"/*/elapsed_s | paste -sd+ -) ))" \
        '{name: $name, row: $tw.run, tailwind: $tw.s, supabase: $su.s, prisma: $pr.s, github: $gh.s, objective: (($tw.s + $su.s + $pr.s + $gh.s) / 4), mean_ms: (($tw.ms + $su.ms + $pr.ms + $gh.ms) / 4), elapsed_s: $el}'
}
log_line() { # summary.json config-diff decision
  local s="$1"
  printf '| %s | `%s` | %s | %.3f | %.3f | %.3f | %.3f | **%.4f** | %.0f | %s | %s |\n' \
    "$(jq -r .name <<<"$s")" "$2" "$(git -C "$REPO" rev-parse --short HEAD)" \
    "$(jq -r .tailwind <<<"$s")" "$(jq -r .supabase <<<"$s")" "$(jq -r .prisma <<<"$s")" "$(jq -r .github <<<"$s")" \
    "$(jq -r .objective <<<"$s")" "$(jq -r .mean_ms <<<"$s")" "$(jq -r .elapsed_s <<<"$s")s" "$3" >> "$LOG"
}
ensure_log() {
  [ -f "$LOG" ] && return
  cat > "$LOG" <<'MD'
# TUNING — the greedy loop of the execution plan §3 (development numbers, dev split only)

Objective: mean success@5 of the hybrid row over the four projects' dev splits, equal weights. Guardrail: no project drops by more than 0.02 from the pre-tuning reference. Candidates are the eight single changes of plan §3 in order, each evaluated on top of the current winner, kept when it improves the objective by ≥ 0.01 and passes the guardrail, otherwise discarded (a regression is logged, never retried with variations). Stop when the list is exhausted or the last two candidates both fail to improve by ≥ 0.01. Test and holdout are never looked at. Every trial's raw reports are under `~/.cache/markdownattractor/bench/tuning/<name>/<project>/` (development artifacts, not committed); the rows here are copied by `scripts/eval/tune.sh` from those files. Latency is the adapter in-process, release build, mean over the hybrid row's questions.

| trial | config diff (on top of the base at that time) | code SHA | tailwind | supabase | prisma | github-docs | objective | mean ms | elapsed | decision |
|---|---|---|---|---|---|---|---|---|---|---|
MD
}
case "$cmd" in
  baseline)
    ensure_log
    s="$(run_all baseline)"; echo "$s" > "$T/baseline.json"; cp "$T/baseline.json" "$T/best.json"
    log_line "$s" "(pre-tuning defaults)" "reference"; echo "$s" ;;
  trial)
    name="${1:?trial name}"; shift; ident "$name"
    ensure_log; [ -f "$T/best.json" ] || die "run baseline first"
    # apply the candidate on top of the base on every store; remember the previous values
    : > "$T/$name.restore"; printf '%s\n' "$@" > "$T/$name.kv"
    for kv in "$@"; do
      key="${kv%%=*}"; val="${kv#*=}"
      for p in $PROJECTS; do dir="$(project_dir "$p")"; prev="$(cfg_get "$dir" "$key")"; printf '%s\t%s\t%s\n' "$dir" "$key" "${prev:-__absent__}" >> "$T/$name.restore"; cfg_set "$dir" "$key" "$(toml_value "$val")"; done
    done
    s="$(run_all "$name")"; echo "$s" > "$T/$name.json"
    diff_txt="$*${FETCH:+ FETCH=$FETCH}"
    best="$(jq -r .objective "$T/best.json")"; obj="$(jq -r .objective <<<"$s")"; ref="$T/baseline.json"
    guard_ok="$(jq -n --argjson s "$s" --argjson r "$(cat "$ref")" '[$s.tailwind - $r.tailwind, $s.supabase - $r.supabase, $s.prisma - $r.prisma, $s.github - $r.github] | all(. >= -0.02)')"
    improved="$(jq -n --argjson o "$obj" --argjson b "$best" '$o - $b >= 0.01')"
    if [ "$improved" = true ] && [ "$guard_ok" = true ]; then decision="**keep** (+$(jq -n --argjson o "$obj" --argjson b "$best" '(($o - $b) * 1000 | round) / 1000') vs best $best)"; else decision="discard ($( [ "$guard_ok" = true ] && echo "Δ objective $(jq -n --argjson o "$obj" --argjson b "$best" '(($o - $b) * 1000 | round) / 1000') < 0.01" || echo "guardrail: a project dropped more than 0.02"))"; fi
    log_line "$s" "$diff_txt" "$decision"
    echo "$s"; echo "decision: $decision"
    # restore the base; `keep <name>` re-applies the winner
    while IFS=$'\t' read -r dir key prev; do if [ "$prev" = __absent__ ]; then sed -i '' -E "/^$key = /d" "$RUN/$dir/.markdownattractor/config.toml"; else cfg_set "$dir" "$key" "$prev"; fi; done < "$T/$name.restore" ;;
  keep)
    name="${1:?trial name}"; ident "$name"
    [ -f "$T/$name.kv" ] && [ -f "$T/$name.json" ] || die "no trial $name"
    # re-apply the candidate's settings permanently (the base moves forward) and record it as best
    while IFS= read -r kv; do [ -n "$kv" ] || continue; key="${kv%%=*}"; val="${kv#*=}"; for p in $PROJECTS; do cfg_set "$(project_dir "$p")" "$key" "$(toml_value "$val")"; done; done < "$T/$name.kv"
    cp "$T/$name.json" "$T/best.json"; printf '| — | base ← `%s` | | | | | | | | | winner so far |\n' "$name" >> "$LOG"
    echo "base is now $name" ;;
  *) die "unknown command $cmd" ;;
esac
