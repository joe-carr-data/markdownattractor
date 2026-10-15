#!/usr/bin/env bash
# T5, axis D (execution plan §4 T5, plan docs/plans/2026-09-m8-t5-replay.md): the simulated
# historical replay. A fixed range of a repository's first-parent history is replayed into mda's
# store one step per day under the test-only clock (`MDA_NOW`, ADR-0006), the stored timestamps
# are validated against git (a mismatch voids the project's replay), and forty generated questions
# with git-computed ground truth are answered by every arm on its own copy of the final checkout,
# every arm with `.git` and `git` in Bash (lib.sh ARM_WITH_GIT=1). Dates throughout are the
# committer dates of the first-parent commits on `main` — the moment a change landed — as UTC
# calendar days; the plan said "author date", the harness uses committer dates because they are
# monotone along the first parent and are what a watcher would have seen.
#
# Layout under $RUN/t5/<project>/:
#   src/            the replay clone (history-bearing; `--shallow-since` range start − 7 days), the
#                   mda arm's working copy: its store is built by `replay`
#   steps.jsonl     the step list (one per UTC day with a markdown change under the content roots)
#   replay/<n>.json one record per replayed step; validation.json after `validate`
#   git/src qmd/src graphify/{graph.json,src}   the other arms' copies of the final checkout (+ .git)
#   runs/           manifest, rows, traces, grades of the answer runs
#
# Usage:
#   scripts/eval/t5.sh setup <project>            # clone, config, the step list → steps.jsonl (committed under T5/<project>/)
#   scripts/eval/t5.sh replay <project>           # every step not yet replayed: checkout, mtimes, MDA_NOW=<date> mda index; then the vectors
#   scripts/eval/t5.sh validate <project>         # stored timestamps vs git → T5/<project>/validation.json (mismatches void the replay)
#   scripts/eval/t5.sh arms <project>             # the git/qmd/graphify copies and the qmd index t5-qmd-<project>
#   scripts/eval/t5.sh questions <project>        # forty seeded questions with git ground truth → T5/<project>/questions.jsonl
#   scripts/eval/t5.sh preflight <project>        # freeze check, env, validation, copies at the pinned commit, three activation probes per arm
#   scripts/eval/t5.sh run <project> [runs=3] [arms]   # structured answers, T2's accounting; resumable
#   scripts/eval/t5.sh status <project>
#   scripts/eval/t5.sh grade <project>            # deterministic: set F1 ≥ 0.8 / date within one day → grades.jsonl
#   scripts/eval/t5.sh analysis <project> [--json] # mda eval --replay-analysis
#   scripts/eval/t5.sh table                      # the page table from the archived grades and analyses
#   scripts/eval/t5.sh redo <project> <id> <arm> <run>   # move one row and its traces to superseded/ (never deleted) for a disclosed re-measurement
# Env: T5_PROJECTS (prisma tailwind-css), T5_RANGE_DAYS (60), T5_MODEL (sonnet), T5_ARMS (git mda qmd graphify),
#      T5_JOBS (1), T5_MAX_TURNS (12), T5_TIMEOUT (600), T5_RETRIES (1), REPO/RUN/MDA (lib.sh).
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
set -euo pipefail
cmd="${1:?setup|replay|validate|arms|questions|preflight|run|status|grade|analysis|table|redo}"; shift
T5="$RUN/t5"; RES="$RESULTS/T5"; PROJECTS="${T5_PROJECTS:-prisma tailwind-css}"; RANGE_DAYS="${T5_RANGE_DAYS:-60}"; N_Q=40
MODEL="${T5_MODEL:-sonnet}"; ARMS="${T5_ARMS:-git mda qmd graphify}"; JOBS="${T5_JOBS:-1}"; MAX_TURNS="${T5_MAX_TURNS:-12}"; TIMEOUT="${T5_TIMEOUT:-600}"; RETRIES="${T5_RETRIES:-1}"
SOURCES="$RUN/docsqa-data/sources.json"; CORPUS="$RUN/docsqa-data/data/corpus.jsonl"
PREAMBLE="The current directory is a git checkout of a documentation site; you may run git. Dates are the UTC calendar days on which changes landed on the main branch. Answer through the structured output only: pages as repository-relative paths, sections as heading paths joined by ' › ', date as YYYY-MM-DD; leave the fields the question does not need empty."
SCHEMA='{"type":"object","properties":{"pages":{"type":"array","items":{"type":"string"}},"sections":{"type":"array","items":{"type":"string"}},"date":{"type":"string"},"evidence":{"type":"string"}},"required":["pages","sections","date","evidence"],"additionalProperties":false}'
unset_nested_session; unset_provider_keys
export ARM_WITH_GIT=1

src_field() { jq -r --arg id "$1" --arg f "$2" '.sources[] | select(.id == $id) | .[$f] | if type == "array" then join(" ") else . end' "$SOURCES"; }
pdir() { echo "$T5/$1"; }
graphify_build() { case "$1" in prisma) echo "$RUN/graphify/prisma-haiku" ;; *) echo "$RUN/graphify/$1" ;; esac; }   # Prisma: the Haiku graph (its Sonnet build never completed)
graphify_arm() { case "$1" in prisma) echo graphify-haiku ;; *) echo graphify ;; esac; }
# UTC day arithmetic without GNU date.
day_shift() { python3 -c 'import sys, datetime; d = datetime.date.fromisoformat(sys.argv[1]); print((d + datetime.timedelta(days=int(sys.argv[2]))).isoformat())' "$1" "$2"; }
store_sha() { sha256 "$1/.markdownattractor/index.sqlite"; }

case "$cmd" in
  setup)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; mkdir -p "$P" "$RES/$project"
    RANGE_DAYS="${2:-$RANGE_DAYS}"   # per project: Tailwind's docs change slowly (60 days gave 5 steps), so it replays 180
    repo="$(src_field "$project" repository)"; rev="$(src_field "$project" revision)"; cdir="$(src_field "$project" checkout_dir)"; roots="$(src_field "$project" content_roots)"
    PIN="$RUN/$cdir"; [ -d "$PIN/.markdownattractor" ] || die "no pinned store at $PIN"
    [ "$(git -C "$PIN" rev-parse HEAD)" = "$rev" ] || die "the pinned checkout is not at $rev"
    end_day="$(git -C "$PIN" log -1 --format=%cI "$rev" | python3 -c 'import sys, datetime; print(datetime.datetime.fromisoformat(sys.stdin.read().strip()).astimezone(datetime.timezone.utc).date().isoformat())')"
    start_day="$(day_shift "$end_day" "-$RANGE_DAYS")"; since="$(day_shift "$start_day" -7)"
    if [ ! -d "$SRC/.git" ]; then
      t0=$(date +%s); git clone -q --shallow-since="$since" --single-branch -b main "$repo" "$SRC"
      echo "clone: $SRC in $(( $(date +%s) - t0 )) s ($(du -sh "$SRC" | cut -f1); history since $since)"
    fi
    git -C "$SRC" cat-file -e "$rev^{commit}" || die "the pinned revision $rev is not in the clone"
    mkdir -p "$SRC/.markdownattractor"; cp "$PIN/.markdownattractor/config.toml" "$SRC/.markdownattractor/config.toml"
    # The step list: first-parent commits of the pinned revision whose committer day is in
    # [start_day, end_day]; one step per UTC day that changed a markdown file under a content
    # root, at the last such commit of the day (first-parent order); the pinned revision is the
    # final step. Every markdown change of the day (any path) is recorded for the mtimes.
    python3 - "$SRC" "$rev" "$start_day" "$end_day" "$roots" "$P/steps.jsonl" "$P/range.json" "$repo" "$since" <<'PY' || die "step list failed"
