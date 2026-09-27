#!/usr/bin/env bash
# T4, axis C on Prisma (execution plan §4, plan docs/plans/2026-09-m7-t4-harness.md): how long
# after a save each arm can find the change. One edit = a dated note with a unique token
# appended to one section; twenty edits on twenty seeded sections; every arm works on its own
# copy of the Prisma checkout (the pinned checkout and the T1/T2 artifacts are never touched):
#   mda      — the copy carries the frozen store; the daemon watches it (backend claude-cli, Haiku)
#   qmd      — the copy has its own qmd index (t4-qmd); trigger: qmd update && qmd embed
#   graphify — the Haiku-built graph's copy (the only completed graphify build on Prisma); trigger:
#              the documented `/graphify <copy> --update` skill flow through the login
#   grep     — files only; no index, no trigger
# Endpoints, each timed from the save, polled every second, 300 s timeout (a timeout is a row):
#   raw      — the token is returned by the arm's raw search for the edited page (mda: `mda search
#              --raw`; qmd: `qmd search`, after its trigger); n/a for graphify and grep
#   card     — mda only: the store's summaries row for the edited section's new hash is `summarized`
#   answer   — a headless `claude -p` with the arm's launch configuration (lib.sh arm_launch on the
#              copy) answers "what does the T4 note in <page> › <heading> say?" with the token and
#              cites the page (T2's citation rule); every arm; for qmd and graphify the trigger runs
#              first and its duration is part of the endpoint
#
# Usage:
#   scripts/eval/t4.sh setup                 # copies, qmd index, graphify copy, the mda daemon, the edit plan (edits.jsonl) — before the freeze
#   scripts/eval/t4.sh status                # daemon, indexes, rows present
#   scripts/eval/t4.sh preflight             # freeze check, env, daemon healthy, token uniqueness, three answer probes per arm on edits 1–3 (no edit applied)
#   scripts/eval/t4.sh run [from] [to]       # edits from..to (default 1..20), every arm in turn; resumable (a present row is never redone)
#   scripts/eval/t4.sh table                 # the page table from the rows
#   scripts/eval/t4.sh teardown              # stop the daemon (copies are kept)
# Env: T4_MODEL (sonnet, the answering model), T4_TIMEOUT (300), T4_ARMS (mda qmd graphify grep), REPO/RUN/MDA.
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
set -euo pipefail
cmd="${1:?setup|status|preflight|run|table|teardown}"; shift
PROJECT=prisma; T4="$RUN/t4"; RES="$RESULTS/T4"; ARMS="${T4_ARMS:-mda qmd graphify grep}"
MODEL="${T4_MODEL:-sonnet}"; TIMEOUT="${T4_TIMEOUT:-300}"; QINDEX=t4-qmd; N_EDITS=20; MIN_BODY=200
PIN="$RUN/$(project_dir "$PROJECT")"
unset_nested_session; unset_provider_keys
now_ms() { python3 -c 'import time; print(int(time.time() * 1000))'; }
copy_of() { echo "$T4/$1/src"; }

# The answer endpoint's session: the arm's launch configuration on its copy.
launch_env() { # arm
  ARM_CORPUS_OVERRIDE="$(copy_of "$1")"; export ARM_CORPUS_OVERRIDE
  case "$1" in
    qmd) export ARM_QMD_INDEX="$QINDEX" ;;
    graphify) export ARM_GRAPHIFY_DIR="$T4/graphify" ;;
  esac
}
clear_env() { unset ARM_CORPUS_OVERRIDE ARM_QMD_INDEX ARM_GRAPHIFY_DIR; }
arm_name() { case "$1" in graphify) echo graphify-haiku ;; *) echo "$1" ;; esac; }   # the launcher's name for the arm

