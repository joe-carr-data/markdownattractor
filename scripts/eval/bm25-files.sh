#!/usr/bin/env bash
# The BM25-over-files control arm (execution plan §1.3, §2.1): one FTS5 table over whole
# pages (path + full text, unicode61 with diacritics removed, the tokenizer mda uses), no
# sections, no cards, no vectors, no model. It separates "sections and cards help" from
# "any index helps". The query form mirrors mda's: every whitespace term double-quoted and
# ANDed; when that matches nothing, the same terms ORed (mda's OR fallback). Pages ranked by
# bm25(); the first ten distinct pages (whole pages, so no deduplication is needed) become
# the rows `mda eval --arm-output` scores.
#
# Usage: scripts/eval/bm25-files.sh build <project>                 # writes the table under $RUN/bm25-files/<project>.sqlite and arms/bm25-files-<project>.json
#        scripts/eval/bm25-files.sh drive <project> <out.jsonl> [split=dev]
#        scripts/eval/bm25-files.sh record <project> [note]      # rewrite the arm record from the existing table (fingerprint, page count, coverage)
# Ties in bm25() are broken by path (ascending), so a run is deterministic (Codex M4).
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
cmd="${1:?build|drive}"; project="${2:?project}"; shift 2
ident "$project"
dir="$(project_dir "$project")"; corpus="$RUN/$dir"
db="$RUN/bm25-files/$project.sqlite"
ARMS="$RESULTS/arms"; mkdir -p "$ARMS" "$RUN/bm25-files"
case "$cmd" in
  build)
    [ ! -e "$db" ] || die "$db exists: a build is done once per freeze"
    t0=$(date +%s)
    n="$(python3 - "$corpus" "$db" <<'PY'
import os, sqlite3, sys
root, db = sys.argv[1], sys.argv[2]
con = sqlite3.connect(db)
con.execute("CREATE VIRTUAL TABLE pages USING fts5(path UNINDEXED, text, tokenize='unicode61 remove_diacritics 2')")
n = 0
for d, dirs, files in os.walk(root):
    dirs[:] = sorted(x for x in dirs if x not in ('.git', '.markdownattractor') and not x.startswith('.'))
    for f in sorted(files):
        if not f.lower().endswith(('.md', '.mdx', '.markdown')):
            continue
        p = os.path.join(d, f)
        if os.path.islink(p):
            continue
        rel = os.path.relpath(p, root).replace(os.sep, '/')
        with open(p, encoding='utf-8', errors='replace') as fh:
            con.execute("INSERT INTO pages(path, text) VALUES (?, ?)", (rel, fh.read()))
        n += 1