import datetime, json, subprocess, sys
src, rev, start, end, roots, out, rng, repo, since = sys.argv[1:10]
roots = roots.split()
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def is_md(p): return p.endswith((".md", ".mdx", ".markdown"))
def under_root(p): return any(p == r or p.startswith(r.rstrip("/") + "/") for r in roots)
commits = []  # oldest first
for line in git("rev-list", "--first-parent", "--reverse", "--format=%H %cI", rev).splitlines():
    if line.startswith("commit "): continue
    sha, ci = line.split(" ", 1)
    day = datetime.datetime.fromisoformat(ci).astimezone(datetime.timezone.utc)
    if start <= day.date().isoformat() <= end: commits.append((sha, day))
if not commits: sys.exit("no first-parent commits in range")
# the first in-range commit must have a parent inside the clone, or its diff is unknowable
first = commits[0][0]
if subprocess.run(["git", "-C", src, "cat-file", "-e", first + "^"], capture_output=True).returncode != 0: sys.exit(f"parent of the first in-range commit {first[:10]} is outside the clone; widen --shallow-since")
by_day = {}
for sha, day in commits:
    diff = git("diff", "--name-status", "-M", f"{sha}^", sha)
    changes = []
    for row in diff.splitlines():
        parts = row.split("\t"); st = parts[0][0]
        path = parts[-1]; old = parts[1] if st == "R" and len(parts) == 3 else None
        if is_md(path) or (old and is_md(old)): changes.append({"status": st, "path": path, **({"from": old} if old else {})})
    d = day.date().isoformat()
    e = by_day.setdefault(d, {"day": d, "shas": [], "changes": []})
    e["shas"].append((sha, day.isoformat())); e["changes"].extend(changes)
steps = []
for d in sorted(by_day):
    e = by_day[d]
    if not any(under_root(c["path"]) or under_root(c.get("from", "")) for c in e["changes"]) and e["shas"][-1][0] != rev: continue
    sha, when = e["shas"][-1]
    steps.append({"step": len(steps) + 1, "sha": sha, "date": datetime.datetime.fromisoformat(when).astimezone(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "day": d,
                  "commits": len(e["shas"]), "changes": e["changes"]})
if steps[-1]["sha"] != rev: sys.exit(f"the last step {steps[-1]['sha'][:10]} is not the pinned revision (the pinned day has later commits?)")
with open(out, "w") as fh:
    for s in steps: fh.write(json.dumps(s) + "\n")
json.dump({"repository": repo, "end_sha": rev, "start_day": start, "end_day": end, "range_days": (datetime.date.fromisoformat(end) - datetime.date.fromisoformat(start)).days,
           "shallow_since": since, "content_roots": roots, "first_parent_commits": len(commits), "steps": len(steps), "date_semantics": "committer date of the first-parent commit, UTC calendar day"}, open(rng, "w"), indent=1)
print(f"{len(steps)} steps over {len(commits)} first-parent commits, {start}..{end}; markdown changes under the roots: {sum(1 for s in steps for c in s['changes'] if under_root(c['path']))}")
PY
    cp "$P/steps.jsonl" "$RES/$project/steps.jsonl"; cp "$P/range.json" "$RES/$project/range.json"
    echo "steps → $RES/$project/steps.jsonl · range → $RES/$project/range.json" ;;

  replay)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; mkdir -p "$P/replay"
    [ -f "$P/steps.jsonl" ] || die "no steps.jsonl (run setup)"
    ! "$MDA" --json status --root "$SRC" 2>/dev/null | jq -e '.daemon != null' >/dev/null || die "a daemon is running on $SRC"
    first_day="$(jq -r 'select(.step == 1) | .date' "$P/steps.jsonl")"
    while IFS= read -r s; do
      n="$(jq -r .step <<<"$s")"; sha="$(jq -r .sha <<<"$s")"; at="$(jq -r .date <<<"$s")"
      [ -f "$P/replay/$n.json" ] && continue
      git -C "$SRC" checkout -q --detach "$sha"
      # mtimes: the step's changed markdown files get the step's date; at the first step every
      # tracked markdown file gets it too (history before the range is flattened to the first step).
      python3 - "$SRC" "$P/steps.jsonl" "$n" <<'PY' || die "mtimes failed"
import datetime, json, os, subprocess, sys
src, steps_f, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
steps = [json.loads(l) for l in open(steps_f)]
step = steps[n - 1]; ts = datetime.datetime.fromisoformat(step["date"].replace("Z", "+00:00")).timestamp()
if n == 1:
    files = [p for p in subprocess.run(["git", "-C", src, "ls-files"], check=True, capture_output=True, text=True).stdout.splitlines() if p.endswith((".md", ".mdx", ".markdown"))]
else:
    files = [c["path"] for c in step["changes"] if c["status"] != "D"]
for p in files:
    ap = os.path.join(src, p)
    if os.path.isfile(ap): os.utime(ap, (ts, ts))
print(f"mtimes set on {len(files)} files")
PY
      before="$("$MDA" --json status --root "$SRC" 2>/dev/null | jq -c '.counts' || echo '{}')"
      t0=$(date +%s)
      MDA_NOW="$at" "$MDA" index --root "$SRC" > "$P/replay/$n.log" 2>&1 || { tail -5 "$P/replay/$n.log"; die "mda index failed at step $n"; }
      # a round can leave sections pending (budget, limit); index until nothing is pending
      for ((k = 0; k < 20; k++)); do
        pend="$("$MDA" --json status --root "$SRC" | jq -r '.counts.pending')"; [ "$pend" = 0 ] && break
        MDA_NOW="$at" "$MDA" index --root "$SRC" >> "$P/replay/$n.log" 2>&1 || true
      done
      t1=$(date +%s); after="$("$MDA" --json status --root "$SRC" | jq -c '.counts')"
      jq -n --argjson n "$n" --arg sha "$sha" --arg at "$at" --argjson before "$before" --argjson after "$after" --argjson secs "$((t1 - t0))" --arg store "$(store_sha "$SRC")" --arg done_at "$(date -u +%FT%TZ)" \
        '{step: $n, sha: $sha, mda_now: $at, seconds: $secs, counts_before: $before, counts_after: $after, cost_usd_delta: (($after.total_cost_usd // 0) - ($before.total_cost_usd // 0)), store_sha256: $store, replayed_at: $done_at}' > "$P/replay/$n.json.tmp" && mv "$P/replay/$n.json.tmp" "$P/replay/$n.json"
      echo "step $n $sha ($at): $((t1 - t0)) s · docs $(jq -r .docs <<<"$after") · sections $(jq -r .sections <<<"$after") · summarized $(jq -r .summarized <<<"$after") · failed $(jq -r .failed <<<"$after") · +\$$(jq -r .cost_usd_delta "$P/replay/$n.json")"
    done < "$P/steps.jsonl"
    if [ ! -f "$P/replay/embeddings.json" ]; then
      t0=$(date +%s); "$MDA" rebuild --embeddings --root "$SRC" > "$P/replay/embeddings.log" 2>&1 || die "rebuild --embeddings failed"
      jq -n --argjson secs "$(( $(date +%s) - t0 ))" --arg store "$(store_sha "$SRC")" --argjson emb "$("$MDA" --json status --root "$SRC" | jq -c '.embeddings')" '{seconds: $secs, store_sha256: $store, embeddings: $emb}' > "$P/replay/embeddings.json"
      echo "vectors: $(( $(date +%s) - t0 )) s"
    fi
    mkdir -p "$RES/$project"; jq -c . "$P"/replay/[0-9]*.json | jq -s 'sort_by(.step)' > "$RES/$project/replay.json"
    echo "replay complete: $(jq length "$RES/$project/replay.json") steps · total $(jq '[.[].seconds] | add' "$RES/$project/replay.json") s · \$$(jq '[.[].cost_usd_delta] | add' "$RES/$project/replay.json") · store sha256 $(store_sha "$SRC" | cut -c1-12)… → $RES/$project/replay.json" ;;

  validate)
    # Every live document's updated_at, first_seen_at, created_at (+ source) and every section's
    # updated_at against the step list; every tombstone against the deletions. The expectations
    # derive from steps.jsonl (git), never from the store.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; mkdir -p "$RES/$project"
    python3 - "$SRC/.markdownattractor/index.sqlite" "$P/steps.jsonl" "$SRC" "$RES/$project/validation.json" <<'PY' || die "validation failed to run"