case "$cmd" in
  setup)
    mkdir -p "$T4/rows" "$T4/traces" "$RES"
    [ -d "$PIN/.markdownattractor" ] || die "no frozen store at $PIN"
    # mda: files + store; the daemon must not be running on the pinned checkout
    if [ ! -d "$(copy_of mda)" ]; then
      mkdir -p "$(copy_of mda)"; rsync -a --exclude .git "$PIN/" "$(copy_of mda)/"; echo "mda copy: $(copy_of mda) (files + store, $(du -sh "$(copy_of mda)/.markdownattractor" | cut -f1))"
    fi
    for a in qmd grep; do
      if [ ! -d "$(copy_of $a)" ]; then mkdir -p "$(copy_of $a)"; rsync -a --exclude .git --exclude .markdownattractor "$PIN/" "$(copy_of $a)/"; echo "$a copy: $(copy_of $a)"; fi
    done
    # graphify: the Haiku build's copy (skill, hooks, graph)
    if [ ! -d "$T4/graphify" ]; then
      [ -f "$RUN/graphify/$PROJECT-haiku/graph.json" ] || die "no graphify-haiku build for $PROJECT"
      mkdir -p "$T4/graphify"; rsync -a "$RUN/graphify/$PROJECT-haiku/" "$T4/graphify/"; echo "graphify copy: $T4/graphify (haiku build, graph sha256 $(sha256 "$T4/graphify/graph.json" | cut -c1-12)…)"
    fi
    # qmd: its own index on the copy
    if [ ! -f "$HOME/.config/qmd/$QINDEX.yml" ]; then
      t0=$(date +%s); qmd --index "$QINDEX" collection add "$(copy_of qmd)" --name "$QINDEX" --mask '**/*.{md,mdx,markdown}' >/dev/null && qmd --index "$QINDEX" update >/dev/null && qmd --index "$QINDEX" embed >/dev/null
      echo "qmd index $QINDEX built in $(( $(date +%s) - t0 )) s"
    fi
    # the mda daemon on its copy
    if ! "$MDA" --json status --root "$(copy_of mda)" 2>/dev/null | jq -e '.daemon != null' >/dev/null; then
      "$MDA" start --root "$(copy_of mda)" --no-example >/dev/null 2>&1 || true; sleep 3
    fi
    "$MDA" --json status --root "$(copy_of mda)" | jq -c '{daemon: (.daemon != null), pending: .summaries.pending, summarized: .summaries.summarized}' 2>/dev/null || echo "status unavailable"
    # the edit plan: twenty sections, seeded, one per document, body ≥ MIN_BODY chars; tokens unique in the corpus
    if [ ! -f "$T4/edits.jsonl" ]; then
      db="$(copy_of mda)/.markdownattractor/index.sqlite"
      sqlite3 -json "$db" "SELECT s.section_id, d.rel_path AS page, s.heading_path, s.line_start, s.line_end, s.section_hash, length(s.text) AS chars FROM sections s JOIN docs d ON d.doc_id = s.doc_id WHERE length(s.text) >= $MIN_BODY AND s.heading_path != '[]'" \
        | jq -c --arg seed "$SEED" '.[]' | while IFS= read -r r; do k="$(printf '%s\x00%s' "$SEED" "$(jq -r .section_id <<<"$r")" | shasum -a 256 | cut -c1-16)"; jq -c --arg k "$k" '. + {key: $k}' <<<"$r"; done \
        | jq -s -c --argjson n "$N_EDITS" 'sort_by(.key) | reduce .[] as $s ({seen: [], out: []}; if (.out | length) >= $n or (.seen | index([$s.page])) != null then . else {seen: (.seen + [$s.page]), out: (.out + [$s])} end) | .out | to_entries[] | .value + {edit: (.key + 1)} | del(.key)' \
        | while IFS= read -r r; do tok="t4-$(jq -r .edit <<<"$r")-$(printf '%s\x00%s' "$SEED" "$(jq -r .section_id <<<"$r")" | shasum -a 256 | cut -c1-8)"; n="$(grep -rl --include='*.md' --include='*.mdx' --include='*.markdown' -e "$tok" "$PIN" | wc -l | tr -d ' ')"; [ "$n" = 0 ] || die "token $tok already occurs in the corpus"; jq -c --arg t "$tok" --arg d "$(date -u +%F)" '. + {token: $t, note: ("T4 note (" + $d + "): " + $t + ".")}' <<<"$r"; done > "$T4/edits.jsonl"
      cp "$T4/edits.jsonl" "$RES/edits.jsonl"
      echo "edit plan: $(grep -c . "$T4/edits.jsonl") edits → $RES/edits.jsonl ($(jq -r .page "$T4/edits.jsonl" | sort -u | wc -l | tr -d ' ') distinct pages)"
    fi ;;
  status)
    "$MDA" --json status --root "$(copy_of mda)" | jq -c '{daemon: (.daemon != null), pending: .summaries.pending, failed: .summaries.failed}'
    qmd --index "$QINDEX" status 2>/dev/null | head -3
    echo "rows: $(ls "$T4/rows" 2>/dev/null | wc -l | tr -d ' ')" ;;
  preflight)
    report="$RESULTS/preflight/T4-$PROJECT.json"; safe_target "$report"; checks='[]'; failed=0
    rec() { checks="$(jq -c --arg n "$1" --arg s "$2" --arg d "$3" '. + [{name: $n, status: $s, detail: $d}]' <<<"$checks")"; [ "$2" != FAIL ] || failed=$((failed + 1)); echo "$2 $1: $3"; }
    jq -n --arg at "$(date -u +%FT%TZ)" '{table: "T4", project: "prisma", run_at: $at, status: "in progress", passed: false}' > "$report"
    if env | grep -qE '^[A-Z_]*_API_KEY='; then rec env FAIL "a provider key is exported"; else rec env ok "no provider key · claude $(claude --version 2>/dev/null | head -1)"; fi
    if out="$("$REPO/scripts/eval/freeze.sh" --protocol final --table T4 --check 2>&1)"; then rec frozen ok "$out"; else rec frozen FAIL "$out"; fi
    if "$MDA" --json status --root "$(copy_of mda)" | jq -e '.daemon != null and .summaries.pending == 0' >/dev/null; then rec daemon ok "running on $(copy_of mda), nothing pending"; else rec daemon FAIL "not running or pending work on the mda copy"; fi
    n_tok=0; while IFS= read -r r; do t="$(jq -r .token <<<"$r")"; for a in $ARMS; do d="$(copy_of $a)"; [ "$a" = graphify ] && d="$T4/graphify/src"; if grep -rq --include='*.md' --include='*.mdx' -e "$t" "$d"; then n_tok=$((n_tok + 1)); fi; done; done < "$T4/edits.jsonl"
    if [ "$n_tok" = 0 ]; then rec tokens ok "no edit token present in any copy yet ($(grep -c . "$T4/edits.jsonl") edits planned)"; else rec tokens FAIL "$n_tok token occurrences already present: the copies are not clean"; fi
    [ "$(sha256 "$T4/edits.jsonl")" = "$(sha256 "$RES/edits.jsonl")" ] && rec plan ok "edits.jsonl matches the committed plan" || rec plan FAIL "edits.jsonl differs from the committed plan"
    # answer probes: three sessions per arm on the first three edits' pages (no edit applied): the arm's tool must be used
    mkdir -p "$RESULTS/preflight/T4-$PROJECT/probes"
    for a in $ARMS; do
      n_ok=0
      for e in 1 2 3; do
        page="$(jq -r --argjson e "$e" 'select(.edit == $e) | .page' "$T4/edits.jsonl")"; heading="$(jq -r --argjson e "$e" 'select(.edit == $e) | .heading_path | fromjson | join(" › ")' "$T4/edits.jsonl" 2>/dev/null || jq -r --argjson e "$e" 'select(.edit == $e) | .heading_path' "$T4/edits.jsonl")"
        launch_env "$a"; arm_launch "$(arm_name "$a")" "$PROJECT" "$MODEL"; clear_env
        trace="$RESULTS/preflight/T4-$PROJECT/probes/$a-$e.jsonl"; rc=0
        (cd "$ARM_CORPUS" && claude --print --no-session-persistence --model "$MODEL" --max-turns 12 --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config --setting-sources "$ARM_SETTING_SOURCES" "${ARM_ARGS[@]}" -- "Answer from the documents in the current directory. In $page, section '$heading': summarise the section in two lines and cite the file." </dev/null > "$trace" 2>"$trace.err") || rc=$?
        [ -s "$trace.err" ] || rm -f "$trace.err"; [ "${#ARM_TMP[@]}" = 0 ] || rm -f "${ARM_TMP[@]}"
        if jq -s --arg w "$ARM_WANT" '(map(select(.type == "assistant")) | map(.message.content[]? | select(.type == "tool_use") | .name) | map(select(test($w))) | length) > 0' "$trace" | grep -q true; then n_ok=$((n_ok + 1)); fi
      done
      if [ "$n_ok" = 3 ]; then rec "probes-$a" ok "3 of 3 sessions used the arm's tool"; else rec "probes-$a" FAIL "$n_ok of 3 sessions used the arm's tool"; fi
    done
    jq -n --arg at "$(date -u +%FT%TZ)" --argjson checks "$checks" --argjson failed "$failed" '{table: "T4", project: "prisma", run_at: $at, status: "completed", passed: ($failed == 0), checks: $checks}' > "$report"
    echo "report: ${report#"$REPO"/} · passed=$([ "$failed" = 0 ] && echo true || echo false)"; [ "$failed" = 0 ] ;;
  run)
    from="${1:-1}"; to="${2:-$N_EDITS}"
    "$REPO/scripts/eval/freeze.sh" --protocol final --table T4 --check >/dev/null || die "T4/FROZEN.md does not check clean: nothing runs against changed inputs"
    apply_edit() { # copy page line_end note -> appends the note after the section's last line
      python3 - "$1/$2" "$3" "$4" <<'PY'
import sys
path, line_end, note = sys.argv[1], int(sys.argv[2]), sys.argv[3]
lines = open(path, encoding="utf-8").read().split("\n")
i = min(line_end, len(lines))
lines[i:i] = ["", note]
open(path, "w", encoding="utf-8").write("\n".join(lines))
PY
    }
    cites_page() { # answer page -> 0 when the answer names the page (exact path or its unique suffix rule of T2)
      grep -qF -e "$2" <<<"$1" && return 0
      grep -qF -e "$(basename "$2")" <<<"$1"
    }
    for ((e = from; e <= to; e++)); do
      r="$(jq -c --argjson e "$e" 'select(.edit == $e)' "$T4/edits.jsonl")"; [ -n "$r" ] || die "no edit $e in the plan"
      page="$(jq -r .page <<<"$r")"; line_end="$(jq -r .line_end <<<"$r")"; note="$(jq -r .note <<<"$r")"; tok="$(jq -r .token <<<"$r")"
      heading="$(jq -r '.heading_path | fromjson | join(" › ")' <<<"$r" 2>/dev/null || jq -r .heading_path <<<"$r")"
      for a in $ARMS; do
        row="$T4/rows/$e-$a.json"; [ ! -f "$row" ] || { echo "edit $e $a: present"; continue; }
        copy="$(copy_of $a)"; [ "$a" = graphify ] && copy="$T4/graphify/src"
        grep -rq --include='*.md' --include='*.mdx' -e "$tok" "$copy" && die "edit $e $a: the token is already in the copy (a previous attempt left it): remove it before rerunning"
        t_save="$(now_ms)"; apply_edit "$copy" "$page" "$line_end" "$note"
        trig_ms=null; trig_rc=null; trig_cost=null
        case "$a" in
          qmd) t0="$(now_ms)"; rc=0; (qmd --index "$QINDEX" update >/dev/null 2>"$T4/traces/$e-qmd-update.err" && qmd --index "$QINDEX" embed >/dev/null 2>>"$T4/traces/$e-qmd-update.err") || rc=$?; trig_ms=$(( $(now_ms) - t0 )); trig_rc=$rc ;;
          graphify)
            t0="$(now_ms)"; rc=0
            (cd "$copy" && CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0 claude --print --setting-sources project --no-session-persistence --model haiku --max-turns 200 --output-format stream-json --verbose --permission-mode dontAsk --disallowedTools ScheduleWakeup CronCreate CronDelete CronList -- "/graphify $copy --update --no-viz" </dev/null > "$T4/traces/$e-graphify-update.jsonl" 2>"$T4/traces/$e-graphify-update.err") || rc=$?
            trig_ms=$(( $(now_ms) - t0 )); trig_rc=$rc; trig_cost="$(jq -s '(map(select(.type == "result")) | last | .total_cost_usd) // null' "$T4/traces/$e-graphify-update.jsonl")"
            # the served graph is the copy's rebuilt graph
            [ ! -f "$copy/graphify-out/graph.json" ] || cp "$copy/graphify-out/graph.json" "$T4/graphify/graph.json" ;;
        esac
        # raw endpoint (mda, qmd) and card endpoint (mda): polled every second from the save
        raw_ms=null; raw_to=null; card_ms=null; card_to=null; polls=0
        if [ "$a" = mda ] || [ "$a" = qmd ]; then
          raw_to=false
          while :; do
            polls=$((polls + 1)); el=$(( $(now_ms) - t_save ))
            if [ "$a" = mda ]; then found="$("$MDA" --json search --raw "$tok" -k 5 --root "$copy" 2>/dev/null | jq -r --arg p "$page" '[.hits[]? | select(.rel_path == $p)] | length')"
            else found="$(qmd --index "$QINDEX" search "$tok" --json 2>/dev/null | jq -r --arg p "$page" '[.[]? | select((.file // .path // "") | contains($p))] | length' 2>/dev/null || echo 0)"; fi
            if [ "${found:-0}" -gt 0 ]; then raw_ms=$el; break; fi
            if [ "$el" -ge $((TIMEOUT * 1000)) ]; then raw_to=true; break; fi
            sleep 1
          done
        fi
        if [ "$a" = mda ]; then
          card_to=false; db="$copy/.markdownattractor/index.sqlite"
          while :; do
            el=$(( $(now_ms) - t_save ))
            state="$(sqlite3 "$db" "SELECT sm.state FROM sections s JOIN docs d ON d.doc_id = s.doc_id JOIN summaries sm ON sm.section_hash = s.section_hash WHERE d.rel_path = '$page' AND s.text LIKE '%$tok%' LIMIT 1" 2>/dev/null || true)"
            if [ "$state" = summarized ]; then card_ms=$el; break; fi
            if [ "$el" -ge $((TIMEOUT * 1000)) ]; then card_to=true; break; fi
            sleep 1
          done
        fi
        # answer endpoint: one headless session with the arm's configuration on the copy
        launch_env "$a"; arm_launch "$(arm_name "$a")" "$PROJECT" "$MODEL"; clear_env
        trace="$T4/traces/$e-$a-answer.jsonl"; rc=0; t0="$(now_ms)"
        (cd "$ARM_CORPUS" && claude --print --no-session-persistence --model "$MODEL" --max-turns 12 --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config --setting-sources "$ARM_SETTING_SOURCES" "${ARM_ARGS[@]}" -- "Answer from the documents in the current directory. In $page, section '$heading', there is a line starting with 'T4 note'. Quote that line exactly and cite the file." </dev/null > "$trace" 2>"$trace.err") || rc=$?
        [ -s "$trace.err" ] || rm -f "$trace.err"; [ "${#ARM_TMP[@]}" = 0 ] || rm -f "${ARM_TMP[@]}"
        ans="$(jq -s -r '(map(select(.type == "result")) | last | .result) // ""' "$trace")"
        ans_ms=$(( $(now_ms) - t_save )); ok=false; grep -qF -e "$tok" <<<"$ans" && cites_page "$ans" "$page" && ok=true
        used="$(jq -s --arg w "$ARM_WANT" '(map(select(.type == "assistant")) | map(.message.content[]? | select(.type == "tool_use") | .name) | map(select(test($w))) | length) > 0' "$trace")"
        jq -n --argjson e "$e" --arg arm "$a" --arg page "$page" --arg tok "$tok" --argjson t_save "$t_save" --argjson trig_ms "$trig_ms" --argjson trig_rc "$trig_rc" --argjson trig_cost "$trig_cost" \
          --argjson raw_ms "$raw_ms" --argjson raw_to "$raw_to" --argjson card_ms "$card_ms" --argjson card_to "$card_to" --argjson polls "$polls" \
          --argjson ans_ms "$ans_ms" --argjson ans_ok "$ok" --argjson ans_rc "$rc" --argjson used "$used" --arg ans "$(printf '%s' "$ans" | head -c 600)" \
          --argjson cost "$(jq -s '(map(select(.type == "result")) | last | .total_cost_usd) // null' "$trace")" --argjson turns "$(jq -s '(map(select(.type == "result")) | last | .num_turns) // null' "$trace")" \
          '{edit: $e, arm: $arm, page: $page, token: $tok, t_save_ms: $t_save, trigger: (if $trig_ms == null then null else {ms: $trig_ms, exit_code: $trig_rc, cost_usd: $trig_cost} end),
            raw: (if $raw_to == null then null else {ms: $raw_ms, timed_out: $raw_to, polls: $polls} end), card: (if $card_to == null then null else {ms: $card_ms, timed_out: $card_to} end),
            answer: {ms: $ans_ms, correct_grounded: $ans_ok, exit_code: $ans_rc, arm_tool_used: $used, turns: $turns, cost_usd: $cost, text: $ans, timed_out: ($ans_ms >= 300000)}}' > "$row.tmp" && mv "$row.tmp" "$row"
        echo "edit $e $a: raw $raw_ms ms · card $card_ms ms · answer $ans_ms ms ok=$ok$( [ "$trig_ms" = null ] || echo " · trigger $trig_ms ms")"
      done
    done ;;
  table)
    echo "| arm | endpoint | n | p50 s | p90 s | max s | timeouts | note |"
    echo "|---|---|---|---|---|---|---|---|"
    cat "$T4"/rows/*.json | jq -s -r '
      def pct(q): sort | if length == 0 then null else .[((length - 1) * q | round)] end;
      def s(v): if v == null then "–" else (v / 100 | round / 10 | tostring) end;
      def row(arm; ep; vals; tos; note): "| \(arm) | \(ep) | \(vals | length + (tos | length)) | \(vals | pct(0.5) | s(.)) | \(vals | pct(0.9) | s(.)) | \(vals | max | s(.)) | \(tos | length) | \(note) |";
      group_by(.arm)[] | .[0].arm as $a |
      (if any(.[]; .raw != null) then row($a; "save → raw-searchable"; [.[] | .raw | select(. != null and .timed_out == false) | .ms]; [.[] | .raw | select(. != null and .timed_out)]; (if $a == "qmd" then "includes `qmd update && qmd embed`" else "the daemon debounce is 1 s" end)) else empty end),
      (if any(.[]; .card != null) then row($a; "save → card"; [.[] | .card | select(. != null and .timed_out == false) | .ms]; [.[] | .card | select(. != null and .timed_out)]; "G1 target: p50 < 15 s") else empty end),
      row($a; "save → correct grounded answer"; [.[] | .answer | select(.correct_grounded) | .ms]; [.[] | .answer | select(.correct_grounded | not)]; ("\([.[] | .answer | select(.correct_grounded)] | length) of \(length) answers quote the note and cite the page" + (if $a == "graphify" then "; includes `/graphify --update`" elif $a == "qmd" then "; includes the update trigger" else "" end))),
      (if any(.[]; .trigger != null) then row($a; "update trigger alone"; [.[] | .trigger | select(. != null) | .ms]; []; (if $a == "graphify" then "`/graphify <copy> --update` through the login (Haiku host); cost $\([.[] | .trigger.cost_usd // 0] | add | . * 100 | round / 100)" else "`qmd update && qmd embed`" end)) else empty end)'
    echo
    echo "Generated by \`scripts/eval/t4.sh table\` from \`evals/results/docsqa/T4/rows/*.json\` (20 edits on Prisma; every endpoint timed from the save; 1 s polling; 300 s timeout; an answer counts when it quotes the token and cites the page). The \"timeouts\" column of the answer endpoint counts answers that did not quote the note or cite the page within the session." ;;
  teardown)
    "$MDA" stop --root "$(copy_of mda)" >/dev/null 2>&1 || true; echo "daemon stopped (copies kept under $T4)" ;;
  *) die "unknown command $cmd" ;;
esac