con.commit(); con.close(); print(n)
PY
)"
    t1=$(date +%s)
    total=0; present=0
    while IFS= read -r p; do total=$((total + 1)); [ -f "$corpus/$p" ] && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
    jq -n --arg project "$project" --argjson n "$n" --argjson s "$((t1 - t0))" --arg sqlite "$(python3 -c 'import sqlite3; print(sqlite3.sqlite_version)')" --argjson total "$total" --argjson present "$present" --arg db "${db/#$HOME/\~}" \
      '{arm: "bm25-files", project: $project, version: ("python3 sqlite3 " + $sqlite + " FTS5"), config: {tokenize: "unicode61 remove_diacritics 2", unit: "whole page", query: "every term double-quoted, AND; OR fallback when AND matches nothing", rank: "bm25()"}, build: {index_s: $s}, coverage: {files_indexed: $n, corpus_pages: $total, corpus_pages_indexed: $present, coverage: (if $total == 0 then 0 else $present / $total end)}, db: $db, latency: "none (quality only, plan §2.1)"}' > "$ARMS/bm25-files-$project.json"
    echo "built $db: $n pages in $((t1 - t0)) s · $present of $total corpus pages on disk · $ARMS/bm25-files-$project.json"
    ;;
  record)
    note="${1:-record rewritten from the existing table}"
    [ -f "$db" ] || die "no $db"
    n="$(sqlite3 "$db" 'SELECT count(*) FROM pages')"
    total=0; present=0
    while IFS= read -r p; do total=$((total + 1)); [ -f "$corpus/$p" ] && present=$((present + 1)); done < <(jq -r --arg pr "$project" 'select(.project == $pr) | .repository_source_path' "$RUN/docsqa-data/data/corpus.jsonl")
    prev="{}"; [ ! -f "$ARMS/bm25-files-$project.json" ] || prev="$(cat "$ARMS/bm25-files-$project.json")"
    jq -n --argjson prev "$prev" --arg project "$project" --argjson n "$n" --arg sqlite "$(python3 -c 'import sqlite3; print(sqlite3.sqlite_version)')" --argjson total "$total" --argjson present "$present" --arg db "${db/#$HOME/\~}" --arg fp "$(bm25_fingerprint "$project")" --arg note "$note" --arg at "$(date -u +%FT%TZ)" \
      '$prev + {arm: "bm25-files", project: $project, version: ("python3 sqlite3 " + $sqlite + " FTS5"), config: {tokenize: "unicode61 remove_diacritics 2", unit: "whole page", query: "every term double-quoted, AND; OR fallback when AND matches nothing", rank: "bm25(), ties by path"}, build: (($prev.build // {}) + {completed: true}), coverage: {files_indexed: $n, corpus_pages: $total, corpus_pages_indexed: $present, coverage: (if $total == 0 then 0 else $present / $total end)}, db: $db, table_fingerprint: $fp, latency: "none (quality only, plan §2.1)", record: {written_at: $at, note: $note}}' > "$ARMS/bm25-files-$project.json"
    echo "recorded $ARMS/bm25-files-$project.json (fingerprint $(jq -r .table_fingerprint "$ARMS/bm25-files-$project.json" | cut -c1-16)…, $n pages)"
    ;;
  drive)
    out="${1:?out.jsonl}"; split="${2:-dev}"
    safe_target "$out"; [ ! -e "$out" ] || die "$out exists"
    [ -f "$db" ] || die "no $db: build first"
    [ "$split" != holdout ] || die "the holdout is sealed (rule 0.2)"
    jq -r --arg s "$split" '.questions[] | select(.split == $s) | .id' "$RESULTS/$project/split.json" > "$out.ids"
    [ -s "$out.ids" ] || die "no questions in split $split for $project"
    python3 - "$db" "$RUN/docsqa-data/data/questions.jsonl" "$project" "$out.ids" "$out" <<'PY'
import json, sqlite3, sys, time
db, qfile, project, idfile, out = sys.argv[1:6]
ids = [l.strip() for l in open(idfile) if l.strip()]
text = {}
for line in open(qfile, encoding='utf-8'):
    r = json.loads(line)
    if r.get('project') == project and r['question_id'] in ids:
        text[r['question_id']] = r['query']
con = sqlite3.connect(db)
def escape(q):
    terms = [t.replace('"', '""') for t in q.split()]
    return ['"%s"' % t for t in terms]
with open(out, 'w', encoding='utf-8') as fh:
    for qid in ids:
        q = text[qid]
        terms = escape(q)
        t0 = time.time()
        rows = []
        form = 'and'
        if terms:
            rows = con.execute("SELECT path FROM pages WHERE pages MATCH ? ORDER BY bm25(pages), path LIMIT 10", (' '.join(terms),)).fetchall()
            if not rows:
                form = 'or'
                rows = con.execute("SELECT path FROM pages WHERE pages MATCH ? ORDER BY bm25(pages), path LIMIT 10", (' OR '.join(terms),)).fetchall()
        ms = int((time.time() - t0) * 1000)
        fh.write(json.dumps({"question_id": qid, "paths": [r[0] for r in rows], "truncated": False,
                             "request": {"form": form, "terms": len(terms)}, "wall_ms": ms}) + "\n")
print(len(ids))
PY
    rm -f "$out.ids"
    echo "wrote $out"
    ;;
  *) die "unknown command $cmd" ;;
esac