import json, os, sqlite3, subprocess, sys
db, steps_f, src, out = sys.argv[1:5]
steps = [json.loads(l) for l in open(steps_f)]
first = steps[0]["date"]
tracked = set(p for p in subprocess.run(["git", "-C", src, "ls-files"], check=True, capture_output=True, text=True).stdout.splitlines() if p.endswith((".md", ".mdx", ".markdown")))
# per path: the dates of its in-range changes, the step it first exists at, the step it was deleted at
changes, first_seen, deleted = {}, {}, {}
present_at_start = set(subprocess.run(["git", "-C", src, "ls-tree", "-r", "--name-only", steps[0]["sha"]], check=True, capture_output=True, text=True).stdout.splitlines())
for p in present_at_start:
    if p.endswith((".md", ".mdx", ".markdown")): first_seen[p] = first
for s in steps:
    for c in s["changes"]:
        p = c["path"]
        changes.setdefault(p, []).append(s["date"])
        if c["status"] == "A" or (c["status"] == "R"): first_seen.setdefault(p, s["date"])
        if c["status"] == "R" and c.get("from"): deleted[c["from"]] = s["date"]
        if c["status"] == "D": deleted[p] = s["date"]
        if c["status"] in ("A", "R") and p in deleted: del deleted[p]   # re-added later
con = sqlite3.connect(db)
mism, n_docs, n_secs, n_tomb = [], 0, 0, 0
for doc_id, rel, created, src_, updated, seen, deleted_at in con.execute("SELECT doc_id, rel_path, created_at, created_at_source, updated_at, first_seen_at, deleted_at FROM docs"):
    if deleted_at:
        n_tomb += 1
        exp = deleted.get(rel)
        if exp is None: mism.append({"path": rel, "kind": "tombstone_without_deletion", "stored": deleted_at})
        elif exp != deleted_at: mism.append({"path": rel, "kind": "deleted_at", "stored": deleted_at, "expected": exp})
        continue
    n_docs += 1
    if rel not in tracked: mism.append({"path": rel, "kind": "doc_not_tracked"}); continue
    exp_updated = changes[rel][-1] if rel in changes else first
    if updated != exp_updated: mism.append({"path": rel, "kind": "updated_at", "stored": updated, "expected": exp_updated})
    exp_seen = first_seen.get(rel, first)
    if seen != exp_seen: mism.append({"path": rel, "kind": "first_seen_at", "stored": seen, "expected": exp_seen})
    if created != exp_seen or src_ != "first_seen": mism.append({"path": rel, "kind": "created_at", "stored": f"{created} ({src_})", "expected": f"{exp_seen} (first_seen)"})
    allowed = set(changes.get(rel, [])) | {first}
    for sid, hp, sup in con.execute("SELECT section_id, heading_path, updated_at FROM sections WHERE doc_id = ?", (doc_id,)):
        n_secs += 1
        if sup not in allowed: mism.append({"path": rel, "kind": "section_updated_at", "section": sid, "stored": sup, "expected_one_of": sorted(allowed)[-3:]})
missing = sorted(p for p in tracked if p not in {r for (r,) in con.execute("SELECT rel_path FROM docs WHERE deleted_at IS NULL")} and not any(p.startswith(x) for x in ("node_modules/", "target/", "vendor/")))
rep = {"project": os.path.basename(os.path.dirname(src)), "steps": len(steps), "docs_checked": n_docs, "sections_checked": n_secs, "tombstones_checked": n_tomb,
       "tracked_markdown_not_indexed": len(missing), "tracked_markdown_not_indexed_sample": missing[:20], "mismatches": len(mism), "mismatch_kinds": {k: sum(1 for m in mism if m["kind"] == k) for k in sorted({m["kind"] for m in mism})},
       "mismatch_sample": mism[:50], "passed": len(mism) == 0, "semantics": "updated_at = committer date of the last in-range first-parent change, else the first step; first_seen_at/created_at (source first_seen) = the first step the file exists at; section updated_at ∈ {first step} ∪ the file's in-range change dates; deleted_at = the deleting step"}
json.dump(rep, open(out, "w"), indent=1)
print(f"{'PASS' if rep['passed'] else 'FAIL'}: {n_docs} docs, {n_secs} sections, {n_tomb} tombstones checked; {len(mism)} mismatches {rep['mismatch_kinds']}; {len(missing)} tracked markdown files not indexed (ignore rules)")
PY
    ;;

  arms)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; rev="$(src_field "$project" revision)"
    [ "$(git -C "$SRC" rev-parse HEAD)" = "$rev" ] || die "the replay clone is not at the pinned revision (replay incomplete?)"
    for a in git qmd; do
      if [ ! -d "$P/$a/src" ]; then mkdir -p "$P/$a/src"; rsync -a --exclude .markdownattractor "$SRC/" "$P/$a/src/"; echo "$a copy: $P/$a/src"; fi
    done
    G="$(graphify_build "$project")"; [ -f "$G/graph.json" ] || die "no graphify build at $G"
    if [ ! -d "$P/graphify/src" ]; then
      mkdir -p "$P/graphify"; rsync -a "$G/" "$P/graphify/"; rsync -a "$SRC/.git/" "$P/graphify/src/.git/"
      (cd "$P/graphify/src" && git reset -q --hard "$rev" 2>/dev/null || true)   # the build's files are the pinned checkout's; .git makes history readable
      echo "graphify copy: $P/graphify (graph sha256 $(sha256 "$P/graphify/graph.json" | cut -c1-12)…, history attached)"
    fi
    qi="t5-qmd-$project"
    if [ ! -f "$HOME/.config/qmd/$qi.yml" ]; then
      t0=$(date +%s); qmd --index "$qi" collection add "$P/qmd/src" --name "$qi" --mask '**/*.{md,mdx,markdown}' >/dev/null && qmd --index "$qi" update >/dev/null && qmd --index "$qi" embed >/dev/null
      echo "qmd index $qi built in $(( $(date +%s) - t0 )) s"
    fi
    echo "arms ready: git $P/git/src · mda $SRC · qmd $P/qmd/src ($qi) · graphify $P/graphify" ;;

  questions)
    # Candidates are enumerated exhaustively from the step list and the corpus page list, kept
    # when the ground truth is answerable (1–12 members; a date inside the range), then drawn
    # in sha256(seed ‖ candidate key) order, 14 "which" / 13 "when" / 13 "added" (a short type
    # is topped up from the others and the counts are recorded).
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; cdir="$(src_field "$project" checkout_dir)"; roots="$(src_field "$project" content_roots)"
    jq -r --arg p "$project" --arg c "$cdir/" 'select(.project == $p) | .repository_source_path | ltrimstr($c)' "$CORPUS" | sort -u > "$P/corpus-pages.txt"
    python3 - "$SRC" "$P/steps.jsonl" "$P/corpus-pages.txt" "$roots" "$SEED" "$N_Q" "$P/questions.jsonl" "$MDA" <<'PY' || die "question generation failed"
