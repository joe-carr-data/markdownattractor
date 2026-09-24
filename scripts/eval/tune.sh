#!/usr/bin/env bash
# The tuning loop of the execution plan §3, one trial at a time: apply one pre-declared
# single change on top of the current base to the four DocsQA stores' config.toml
# (development numbers, dev split only), score the hybrid row on each project, archive the
# trial (manifest + the four results.json, committed under evals/results/docsqa/tuning/), append
# the row to evals/results/docsqa/TUNING.md with the objective (mean success@5 of the hybrid
# row over the four projects, equal weights), the guardrail (no project below the reference
# by more than 0.02), latency and elapsed time, then restore the base. The greedy decision
# (keep if the objective improves by ≥ 0.01 over the best so far and the guardrail holds) is
# written into the trial's manifest; `keep <name>` adopts a kept candidate as the new base
# and refuses a discarded or stale one. Every failure restores the base and exits non-zero.
#
# Usage:
#   scripts/eval/tune.sh baseline                         # the pre-tuning reference (current config)
#   scripts/eval/tune.sh trial <name> [key=value]...      # one candidate on top of the current base
#   scripts/eval/tune.sh keep <name>                      # adopt the candidate's settings as the new base
#   scripts/eval/tune.sh base                             # print the current base (settings and fetch)
#   scripts/eval/tune.sh explore post-stop-<cN> [key=value]...  # information only (plan §3, 2026-09-24
#                                                         # amendment): one candidate against the unchanged
#                                                         # pre-tuning configuration, never adoptable
# Keys: the config.toml keys search_rrf_k, search_raw_weight, search_questions_weight,
# search_and_stopwords, embedding_text (v1|questions-first|with-entities|
# questions-first-with-entities) and the adapter parameter fetch=<n> (plan §3 candidate 6),
# all persisted in the base. An embedding_text change re-embeds every card under a new
# model id (local, no model call; ≈ 1 h for all four corpora, kept in the stores).
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
cmd="${1:?baseline|trial|keep|base|explore}"; shift
T="$RUN/tuning"; mkdir -p "$T"; LOG="$RESULTS/TUNING.md"; ARCH="$RESULTS/tuning"; mkdir -p "$ARCH"
BASE="$T/base.kv"; [ -f "$BASE" ] || : > "$BASE"
HYBRID="hybrid (cards + raw + vectors)"
cfg_file() { echo "$RUN/$(project_dir "$1")/.markdownattractor/config.toml"; }
cfg_set() { # project key value(toml)
  local f; f="$(cfg_file "$1")"
  if grep -qE "^$2 = " "$f"; then sed -i '' -E "s|^$2 = .*|$2 = $3|" "$f"; else printf '%s = %s\n' "$2" "$3" >> "$f"; fi
}
cfg_del() { local f; f="$(cfg_file "$1")"; sed -i '' -E "/^$2 = /d" "$f"; }
toml_value() { case "$1" in true|false) echo "$1" ;; *[!0-9.]*) echo "\"$1\"" ;; *) echo "$1" ;; esac; }
base_fetch() { grep -E '^fetch=' "$BASE" | tail -1 | sed 's/^fetch=//' || true; }
base_fingerprint() { sort "$BASE" | shasum -a 256 | cut -c1-16; }
# Snapshot the four configs and restore them on every exit path, so a failed or interrupted
# trial never leaves candidate settings installed (Codex M3 F1).
snap=""
snapshot() { snap="$(mktemp -d -t mda-tune-snap.XXXXXX)"; local p; for p in $PROJECTS; do cp "$(cfg_file "$p")" "$snap/$p.toml"; done; trap 'restore' EXIT; }
restore() { local p; [ -n "$snap" ] || return 0; for p in $PROJECTS; do cp "$snap/$p.toml" "$(cfg_file "$p")"; done; rm -rf "$snap"; snap=""; }
apply_base() { local -a base_kvs=(); while IFS= read -r l; do [ -n "$l" ] && base_kvs+=("$l"); done < "$BASE"; [ "${#base_kvs[@]}" = 0 ] || apply_kv "${base_kvs[@]}"; }
apply_kv() { # key=value ... (fetch=<n> is not a config key)
  local kv key val p
  for kv in "$@"; do
    key="${kv%%=*}"; val="${kv#*=}"
    [ "$key" != fetch ] || continue
    for p in $PROJECTS; do cfg_set "$p" "$key" "$(toml_value "$val")"; done
  done
}
fetch_of() { local kv; for kv in "$@"; do [ "${kv%%=*}" = fetch ] && echo "${kv#*=}"; done; base_fetch; }
run_all() { # name fetch -> $T/<name>/<project>/… and the summary json; requires the hybrid row on every project
  local name="$1" fetch="$2" p dir out t0 t1 args=()
  [ -z "$fetch" ] || args=(--fetch "$fetch")
  for p in $PROJECTS; do
    dir="$(project_dir "$p")"; out="$T/$name/$p"; rm -rf "$out"; mkdir -p "$out"
    t0=$(date +%s)
    "$MDA" --json eval --dataset docsqa --data "$RUN/docsqa-data" --project "$p" --root "$RUN/$dir" --split dev ${args[@]+"${args[@]}"} --out "$out" > "$out/report.json" 2>"$out/err.log" || die "eval failed on $p (see $out/err.log)"
    t1=$(date +%s); echo "$((t1 - t0))" > "$out/elapsed_s"
    jq -e --arg h "$HYBRID" '.runs[] | select(.run == $h) | select(.metrics.questions > 0)' "$out/results.json" >/dev/null || die "no hybrid row on $p: embeddings off or model missing (see $out/report.json)"
    [ "$(jq -r '.card_coverage.complete' "$out/results.json")" = true ] || die "$p: card coverage incomplete"
  done
  m() { jq --arg h "$HYBRID" '.runs[] | select(.run == $h) | {s: .metrics.success_at_5, ms: .metrics.mean_ms, n: .metrics.questions, model: input_filename}' "$T/$name/$1/results.json" | jq --slurpfile r "$T/$name/$1/results.json" '. + {embedding_model: $r[0].embedding_model, search: $r[0].search}'; }
  jq -n --arg name "$name" --argjson tw "$(m tailwind-css)" --argjson su "$(m supabase)" --argjson pr "$(m prisma)" --argjson gh "$(m github-docs)" \
        --argjson el "$(( $(cat "$T/$name"/*/elapsed_s | paste -sd+ -) ))" \
        '{name: $name, tailwind: $tw.s, supabase: $su.s, prisma: $pr.s, github: $gh.s, n: {tailwind: $tw.n, supabase: $su.n, prisma: $pr.n, github: $gh.n},
          objective: (($tw.s + $su.s + $pr.s + $gh.s) / 4), mean_ms: (($tw.ms + $su.ms + $pr.ms + $gh.ms) / 4), elapsed_s: $el,
          embedding_model: $tw.embedding_model, search: $tw.search}'
}
archive() { # name summary-json kv-list decision
  local name="$1" s="$2" kvs="$3" decision="$4" p d
  d="$ARCH/$name"; rm -rf "$d"; mkdir -p "$d"
  for p in $PROJECTS; do cp "$T/$name/$p/results.json" "$d/$p.results.json"; done
  jq -n --arg name "$name" --argjson s "$s" --arg kvs "$kvs" --arg decision "$decision" --arg sha "$(git -C "$REPO" rev-parse HEAD)" \
     --arg bin "$(sha256 "$MDA")" --arg base "$(sort "$BASE" | tr '\n' ' ')" --arg base_fp "$(base_fingerprint)" --arg frozen "$(sha256 "$RESULTS/FROZEN.md")" \
     --arg cards "$(grep -E '^- cards-' "$RESULTS/FROZEN.md" | sed -E 's/.*sha256 ([0-9a-f]{64}).*/\1/' | tr '\n' ' ')" --arg at "$(date -u +%FT%TZ)" \
     '{trial: $name, at: $at, hypothesis_change: ($kvs | split(" ") | map(select(. != ""))), base_before: ($base | split(" ") | map(select(. != ""))), base_fingerprint: $base_fp,
       code_sha: $sha, binary_sha256: $bin, frozen_md_sha256: $frozen, cards_sha256: ($cards | split(" ") | map(select(. != ""))), summary: $s, decision: $decision,
       per_project_results: "<project>.results.json beside this manifest (archived observations; metrics regenerate from their page lists through --arm-output)"}' > "$d/manifest.json"
}
log_line() { # name kvs summary decision
  local s="$3"
  printf '| %s | `%s` | %s | %.3f (%s) | %.3f (%s) | %.3f (%s) | %.3f (%s) | **%.4f** | %.0f | %ss | %s |\n' \
    "$1" "${2:-(pre-tuning defaults)}" "$(git -C "$REPO" rev-parse --short HEAD)" \
    "$(jq -r .tailwind <<<"$s")" "$(jq -r .n.tailwind <<<"$s")" "$(jq -r .supabase <<<"$s")" "$(jq -r .n.supabase <<<"$s")" \
    "$(jq -r .prisma <<<"$s")" "$(jq -r .n.prisma <<<"$s")" "$(jq -r .github <<<"$s")" "$(jq -r .n.github <<<"$s")" \
    "$(jq -r .objective <<<"$s")" "$(jq -r .mean_ms <<<"$s")" "$(jq -r .elapsed_s <<<"$s")" "$4" >> "$LOG"
}
ensure_log() {
  [ -f "$LOG" ] && return
  cat > "$LOG" <<'MD'
# TUNING — the greedy loop of the execution plan §3 (development numbers, dev split only)

Objective: mean success@5 of the hybrid row over the four projects' dev splits, equal weights (denominators in parentheses). Guardrail: no project drops by more than 0.02 from the pre-tuning reference. Candidates are the single changes of plan §3 in order, each evaluated on top of the current base (the winner so far), kept when it improves the objective by ≥ 0.01 and passes the guardrail, otherwise discarded (a regression is logged, never retried with variations; a tie within 0.01 keeps the simpler base). Stop when the list is exhausted or two consecutive candidates fail to improve the objective by ≥ 0.01 (a guardrail-only discard does not count). Candidate 8 (a larger embedder) is not run without the vector-only diagnostic and an ADR. Test and holdout are never looked at. Each trial's manifest (hypothesis, base before, code SHA, binary sha256, cards hashes, decision) and its four `results.json` are archived under `evals/results/docsqa/tuning/<trial>/`; the rows here are written by `scripts/eval/tune.sh` from those files. Latency is the adapter in-process, release build, mean over the hybrid row's questions; elapsed includes re-embedding when the embedding text changed.

| trial | change (on top of the base at that time) | code SHA | tailwind (n) | supabase (n) | prisma (n) | github-docs (n) | objective | mean ms | elapsed | decision |
|---|---|---|---|---|---|---|---|---|---|---|
MD
}
EXPLORE_MARK="## Post-stop exploratory — information only (2026-09-24)"
ensure_explore_section() { # appended once, after the greedy loop's outcome; rows go under its own table
  grep -qF "$EXPLORE_MARK" "$LOG" && return
  cat >> "$LOG" <<'MD'

## Post-stop exploratory — information only (2026-09-24)

These trials occur after the registered stopping event and are not part of the greedy selection loop or final T1 results (execution plan §3, 2026-09-24 amendment, decided with Codex: `docs/reviews/codex/2026-09-24-post-stop-c3-c7.md`). Trial ids are `post-stop-c3` through `post-stop-c7`; every trial uses the unchanged pre-tuning baseline (`scripts/eval/tune.sh explore` refuses to run on a moved base), the same eligible dev questions and the original labels. Each row reports the configuration difference, code SHA, per-project scores with denominators, the objective, the unrounded objective difference with a descriptive 95% interval from a within-project paired bootstrap (5,000 draws, seed 20260922, `mda eval --compare`, joint resampling over the four projects), paired wins/losses, latency and elapsed time; the manifest, the four `results.json` and `compare.json` are archived under `evals/results/docsqa/tuning/<trial>/`. Decisions are `screen-pass-not-adopted`, `screen-fail-not-adopted` or `invalid-not-adopted`; `keep` refuses all three.

A within-project bootstrap of the baseline objective (5,000 draws, seed 20260922): objective 0.5014, SE 0.0486, 95% interval [0.406, 0.597], scored denominators 49/37/12/25 for GitHub Docs/Prisma/Supabase/Tailwind. That interval describes baseline sampling uncertainty, not the uncertainty of a candidate's paired improvement; the 0.01 screen is not a significance threshold. The M3 winner and this release's product defaults remain unchanged regardless of these observations.

**Screening and disposition.** A valid trial is `screen-pass-not-adopted` only if its unrounded mean success@5 improvement over the pre-tuning baseline is ≥ 0.01 and every project's unrounded change is ≥ −0.02; otherwise it is `screen-fail-not-adopted`. At the fixed denominators 49/37/12/25 the guardrail permits no net loss of one successful question on any project: even 1/49 exceeds 0.02. Question ids, eligibility, labels and denominators stay fixed; a trial whose evaluation fails or whose question set differs from the baseline's is `invalid-not-adopted`, logged with its error. The paired intervals do not authorize adoption or establish significance across five trials. No exploratory score triggers adoption for this release; passing candidates are hypotheses for a future, separately declared evaluation. If none passes, the defaults stay and M4 proceeds without additional trials or relaxed thresholds. The final `FROZEN.md` records the unchanged effective configuration and "Selection: original §3 winner; post-stop diagnostics excluded from selection".

| trial | change (against the pre-tuning baseline) | code SHA | tailwind (n) | supabase (n) | prisma (n) | github-docs (n) | objective | Δ objective, 95% paired | wins/losses | mean ms | elapsed | decision |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
MD
}
explore_line() { # name kvs summary compare-json decision-text
  local s="$3" c="$4"
  printf '| %s | `%s` | %s | %.3f (%s) | %.3f (%s) | %.3f (%s) | %.3f (%s) | **%.4f** | %+.4f [%+.4f, %+.4f] | %s/%s | %.0f | %ss | %s |\n' \
    "$1" "$2" "$(git -C "$REPO" rev-parse --short HEAD)" \
    "$(jq -r .tailwind <<<"$s")" "$(jq -r .n.tailwind <<<"$s")" "$(jq -r .supabase <<<"$s")" "$(jq -r .n.supabase <<<"$s")" \
    "$(jq -r .prisma <<<"$s")" "$(jq -r .n.prisma <<<"$s")" "$(jq -r .github <<<"$s")" "$(jq -r .n.github <<<"$s")" \
    "$(jq -r .objective <<<"$s")" "$(jq -r .report.objective_delta <<<"$c")" "$(jq -r '.report.objective_ci95[0]' <<<"$c")" "$(jq -r '.report.objective_ci95[1]' <<<"$c")" \
    "$(jq -r .report.wins <<<"$c")" "$(jq -r .report.losses <<<"$c")" "$(jq -r .mean_ms <<<"$s")" "$(jq -r .elapsed_s <<<"$s")" "$5" >> "$LOG"
}
explore_invalid() { # name kvs reason
  local d="$ARCH/$1"; mkdir -p "$d"
  jq -n --arg name "$1" --arg kvs "$2" --arg reason "$3" --arg sha "$(git -C "$REPO" rev-parse HEAD)" --arg at "$(date -u +%FT%TZ)" \
     '{trial: $name, at: $at, hypothesis_change: ($kvs | split(" ") | map(select(. != ""))), code_sha: $sha, decision: "invalid-not-adopted", reason: $reason}' > "$d/manifest.json"
  printf '| %s | `%s` | %s | | | | | | | | | | invalid-not-adopted (%s) |\n' "$1" "$2" "$(git -C "$REPO" rev-parse --short HEAD)" "$3" >> "$LOG"
}
case "$cmd" in
  base) cat "$BASE"; echo "fingerprint $(base_fingerprint)" ;;
  explore-table) # the page table (docs/benchmarks.md) from the archived post-stop trials, never retyped
    echo "| trial | change | tailwind | supabase | prisma | github-docs | objective | Δ objective, 95% paired | wins/losses | decision |"
    echo "|---|---|---|---|---|---|---|---|---|---|"
    for d in "$ARCH"/post-stop-*/; do
      m="$d/manifest.json"; [ -f "$m" ] || continue
      if [ -f "$d/compare.json" ]; then
        jq -r --slurpfile c "$d/compare.json" '[.trial, "`" + (.hypothesis_change | join(" ")) + "`", (.summary.tailwind | . * 1000 | round / 1000), (.summary.supabase | . * 1000 | round / 1000), (.summary.prisma | . * 1000 | round / 1000), (.summary.github | . * 1000 | round / 1000), (.summary.objective | . * 10000 | round / 10000),
          (($c[0].report.objective_delta | . * 10000 | round / 10000 | tostring) + " [" + ($c[0].report.objective_ci95[0] | . * 10000 | round / 10000 | tostring) + ", " + ($c[0].report.objective_ci95[1] | . * 10000 | round / 10000 | tostring) + "]"),
          (($c[0].report.wins | tostring) + "/" + ($c[0].report.losses | tostring)), .decision] | "| " + join(" | ") + " |"' "$m"
      else jq -r '"| " + .trial + " | `" + (.hypothesis_change | join(" ")) + "` | | | | | | | | " + .decision + " (" + (.reason // "") + ") |"' "$m"; fi
    done ;;
  explore)
    name="${1:?trial name}"; shift; ident "$name"
    case "$name" in post-stop-*) ;; *) die "exploratory trials are named post-stop-<candidate>" ;; esac
    ensure_log; [ -f "$T/baseline.json" ] && [ -f "$ARCH/baseline/manifest.json" ] || die "run baseline first"
    [ ! -s "$BASE" ] || die "the base has moved from the pre-tuning configuration ($(sort "$BASE" | tr '\n' ' ')): post-stop trials run only against it"
    [ ! -d "$ARCH/$name" ] || die "trial $name already archived: trial names are immutable"
    ensure_explore_section
    snapshot; apply_base; apply_kv "$@"; fetch="$(fetch_of "$@")"
    if ! s="$(run_all "$name" "$fetch")"; then explore_invalid "$name" "$*" "evaluation failed (see $T/$name/*/err.log)"; exit 1; fi
    echo "$s" > "$T/$name.json"
    ref="$(cat "$T/baseline.json")"
    guard_ok="$(jq -n --argjson s "$s" --argjson r "$ref" '[$s.tailwind - $r.tailwind, $s.supabase - $r.supabase, $s.prisma - $r.prisma, $s.github - $r.github] | all(. >= -0.02)')"
    delta_raw="$(jq -n --argjson s "$s" --argjson r "$ref" '$s.objective - $r.objective')"
    screen="$(jq -n --argjson d "$delta_raw" '$d >= 0.01')"
    if [ "$screen" = true ] && [ "$guard_ok" = true ]; then decision="screen-pass-not-adopted"; text="screen-pass — **not adopted** (Δ ≥ 0.01 and guardrail held; a hypothesis for a future, separately declared evaluation)"
    elif [ "$screen" = true ]; then decision="screen-fail-not-adopted"; text="screen-fail — not adopted (guardrail: a project lost more than 0.02)"
    else decision="screen-fail-not-adopted"; text="screen-fail — not adopted (Δ < 0.01)"; fi
    printf '%s\n' "$@" > "$T/$name.kv"; jq -n --arg d "$decision" --arg fp "$(base_fingerprint)" '{decision: $d, base_fingerprint: $fp}' > "$T/$name.decision.json"
    archive "$name" "$s" "$*" "$decision"
    cmp_args=(); for p in $PROJECTS; do cmp_args+=(--compare "$p" "$ARCH/baseline/$p.results.json" "$ARCH/$name/$p.results.json"); done
    if ! c="$("$MDA" --json eval "${cmp_args[@]}" --draws 5000 --seed 20260922)"; then
      rm -rf "${ARCH:?}/${name:?}"; explore_invalid "$name" "$*" "paired comparison failed: question sets differ from the baseline's"; exit 1
    fi
    echo "$c" > "$ARCH/$name/compare.json"
    explore_line "$name" "$*" "$s" "$c" "$text"
    echo "$s"; echo "$c" | jq -c '.report | {objective_delta, objective_ci95, wins, losses}'; echo "decision: $decision" ;;
  baseline)
    ensure_log; snapshot; apply_base
    s="$(run_all baseline "$(base_fetch)")"; echo "$s" > "$T/baseline.json"; cp "$T/baseline.json" "$T/best.json"
    archive baseline "$s" "" "reference"; log_line baseline "" "$s" "reference"; echo "$s" ;;
  trial)
    name="${1:?trial name}"; shift; ident "$name"
    ensure_log; [ -f "$T/best.json" ] || die "run baseline first"
    [ ! -d "$ARCH/$name" ] || die "trial $name already archived: trial names are immutable"
    snapshot; apply_base; apply_kv "$@"; fetch="$(fetch_of "$@")"
    s="$(run_all "$name" "$fetch")"; echo "$s" > "$T/$name.json"
    best="$(jq -r .objective "$T/best.json")"; obj="$(jq -r .objective <<<"$s")"
    guard_ok="$(jq -n --argjson s "$s" --argjson r "$(cat "$T/baseline.json")" '[$s.tailwind - $r.tailwind, $s.supabase - $r.supabase, $s.prisma - $r.prisma, $s.github - $r.github] | all(. >= -0.02)')"
    delta="$(jq -n --argjson o "$obj" --argjson b "$best" '(($o - $b) * 10000 | round) / 10000')"
    improved="$(jq -n --argjson d "$delta" '$d >= 0.01')"
    if [ "$improved" = true ] && [ "$guard_ok" = true ]; then decision="keep"; text="**keep** (Δ objective +$delta vs best $best)"
    elif [ "$improved" = true ]; then decision="discard-guardrail"; text="discard: guardrail (a project dropped more than 0.02 from the reference; Δ objective +$delta)"
    else decision="discard"; text="discard (Δ objective $delta < 0.01; the simpler base stays)"; fi
    printf '%s\n' "$@" > "$T/$name.kv"; jq -n --arg d "$decision" --arg fp "$(base_fingerprint)" '{decision: $d, base_fingerprint: $fp}' > "$T/$name.decision.json"
    archive "$name" "$s" "$*" "$decision"; log_line "$name" "$*" "$s" "$text"
    echo "$s"; echo "decision: $decision" ;;
  keep)
    name="${1:?trial name}"; ident "$name"
    [ -f "$T/$name.decision.json" ] || die "no decided trial $name"
    [ "$(jq -r .decision "$T/$name.decision.json")" = keep ] || die "trial $name was not a keep"
    [ "$(jq -r .base_fingerprint "$T/$name.decision.json")" = "$(base_fingerprint)" ] || die "trial $name was evaluated on another base (stale)"
    # the base moves forward: its key=value pairs join the base (later values win), permanently applied
    { cat "$BASE"; cat "$T/$name.kv"; } | awk -F= '!/^$/ {v[$1]=$0} END {for (k in v) print v[k]}' | sort > "$BASE.new" && mv "$BASE.new" "$BASE"
    apply_base
    cp "$T/$name.json" "$T/best.json"; printf '| — | base ← `%s` (fingerprint %s) | | | | | | | | | winner so far |\n' "$name" "$(base_fingerprint)" >> "$LOG"
    echo "base is now $name: $(sort "$BASE" | tr '\n' ' ')" ;;
  *) die "unknown command $cmd" ;;
esac
