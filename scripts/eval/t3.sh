#!/usr/bin/env bash
# T3, axis E (execution plan §4): what each arm's first build cost on the four corpora, per
# project and per 1,000 sections, from the archived records — mda's committed cards and their
# provenance, the qmd, graphify and BM25-over-files arm records — plus the size of the
# artifact each arm scores from, measured on this machine and written to
# evals/results/docsqa/T3/sizes.json (an artifact size is not a frozen input; the page says
# where it was measured). Incremental cost after one edited section is T4/M7 and the column
# says so. Numbers are never retyped: this script renders the page table.
#
# Usage: scripts/eval/t3.sh sizes            # measure the live artifacts → T3/sizes.json
#        scripts/eval/t3.sh table            # the page table from the records and sizes.json
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
set -euo pipefail
cmd="${1:?sizes|table}"; shift
T3="$RESULTS/T3"; mkdir -p "$T3"
ver="$("$MDA" --version)"; ver="${ver#mda }"

case "$cmd" in
  sizes)
    out='{}'
    for p in $PROJECTS; do
      d="$(project_dir "$p")"
      mda_b="$(stat -f %z "$RUN/$d/.markdownattractor/index.sqlite")"
      qmd_b="$(stat -f %z "$HOME/.cache/qmd/$p.sqlite" 2>/dev/null || echo null)"
      bm25_b="$(stat -f %z "$RUN/bm25-files/$p.sqlite" 2>/dev/null || echo null)"
      g_b=null; [ ! -f "$RESULTS/arms/graphs/graphify-$p.graph.json.gz" ] || g_b="$(gunzip -c "$RESULTS/arms/graphs/graphify-$p.graph.json.gz" | wc -c | tr -d ' ')"
      gh_b=null; [ ! -f "$RESULTS/arms/graphs/graphify-haiku-$p.graph.json.gz" ] || gh_b="$(gunzip -c "$RESULTS/arms/graphs/graphify-haiku-$p.graph.json.gz" | wc -c | tr -d ' ')"
      out="$(jq -c --arg p "$p" --argjson m "$mda_b" --argjson q "$qmd_b" --argjson b "$bm25_b" --argjson g "$g_b" --argjson gh "$gh_b" '. + {($p): {mda_store_bytes: $m, qmd_index_bytes: $q, bm25_table_bytes: $b, graphify_graph_bytes: $g, graphify_haiku_graph_bytes: $gh}}' <<<"$out")"
    done
    jq -n --argjson s "$out" --arg at "$(date -u +%FT%TZ)" --arg host "$(uname -m) $(sw_vers -productVersion 2>/dev/null || uname -r)" \
      '{measured_at: $at, host: $host, note: "live artifact sizes on the machine that ran the benchmark (mda store = .markdownattractor/index.sqlite incl. FTS content and f32 vectors; qmd index incl. its llm_cache; BM25-over-files FTS5 table; graphify graph.json)", sizes: $s}' > "$T3/sizes.json"
    echo "wrote $T3/sizes.json" ;;
  table)
    [ -f "$T3/sizes.json" ] || die "run sizes first"
    echo "| Project | sections | arm | first build wall-clock | model cost (list-price eq.) | tokens in / out | wall per 1K sections | cost per 1K sections | artifact size | incremental after one edit |"
    echo "|---|---|---|---|---|---|---|---|---|---|"
    for p in $PROJECTS; do
      prov="$RESULTS/cards-$ver-$p.json.provenance.json"; [ -f "$prov" ] || die "no $prov"
      sec="$(jq -r .sections "$prov")"; k="$(jq -n --argjson s "$sec" '$s / 1000')"
      sz="$(jq -c --arg p "$p" '.sizes[$p]' "$T3/sizes.json")"
      mb() { jq -nr --argjson b "$1" 'if $b == null then "n/a" else (($b / 1048576 * 10 | round) / 10 | tostring) + " MB" end'; }
      # mda: cards from the provenance; the raw index and attach+embed times from the T1 preflight's reconstruction (a clean rebuild from the committed cards)
      pf="$RESULTS/preflight/T1-$p.json"; idx_s="$(jq -r '.checks[] | select(.name == "reconstruction") | .index_s // "?"' "$pf" 2>/dev/null || echo "?")"; emb_s="$(jq -r '.checks[] | select(.name == "reconstruction") | .attach_embed_score_s // "?"' "$pf" 2>/dev/null || echo "?")"
      jq -r --arg p "$p" --argjson sec "$sec" --argjson k "$k" --arg idx "$idx_s" --arg emb "$emb_s" --arg sz "$(mb "$(jq .mda_store_bytes <<<"$sz")")" '
        def r0: . | round | tostring; def r2: (. * 100 | round) / 100 | tostring;
        "| \($p) | \($sec) | mda (cards + vectors) | raw index \($idx) s · summarise: one `claude -p` per section, wall not recorded at M1 · attach + embed \($emb) s | $\(.usage.cost_usd_list_price_equivalent | r2) (Haiku 4.5) | \(.usage.input_tokens) / \(.usage.output_tokens) | embed ≈ \(($emb | tonumber? // 0) / $k | r0) s | $\(.usage.cost_usd_list_price_equivalent / $k | r2) | \($sz) | one section: hash-keyed, only the edited section is re-summarised and re-embedded (measured at M7, T4) |"' "$prov"
      q="$RESULTS/arms/qmd-$p.json"
      jq -r --arg p "$p" --argjson sec "$sec" --argjson k "$k" --arg sz "$(mb "$(jq .qmd_index_bytes <<<"$sz")")" 'def r0: . | round | tostring;
        "| \($p) | \($sec) | qmd 2.8.3 | \(.build.total_s) s (embed \(.build.embed_s) s, local EmbeddingGemma on Metal) | $0 (no remote model call) | – | \(.build.total_s / $k | r0) s | $0 | \($sz) | `qmd update && qmd embed` (measured at M7, T4) |"' "$q"
      for arm in graphify graphify-haiku; do
        g="$RESULTS/arms/$arm-$p.json"; [ -f "$g" ] || continue
        jq -r --arg p "$p" --arg arm "$arm" --argjson sec "$sec" --argjson k "$k" --arg sz "$(mb "$(jq --arg a "$arm" '.[$a + "_graph_bytes" | sub("-"; "_")]' <<<"$sz")")" '
          def r0: . | round | tostring; def r2: (. * 100 | round) / 100 | tostring;
          (.build.model_usage // {} | to_entries | map(.value) | {inp: ((map((.uncached_input // 0) + (.cache_read // 0) + (.cache_creation // 0)) | add) // null), out: ((map(.output // 0) | add) // null)}) as $u |
          ((.failed_attempts // [] | map(.cost_usd_list_price // 0) | add) // 0) as $fail | (.failed_attempts // [] | length) as $nfail |
          if .build.completed == true then
            "| \($p) | \($sec) | \($arm) (\(.build.host_model_alias // "?")-hosted `/graphify`) | \(.build.wall_s) s · \(.build.turns) turns | $\(.build.cost_usd_list_price | r2)\(if $nfail > 0 then " + $" + ($fail | r2) + " in " + ($nfail | tostring) + " failed attempt(s)" else "" end) | \($u.inp // "?") / \($u.out // "?") | \(.build.wall_s / $k | r0) s | $\(.build.cost_usd_list_price / $k | r2) | \($sz) | `/graphify --update` skill flow through the login (measured at M7, T4) |"
          else
            "| \($p) | \($sec) | \($arm) | did not complete (\($nfail + (if .build.cost_usd_list_price != null then 1 else 0 end)) attempt(s)\(if .build.wall_s != null then ", last " + (.build.wall_s | tostring) + " s · " + ((.build.turns // 0) | tostring) + " turns" else "" end)) | $\(($fail + (.build.cost_usd_list_price // 0)) | r2) spent, no graph | – | – | – | – | – |"
          end' "$g"
      done
      b="$RESULTS/arms/bm25-files-$p.json"
      jq -r --arg p "$p" --argjson sec "$sec" --argjson k "$k" --arg sz "$(mb "$(jq .bm25_table_bytes <<<"$sz")")" 'def r1: (. * 10 | round) / 10 | tostring;
        if .build.index_s == null then "| \($p) | \($sec) | BM25-over-files | not recorded (the table was built at M2, its record written afterwards: `arms/bm25-files-\($p).json`) | $0 | – | – | $0 | \($sz) | rebuild the table (seconds) |"
        else "| \($p) | \($sec) | BM25-over-files | \(.build.index_s) s | $0 | – | \(.build.index_s / $k | r1) s | $0 | \($sz) | rebuild the table (seconds) |" end' "$b"
    done
    echo
    echo "Generated by \`scripts/eval/t3.sh table\` from \`evals/results/docsqa/cards-$ver-<project>.json.provenance.json\`, \`arms/<arm>-<project>.json\`, the T1 preflight reconstruction timings and \`T3/sizes.json\` ($(jq -r '.host' "$T3/sizes.json"), measured $(jq -r .measured_at "$T3/sizes.json")). Sections = the corpus's markdown sections as mda parses them (the unit for every arm's per-1K figure). Model costs are list-price equivalents of calls that went through the owner's Claude Code login. mda's summarisation wall-clock was not recorded at M1 (cards were built across sessions); its per-section cost is the provenance's. Incremental cost after one edit is T4 (M7)." ;;
  *) die "unknown command $cmd" ;;
esac