import collections, datetime, hashlib, json, os, subprocess, sys, tempfile
src, steps_f, pages_f, roots, seed, n_q, out, mda = sys.argv[1:9]
roots = roots.split(); steps = [json.loads(l) for l in open(steps_f)]
pages = set(l.strip() for l in open(pages_f) if l.strip())
days = [s["day"] for s in steps]; start, end = days[0], days[-1]
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def parse(sha, path):
    blob = subprocess.run(["git", "-C", src, "show", f"{sha}:{path}"], capture_output=True, text=True)
    if blob.returncode != 0: return None
    with tempfile.NamedTemporaryFile("w", suffix=os.path.splitext(path)[1], delete=False, encoding="utf-8") as fh: fh.write(blob.stdout); tmp = fh.name
    try:
        r = subprocess.run([mda, "--json", "parse", tmp], capture_output=True, text=True)
        if r.returncode != 0: return None
        return {" › ".join(s["heading_path"]): s["hash"] for s in json.loads(r.stdout)["sections"] if s["heading_path"]}
    finally: os.unlink(tmp)
# page-level change calendar (corpus pages only)
changed_on = collections.defaultdict(set); added_on = {}; deleted_on = {}
for s in steps:
    for c in s["changes"]:
        p = c["path"]
        if p not in pages and c.get("from") not in pages: continue
        if c["status"] == "D": deleted_on[p] = s["day"]; continue
        changed_on[p].add(s["day"])
        if c["status"] in ("A", "R") and p in pages: added_on.setdefault(p, s["day"])
def dir_of(p, depth):
    parts = p.split("/")
    for r in roots:
        rp = r.rstrip("/").split("/")
        if parts[:len(rp)] == rp: return "/".join(parts[:len(rp) + depth]) if len(parts) > len(rp) + depth else None
    return None
cands = []
# (a) which pages changed between D1 and D2 (inclusive), under a directory, windows of 1–14 days between step days
for i, d1 in enumerate(days):
    for d2 in days[i:]:
        if (datetime.date.fromisoformat(d2) - datetime.date.fromisoformat(d1)).days > 14: break
        for depth in (0, 1):
            by_dir = collections.defaultdict(set)
            for p, ds in changed_on.items():
                if any(d1 <= d <= d2 for d in ds):
                    d = dir_of(p, depth)
                    if d: by_dir[d].add(p)
            for d, ps in by_dir.items():
                if 1 <= len(ps) <= 12:
                    cands.append({"type": "which", "kind": "pages", "key": f"which-pages|{d}|{d1}|{d2}", "q": f"Which documentation pages under `{d}/` changed between {d1} and {d2} (inclusive)?",
                                  "truth": {"pages": sorted(ps)}, "window": [d1, d2], "dir": d})
# (a') which sections of a page changed at a step; (b') when a section was added; from the parsed blobs
sec_hist = {}
for s in steps:
    for c in s["changes"]:
        p = c["path"]
        if p not in pages or c["status"] == "D": continue
        now = parse(s["sha"], p)
        if now is None: continue
        prev_sha = subprocess.run(["git", "-C", src, "rev-parse", f"{s['sha']}^"], capture_output=True, text=True).stdout.strip()
        before = parse(prev_sha, p) if c["status"] != "A" else {}
        if before is None: before = {}
        changed = sorted(h for h, hh in now.items() if before.get(h) != hh)
        if 1 <= len(changed) <= 12 and before:
            cands.append({"type": "which", "kind": "sections", "key": f"which-sections|{p}|{s['day']}", "q": f"Which sections of `{p}` changed on {s['day']}?", "truth": {"sections": changed}, "window": [s["day"], s["day"]], "page": p})
        if before:
            for h in now:
                if h not in before: sec_hist.setdefault((p, h), s["day"])
# (b) when was a page last changed (pages changed in range)
for p, ds in changed_on.items():
    if p in deleted_on: continue
    cands.append({"type": "when", "kind": "page", "key": f"when-page|{p}", "q": f"On what day was `{p}` last changed?", "truth": {"date": max(ds)}, "page": p})
for (p, h), d in sec_hist.items():
    cands.append({"type": "when", "kind": "section", "key": f"when-section|{p}|{h}", "q": f"On what day was the section `{h}` added to `{p}`?", "truth": {"date": d}, "page": p, "section": h})
# (c) which pages were added between D1 and D2 (calendar months clipped to the range), under a directory
months = sorted({d[:7] for d in days})
for m in months:
    d1 = max(start, m + "-01"); last = (datetime.date.fromisoformat(m + "-28") + datetime.timedelta(days=4)).replace(day=1) - datetime.timedelta(days=1); d2 = min(end, last.isoformat())
    for depth in (0, 1):
        by_dir = collections.defaultdict(set)
        for p, d in added_on.items():
            if d1 <= d <= d2 and p not in deleted_on:
                dd = dir_of(p, depth)
                if dd: by_dir[dd].add(p)
        for dd, ps in by_dir.items():
            if 1 <= len(ps) <= 12:
                cands.append({"type": "added", "kind": "pages", "key": f"added|{dd}|{d1}|{d2}", "q": f"Which documentation pages under `{dd}/` were added between {d1} and {d2} (inclusive)?", "truth": {"pages": sorted(ps)}, "window": [d1, d2], "dir": dd})
key = lambda k: hashlib.sha256((seed + "\0" + k).encode()).hexdigest()
for c in cands: c["order"] = key(c["key"])
cands.sort(key=lambda c: c["order"])
want = {"which": 14, "when": 13, "added": 13}; n = int(n_q)
pool = {t: [c for c in cands if c["type"] == t] for t in want}
chosen = []
for t, k in want.items():
    kinds = sorted({c["kind"] for c in pool[t]}); picked = []
    while len(picked) < k and any(c not in picked for c in pool[t]):
        for kind in kinds:   # round-robin over kinds (pages/sections, page/section) in seeded order
            nxt = next((c for c in pool[t] if c["kind"] == kind and c not in picked), None)
            if nxt is not None and len(picked) < k: picked.append(nxt)
    chosen += picked
if len(chosen) < n:
    rest = [c for c in cands if c not in chosen]
    chosen += rest[:n - len(chosen)]
chosen.sort(key=lambda c: c["order"])
with open(out, "w", encoding="utf-8") as fh:
    for i, c in enumerate(chosen, 1):
        c = dict(c); c["id"] = f"t5-{i:02d}-{c['type']}"; c["scoring"] = "set F1 ≥ 0.8" if c["type"] != "when" else "within one day"
        c["evidence"] = {"steps": [s["sha"][:12] for s in steps if (c.get("window") and c["window"][0] <= s["day"] <= c["window"][1]) or (c.get("truth", {}).get("date") == s["day"])][:20], "date_semantics": "first-parent committer date, UTC day"}
        fh.write(json.dumps(c, ensure_ascii=False) + "\n")
counts = collections.Counter(c["type"] for c in chosen)
print(f"candidates: which {len(pool['which'])} (pages {sum(1 for c in pool['which'] if c['kind']=='pages')}, sections {sum(1 for c in pool['which'] if c['kind']=='sections')}) · when {len(pool['when'])} · added {len(pool['added'])}; drawn {len(chosen)}: {dict(counts)} → {out}")
PY
    mkdir -p "$RES/$project"; cp "$P/questions.jsonl" "$RES/$project/questions.jsonl"; echo "questions → $RES/$project/questions.jsonl ($(sha256 "$RES/$project/questions.jsonl" | cut -c1-12)…)" ;;

  status)
    project="${1:?project}"; P="$(pdir "$project")"
    echo "steps: $(grep -c . "$P/steps.jsonl" 2>/dev/null || echo 0) · replayed: $(ls "$P/replay" 2>/dev/null | grep -c '^[0-9]*\.json$' || true) · validation: $(jq -r '.passed // "n/a"' "$RES/$project/validation.json" 2>/dev/null || echo n/a) · questions: $(grep -c . "$P/questions.jsonl" 2>/dev/null || echo 0) · rows: $(ls "$P/runs/rows" 2>/dev/null | wc -l | tr -d ' ')" ;;

  *) ;;   # the answer phase and the renderers follow below
esac

[ "$cmd" = run ] || [ "$cmd" = grade ] || [ "$cmd" = analysis ] || [ "$cmd" = table ] || [ "$cmd" = preflight ] || [ "$cmd" = redo ] || exit 0

# ---------------------------------------------------------------- the answer phase (T2's machinery, structured answers)
assemble() { local out="$1"; : > "$out/runs.jsonl"; local f; for f in "$out"/rows/*.json; do [ -f "$f" ] || continue; jq -c . "$f" >> "$out/runs.jsonl"; done; }
run_once() { # corpus trace question args... -> exit code (124 = timeout)
  local corpus="$1" trace="$2" q="$3"; shift 3
  local pid rc=0 t=0
  (cd "$corpus" && exec claude "$@" -- "$PREAMBLE Question: $q" </dev/null >"$trace" 2>"$trace.err") & pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$t" -ge "$TIMEOUT" ]; then kill -TERM "$pid" 2>/dev/null; sleep 2; kill -KILL "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; return 124; fi
    sleep 1; t=$((t + 1))
  done
  wait "$pid" || rc=$?
  return "$rc"
}
row_of() { # id arm run q trace attempts-json wall want-regex -> json (T2's row plus the structured answer and the arm's tool use)
  jq -c -s --arg id "$1" --arg arm "$2" --argjson run "$3" --arg q "$4" --argjson attempts "$6" --argjson wall "$7" --arg want "$8" --arg trace "$(basename "$5")" '
    (map(select(.type == "result")) | last) as $res |
    ([.[] | select(.type == "stream_event" and .event.type == "message_delta") | .event.usage
      | {inp: ((.input_tokens // 0) + (.cache_read_input_tokens // 0) + (.cache_creation_input_tokens // 0)), out: (.output_tokens // 0)}]) as $turns |
    ([range(1; $turns | length) as $t | ($turns[$t].inp - $turns[$t - 1].inp - $turns[$t - 1].out)]) as $deltas |
    (map(select(.type == "assistant")) | map(.message.content[]? | select(.type == "tool_use") | {id, name, input})) as $uses |
    (map(select(.type == "user")) | map(.message.content[]? | select(type == "object" and .type == "tool_result") | {id: .tool_use_id, error: (.is_error // false)})) as $results |
    ($uses | map(. as $u | {name: $u.name, ok: (($results | map(select(.id == $u.id and (.error | not))) | length) > 0)})) as $calls |
    (map(select(.type == "assistant")) | map(.message.model // empty) | unique) as $models |
    ($attempts | last) as $last |
    {id: $id, arm: $arm, run: $run, q: $q, trace: $trace, attempts: $attempts, exit_code: $last.exit_code, wall_s: $wall,
     structured: ($res.structured_output // null), answer_text: (($res.result // "") | tostring | .[:600]),
     turns: ($res.num_turns // null), turns_with_usage: ($turns | length),
     input_tokens: (if $res == null then null else (($res.usage.input_tokens // 0) + ($res.usage.cache_read_input_tokens // 0) + ($res.usage.cache_creation_input_tokens // 0)) end),
     output_tokens: ($res.usage.output_tokens // null),
     source_tokens: (if ($turns | length) < 2 then 0 else ($deltas | map(if . < 0 then 0 else . end) | add) end),
     source_tokens_signed: (if ($turns | length) < 2 then 0 else ($deltas | add) end),
     tool_calls: ($calls | length), tools: ($calls | map(.name)), failed_calls: ($calls | map(select(.ok | not)) | length),
     git_commands: ($uses | map(select(.name == "Bash") | .input.command // "" | .[:80])),
     arm_tool_used: (($calls | map(select(.name | test($want))) | length) > 0),
     cost_usd: ($res.total_cost_usd // null), cost_usd_all_attempts: ($attempts | map(.cost_usd // 0) | add), attempts_made: ($attempts | length), models: $models,
     error: (($res == null) or ($res.is_error // false) or ($last.exit_code != 0) or ($res.structured_output == null)),
     stderr: (if (($res == null) or ($last.exit_code != 0)) then $last.stderr else null end)}' "$5"
}
launch_env() { # project arm
  local P; P="$(pdir "$1")"
  case "$2" in
    mda) ARM_CORPUS_OVERRIDE="$P/src" ;; git) ARM_CORPUS_OVERRIDE="$P/git/src" ;;
    qmd) ARM_CORPUS_OVERRIDE="$P/qmd/src"; export ARM_QMD_INDEX="t5-qmd-$1" ;;
    graphify) ARM_CORPUS_OVERRIDE="$P/graphify/src"; export ARM_GRAPHIFY_DIR="$P/graphify" ;;
  esac; export ARM_CORPUS_OVERRIDE
}
clear_env() { unset ARM_CORPUS_OVERRIDE ARM_QMD_INDEX ARM_GRAPHIFY_DIR; }
arm_name() { case "$2" in graphify) graphify_arm "$1" ;; *) echo "$2" ;; esac; }

case "$cmd" in
  run)
    project="${1:?project}"; runs="${2:-3}"; arms="${3:-$ARMS}"; arms="${arms//,/ }"; ident "$project"; P="$(pdir "$project")"; out="$P/runs"
    [ -f "$RES/$project/questions.jsonl" ] || die "no committed questions for $project"; questions="$RES/$project/questions.jsonl"
    [ "$(jq -r .passed "$RES/$project/validation.json")" = true ] || die "the replay of $project is not validated (or voided): no answer runs"
    mkdir -p "$out/rows" "$out/traces"
    common=(--print --no-session-persistence --model "$MODEL" --max-turns "$MAX_TURNS" --output-format stream-json --verbose
            --include-partial-messages --permission-mode dontAsk --strict-mcp-config --json-schema "$SCHEMA")
    launches='{}'; trap '[ "${#ARM_TMP[@]:-0}" = 0 ] || rm -f "${ARM_TMP[@]}"' EXIT
    for arm in $arms; do launch_env "$project" "$arm"; arm_launch "$(arm_name "$project" "$arm")" "$project" "$MODEL"; clear_env; launches="$(jq -c --arg a "$arm" --argjson l "$ARM_LAUNCH" '. + {($a): $l}' <<<"$launches")"; done
    manifest="$(jq -n --arg project "$project" --arg model "$MODEL" --argjson runs "$runs" --arg arms "$arms" --arg qf "$questions" --arg qsha "$(sha256 "$questions")" \
      --argjson ids "$(jq -c '[.id]' "$questions" | jq -s 'add')" --argjson launches "$launches" --arg mda "$("$MDA" --version)" --arg sha "$(git -C "$REPO" rev-parse HEAD)" \
      --argjson max_turns "$MAX_TURNS" --argjson timeout "$TIMEOUT" --argjson retries "$RETRIES" --arg at "$(date -u +%FT%TZ)" --arg mda_sha "$(sha256 "$MDA")" --arg preamble "$PREAMBLE" --argjson schema "$SCHEMA" \
      --arg store "$(store_sha "$P/src")" --arg val "$(sha256 "$RES/$project/validation.json")" --arg steps "$(sha256 "$RES/$project/steps.jsonl")" \
      '{table: "T5", project: $project, model: $model, runs: $runs, arms: ($arms | split(" ")), question_ids: $ids, questions_file: $qf, questions_sha256: $qsha,
        launches: $launches, mda: $mda, mda_sha256: $mda_sha, preamble: $preamble, answer_schema: $schema, store_sha256: $store, validation_sha256: $val, steps_sha256: $steps, source_commit: $sha,
        max_turns: $max_turns, timeout_s: $timeout, retries: $retries, recorded_at: $at,
        rules: {failures: "a missing, errored, timed-out run or one without a structured answer is a row scored 0 correct with its effort as recorded (rule 0.3)", tokens: "transcript usage per turn from message_delta (rule 0.7)", servers: "every run starts its MCP server cold (rule 0.9)", git: "every arm has .git and Bash restricted to git (plan §4 T5)"}}')"
    fingerprint() { jq -S 'del(.recorded_at, .source_commit) | .launches |= map_values(.claude_flags |= map(if test("mda-arm-mcp|qmd-skill") then "<tmp>" else . end))' "$@"; }
    if [ -f "$out/manifest.json" ]; then
      fingerprint "$out/manifest.json" > "$out/.m.a"; fingerprint <<<"$manifest" > "$out/.m.b"
      diff "$out/.m.a" "$out/.m.b" > "$out/.m.diff" || die "$out/manifest.json describes another configuration (see $out/.m.diff)"
      rm -f "$out/.m.a" "$out/.m.b" "$out/.m.diff"; echo "resuming $out"
    else echo "$manifest" > "$out/manifest.json"; fi
    one() { # id q arm run
      local id="$1" q="$2" arm="$3" r="$4" key trace attempts='[]' rc t0 t1 a
      key="$id-$arm-$r"; trace="$out/traces/$key.jsonl"
      launch_env "$project" "$arm"; arm_launch "$(arm_name "$project" "$arm")" "$project" "$MODEL"; clear_env
      t0=$(date +%s)
      for ((a = 1; a <= RETRIES + 1; a++)); do
        rc=0; run_once "$ARM_CORPUS" "$trace" "$q" "${common[@]}" --setting-sources "$ARM_SETTING_SOURCES" "${ARM_ARGS[@]}" || rc=$?
        use="$(jq -c -s '(map(select(.type == "result")) | last) as $r | if $r == null then {input_tokens: null, output_tokens: null, cost_usd: null, turns: null} else {input_tokens: (($r.usage.input_tokens // 0) + ($r.usage.cache_read_input_tokens // 0) + ($r.usage.cache_creation_input_tokens // 0)), output_tokens: ($r.usage.output_tokens // null), cost_usd: ($r.total_cost_usd // null), turns: ($r.num_turns // null)} end' "$trace" 2>/dev/null || echo '{}')"
        attempts="$(jq -c --argjson rc "$rc" --argjson use "$use" --arg err "$( (tail -c 300 "$trace.err" 2>/dev/null || true) | tr '\n' ' ')" --arg at "$(date -u +%FT%TZ)" '. + [{attempt: (length + 1), exit_code: $rc, timed_out: ($rc == 124), stderr: $err, at: $at} + $use]' <<<"$attempts")"
        if [ "$rc" = 0 ] && jq -e 'select(.type == "result") | .structured_output' "$trace" >/dev/null 2>&1; then break; fi
        [ "$a" -le "$RETRIES" ] && cp "$trace" "$trace.attempt$a" 2>/dev/null || true
      done
      t1=$(date +%s); [ -s "$trace.err" ] || rm -f "$trace.err"
      row_of "$id" "$arm" "$r" "$q" "$trace" "$attempts" "$((t1 - t0))" "$ARM_WANT" > "$out/rows/$key.json.tmp" && mv "$out/rows/$key.json.tmp" "$out/rows/$key.json"
      [ "${#ARM_TMP[@]}" = 0 ] || rm -f "${ARM_TMP[@]}"
      if [ "$(jq -r .error "$out/rows/$key.json")" = true ]; then echo "$id $arm run $r: ERROR (exit $(jq -r .exit_code "$out/rows/$key.json"))"; else echo "$id $arm run $r: $(jq -r '"\(.tool_calls) calls · \(.source_tokens) source tokens · \(.wall_s) s · tool \(.arm_tool_used)"' "$out/rows/$key.json")"; fi
    }
    n_total=0; n_done=0
    while IFS= read -r qline; do
      id="$(jq -r .id <<<"$qline")"; q="$(jq -r .q <<<"$qline")"
      for arm in $arms; do for ((r = 1; r <= runs; r++)); do
        n_total=$((n_total + 1)); key="$id-$arm-$r"
        if [ -f "$out/rows/$key.json" ]; then n_done=$((n_done + 1)); continue; fi
        while [ "$(jobs -rp | wc -l | tr -d ' ')" -ge "$JOBS" ]; do sleep 1; done
        one "$id" "$q" "$arm" "$r" &
        [ "$JOBS" -gt 1 ] || wait
      done; done
    done < <(jq -c . "$questions")
    wait; assemble "$out"
    n_err="$(jq -s 'map(select(.error)) | length' "$out/runs.jsonl")"
    echo "$out: $(grep -c . "$out/runs.jsonl") of $n_total rows ($n_done were present; $n_err error rows, kept)"; [ "$n_err" = 0 ] || exit 2 ;;

  grade)
    # Deterministic. Pages: repository-relative paths after stripping `./`, a leading
    # `<checkout_dir>/` or `<project>/` (qmd's collection prefix); an answer path that is the
    # unique suffix of one corpus page resolves to it; set F1 against the truth, correct when
    # F1 ≥ 0.8. Sections: heading paths compared after whitespace/case folding; an answer that
    # gives only the last heading matches a truth path ending in it (declared leniency). Dates:
    # correct when within one day of the truth. A failed run scores 0 with its effort kept.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; out="$P/runs"; [ -f "$out/manifest.json" ] || die "no manifest"; assemble "$out"
    cdir="$(src_field "$project" checkout_dir)"
    python3 - "$out/runs.jsonl" "$RES/$project/questions.jsonl" "$P/corpus-pages.txt" "$cdir" "$project" "$out/grades.jsonl" <<'PY' || die "grading failed"
import datetime, json, re, sys
runs_f, q_f, pages_f, cdir, project, out = sys.argv[1:7]
qs = {json.loads(l)["id"]: json.loads(l) for l in open(q_f, encoding="utf-8")}
pages = [l.strip() for l in open(pages_f) if l.strip()]
def norm_page(p):
    p = p.strip().strip("`'\"").lstrip("./")
    for pre in (cdir + "/", project + "/"):
        if p.startswith(pre): p = p[len(pre):]
    if p in pages: return p
    hits = [c for c in pages if c.endswith("/" + p) or c == p]
    return hits[0] if len(hits) == 1 else p
def norm_sec(s): return re.sub(r"\s+", " ", s.strip().strip("`'\"")).lower()
def f1(a, b):
    a, b = set(a), set(b)
    if not a and not b: return 1.0
    tp = len(a & b)
    if tp == 0: return 0.0
    pr, rc = tp / len(a), tp / len(b); return 2 * pr * rc / (pr + rc)
def sec_match(ans, truth):
    ans_n = {norm_sec(x) for x in ans}; truth_n = {norm_sec(x): x for x in truth}
    matched = set(); used = set()
    for a in ans_n:
        for t in truth_n:
            if t in used: continue
            if a == t or t.endswith(" › " + a) or a.endswith(" › " + t):
                matched.add(a); used.add(t); break
    a, b = len(ans_n), len(truth_n); tp = len(matched)
    if tp == 0: return 0.0
    pr, rc = tp / a, tp / b; return 2 * pr * rc / (pr + rc)
n = 0
with open(out, "w", encoding="utf-8") as fh:
    for l in open(runs_f, encoding="utf-8"):
        r = json.loads(l); q = qs[r["id"]]; s = r.get("structured") or {}
        g = {"id": r["id"], "arm": r["arm"], "run": r["run"], "type": q["type"], "kind": q.get("kind"), "error": bool(r["error"]), "tool_calls": r["tool_calls"], "source_tokens": r["source_tokens"],
             "wall_s": r["wall_s"], "cost_usd": r.get("cost_usd"), "arm_tool_used": r.get("arm_tool_used"), "git_used": any(c.startswith("git") for c in r.get("git_commands", []))}
        if r["error"] or not s: g.update({"correct": 0, "score": None, "detail": "no structured answer"})
        elif q["type"] == "when":
            d = (s.get("date") or "").strip()[:10]
            try: delta = abs((datetime.date.fromisoformat(d) - datetime.date.fromisoformat(q["truth"]["date"])).days)
            except ValueError: delta = None
            g.update({"correct": int(delta is not None and delta <= 1), "score": delta, "detail": f"answer {d or '∅'} vs {q['truth']['date']}"})
        elif "sections" in q["truth"]:
            sc = sec_match(s.get("sections") or [], q["truth"]["sections"]); g.update({"correct": int(sc >= 0.8), "score": round(sc, 3), "detail": f"{len(s.get('sections') or [])} answered vs {len(q['truth']['sections'])}"})
        else:
            ans = sorted({norm_page(p) for p in (s.get("pages") or [])}); sc = f1(ans, q["truth"]["pages"]); g.update({"correct": int(sc >= 0.8), "score": round(sc, 3), "detail": f"{len(ans)} answered vs {len(q['truth']['pages'])}"})
        fh.write(json.dumps(g, ensure_ascii=False) + "\n"); n += 1
print(f"{n} rows graded → {out}")
PY
    mkdir -p "$RES/$project"; cp "$out/grades.jsonl" "$RES/$project/grades.jsonl"; cp "$out/manifest.json" "$RES/$project/manifest.json"
    jq -s 'group_by(.arm) | map({(.[0].arm): {rows: length, correct: (map(.correct) | add), errors: (map(select(.error)) | length), git_used: (map(select(.git_used)) | length), tool_used: (map(select(.arm_tool_used)) | length)}}) | add' "$out/grades.jsonl" ;;

  analysis)
    project="${1:?project}"; shift; ident "$project"
    "$MDA" ${1:+"$1"} eval --replay-analysis "$RES/$project/grades.jsonl" --manifest "$RES/$project/manifest.json" --mda-arm mda --comparators "$(printf '%s' "$ARMS" | tr ' ' ',' | sed 's/mda,//; s/,mda//')" ;;

  redo)
    project="${1:?project}"; id="${2:?id}"; arm="${3:?arm}"; r="${4:?run}"; P="$(pdir "$project")"; out="$P/runs"; key="$id-$arm-$r"
    sup="$out/superseded/$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$sup"
    for f in "$out/rows/$key.json" "$out/traces/$key.jsonl" "$out/traces/$key.jsonl.err" "$out"/traces/"$key".jsonl.attempt*; do [ -e "$f" ] && mv "$f" "$sup/"; done
    echo "moved $key to $sup (disclose the replacement on the page)" ;;

  preflight)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; rev="$(src_field "$project" revision)"
    report="$RESULTS/preflight/T5-$project.json"; safe_target "$report"; checks='[]'; failed=0
    rec() { checks="$(jq -c --arg n "$1" --arg s "$2" --arg d "$3" '. + [{name: $n, status: $s, detail: $d}]' <<<"$checks")"; [ "$2" != FAIL ] || failed=$((failed + 1)); echo "$2 $1: $3"; }
    jq -n --arg p "$project" --arg at "$(date -u +%FT%TZ)" '{table: "T5", project: $p, run_at: $at, status: "in progress", passed: false}' > "$report"
    if env | grep -qE '^[A-Z_]*_API_KEY='; then rec env FAIL "a provider key is exported"; else rec env ok "no provider key · claude $(claude --version 2>/dev/null | head -1)"; fi
    if out="$("$REPO/scripts/eval/freeze.sh" --protocol final --table T5 --check 2>&1)"; then rec frozen ok "$out"; else rec frozen FAIL "$out"; fi
    if [ "$(jq -r .passed "$RES/$project/validation.json" 2>/dev/null)" = true ]; then rec validation ok "$(jq -r '"\(.docs_checked) docs, \(.sections_checked) sections, 0 mismatches"' "$RES/$project/validation.json")"; else rec validation FAIL "validation.json missing or not passed"; fi
    [ "$(store_sha "$P/src")" = "$(jq -r .store_sha256 "$P/replay/embeddings.json" 2>/dev/null)" ] && rec store ok "mda store unchanged since the replay ($(store_sha "$P/src" | cut -c1-12)…)" || rec store FAIL "mda store differs from the end of the replay"
    for a in $ARMS; do launch_env "$project" "$a"; d="$ARM_CORPUS_OVERRIDE"; clear_env; h="$(git -C "$d" rev-parse HEAD 2>/dev/null || echo none)"; [ "$h" = "$rev" ] && rec "copy-$a" ok "$d at $rev" || rec "copy-$a" FAIL "$d at $h"; done
    [ -f "$HOME/.config/qmd/t5-qmd-$project.yml" ] && rec qmd-index ok "t5-qmd-$project $(qmd_fingerprint "t5-qmd-$project" | cut -c1-12)…" || rec qmd-index FAIL "no index t5-qmd-$project"
    # activation probes (rule 0.5): the first three questions of the T2 sample through probe.sh,
    # each arm on its copy with git available (T4 lesson: a question naming the page proves nothing)
    mkdir -p "$RESULTS/preflight/T5-$project/probes"; : > "$RESULTS/preflight/T5-$project/probes/summary.jsonl"
    for a in $ARMS; do
      n_ok=0
      for qid in $(head -3 "$RESULTS/T2/$project/questions.jsonl" | jq -r .id); do
        launch_env "$project" "$a"; rc=0
        line="$("$REPO/scripts/eval/probe.sh" "$(arm_name "$project" "$a")" "$project" "$qid" "$RESULTS/preflight/T5-$project/probes/$a-${qid//[^A-Za-z0-9_.-]/_}.jsonl" "$MODEL" 2>>"$P/probes.err")" || rc=$?
        clear_env; [ -z "$line" ] || echo "$line" >> "$RESULTS/preflight/T5-$project/probes/summary.jsonl"
        [ "$rc" = 0 ] && n_ok=$((n_ok + 1))
      done
      if [ "$n_ok" = 3 ]; then rec "probes-$a" ok "3 of 3 probes made a successful call of the arm's tool (on its copy)"; else rec "probes-$a" FAIL "$n_ok of 3 probes activated (see $P/probes.err)"; fi
    done
    jq -n --arg p "$project" --arg at "$(date -u +%FT%TZ)" --argjson checks "$checks" --argjson failed "$failed" --arg sha "$(git -C "$REPO" rev-parse HEAD)" \
      '{table: "T5", project: $p, run_at: $at, source_commit: $sha, checks: $checks, failed: $failed, status: (if $failed == 0 then "passed" else "failed" end), passed: ($failed == 0)}' > "$report"
    echo "$report: $([ "$failed" = 0 ] && echo PASSED || echo "FAILED ($failed)")"; [ "$failed" = 0 ] ;;

  table)
    python3 - "$RES" "$PROJECTS" <<'PY'
import json, os, statistics, sys
res, projects = sys.argv[1], sys.argv[2].split()
def med(xs): xs = [x for x in xs if x is not None]; return statistics.median(xs) if xs else None
def fmt(x, d=1): return "–" if x is None else (f"{x:.{d}f}" if isinstance(x, float) else str(x))
print("| Project | Steps (days) | Range | Replay (min) | Replay cost (list) | Validation |")
print("|---|---|---|---|---|---|")
for p in projects:
    d = os.path.join(res, p)
    try: rng = json.load(open(f"{d}/range.json")); rep = json.load(open(f"{d}/replay.json")); val = json.load(open(f"{d}/validation.json"))
    except FileNotFoundError: print(f"| {p} | – | – | – | – | not replayed |"); continue
    print(f"| {p} | {rng['steps']} | {rng['start_day']} → {rng['end_day']} ({rng['first_parent_commits']} commits) | {sum(r['seconds'] for r in rep)/60:.0f} | ${sum(r['cost_usd_delta'] for r in rep):.2f} | {'passed' if val['passed'] else 'FAILED'}: {val['docs_checked']} docs, {val['sections_checked']} sections, {val['mismatches']} mismatches |")
print()
for p in projects:
    d = os.path.join(res, p)
    if not os.path.exists(f"{d}/grades.jsonl"): continue
    g = [json.loads(l) for l in open(f"{d}/grades.jsonl")]
    arms = sorted({r["arm"] for r in g}, key=lambda a: (a != "mda", a))
    print(f"**{p}** — per arm (question-level medians over runs; correctness = share of questions whose median run is correct)")
    print()
    print("| Arm | Correct (all) | which | when | added | Tool calls (median) | Source tokens (median) | Wall s (median) | Cost/run (median) | Error rows | git used | arm tool used |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for a in arms:
        rows = [r for r in g if r["arm"] == a]; byq = {}
        for r in rows: byq.setdefault(r["id"], []).append(r)
        def qmed(key, f=lambda r: r[key]): return [med([f(r) for r in rs]) for rs in byq.values()]
        corr = {t: [med([r["correct"] for r in rs]) for q, rs in byq.items() if rs[0]["type"] == t] for t in ("which", "when", "added")}
        allc = qmed("correct")
        print(f"| {a} | {fmt(sum(1 for c in allc if c >= 0.5)/len(allc), 2)} | " + " | ".join(fmt(sum(1 for c in v if c >= 0.5)/len(v), 2) if v else "–" for v in corr.values()) +
              f" | {fmt(med(qmed('tool_calls')))} | {fmt(med(qmed('source_tokens')), 0)} | {fmt(med(qmed('wall_s')))} | ${fmt(med([r['cost_usd'] for r in rows]), 3)} | {sum(1 for r in rows if r['error'])} | {sum(1 for r in rows if r['git_used'])}/{len(rows)} | {sum(1 for r in rows if r.get('arm_tool_used'))}/{len(rows)} |")
    print()
    an = f"{d}/analysis.json"
    if os.path.exists(an):
        A = json.load(open(an))["analysis"] if "analysis" in json.load(open(an)) else json.load(open(an))
        print(f"**{p}** — mda vs each arm (paired bootstrap over the questions, {A.get('draws', 10000)} draws, seed {A.get('seed', 20260922)}; difference = mda − arm)")
        print()
        print("| Pair | Δ correct [95% CI] | Δ tool calls [95% CI] | Δ source tokens [95% CI] | Claim (correct with less effort) |")
        print("|---|---|---|---|---|")
        for pr in A["pairs"]:
            c = pr["correct"]; t = pr["tool_calls"]; s = pr["source_tokens"]
            print(f"| mda vs {pr['comparator']} | {c['diff']:+.3f} [{c['lower']:+.3f}, {c['upper']:+.3f}] | {t['diff']:+.1f} [{t['lower']:+.1f}, {t['upper']:+.1f}] | {s['diff']:+.0f} [{s['lower']:+.0f}, {s['upper']:+.0f}] | {'**yes**' if pr['claim'] else 'no'} |")
        print()
print("Generated by `scripts/eval/t5.sh table` from `evals/results/docsqa/T5/<project>/{range,replay,validation}.json`, `grades.jsonl` and `analysis.json`. Correctness: set F1 ≥ 0.8 for which/added questions, within one day for when questions; a failed run is a row scored 0 with its effort as recorded. Dates are first-parent committer dates (UTC days). The claim is made for a pair only when the lower bound of Δ correct ≥ −0.05 and the upper bounds of both effort differences are < 0 (plan §4 T5).")
PY
    ;;
esac
