#!/usr/bin/env bash
# T5, axis D (execution plan §4 T5, plan docs/plans/2026-09-m8-t5-replay.md): the simulated
# historical replay. A fixed range of a repository's first-parent history is replayed into mda's
# store one step per day under the test-only clock (`MDA_NOW`, ADR-0006), the stored timestamps
# are validated against git (a mismatch voids the project's replay), and forty generated questions
# with git-computed ground truth are answered by every arm on its own copy of the final checkout,
# every arm with `.git` and `git` in Bash (lib.sh ARM_WITH_GIT=1); the companion arm mda-index is
# mda without Bash (the time index alone). Dates throughout are the committer dates of the
# first-parent commits on `main` — the moment a change landed — as UTC calendar days.
#
# Ground truth and validation are derived from the STEP TREES (`git ls-tree` / `git diff` between
# consecutive step commits), never from per-commit events and never from the store: what the replay
# observed at step n is exactly the tree of step n compared with the tree of step n−1 (Codex M8 F3–F5).
#
# Layout under $RUN/t5/<project>/:
#   src/            the replay clone (history-bearing; `--shallow-since` range start − 7 days), the
#                   mda arm's working copy: its store is built by `replay`
#   replay/<n>.json one record per replayed step (binary hash, FROZEN.md hash, counts, cost)
#   git/src qmd/src graphify/{graph.json,src}   the other arms' copies of the final checkout (+ .git)
#   runs/           manifest, rows, traces, grades of the counted answer runs; runs-smoke/ for T5_SMOKE
# Committed under evals/results/docsqa/T5/<project>/: range.json, steps.jsonl, questions.jsonl (inputs,
# hashed by the freeze); replay.json, validation.json, manifest.json, grades.jsonl, analysis*.json (outputs).
#
# Usage:
#   scripts/eval/t5.sh setup <project> [range_days]   # clone, config, the step list → T5/<project>/{range.json,steps.jsonl}
#   scripts/eval/t5.sh questions <project>        # forty seeded questions with git ground truth → T5/<project>/questions.jsonl
#   scripts/eval/t5.sh replay <project>           # under the T5 freeze: every step not yet replayed; then the vectors (final step's clock)
#   scripts/eval/t5.sh validate <project>         # stored dates vs the step trees → T5/<project>/validation.json (bound to the store's hash)
#   scripts/eval/t5.sh arms <project>             # the git/qmd/graphify copies and the qmd index t5-qmd-<project>
#   scripts/eval/t5.sh preflight <project>        # freeze check, env, validation bound to the store, copies, graph hash, probes per arm
#   scripts/eval/t5.sh run <project> [runs=3] [arms]   # counted runs: freeze + preflight enforced; structured answers; resumable
#   scripts/eval/t5.sh status <project>
#   scripts/eval/t5.sh grade <project>            # deterministic; refuses an incomplete run set
#   scripts/eval/t5.sh analysis <project> [--json] # mda eval --replay-analysis (T5_ANALYSIS_ARM=mda-index for the companion)
#   scripts/eval/t5.sh table                      # the page table from the archived analyses, replay and validation records
#   scripts/eval/t5.sh redo <project> <id> <arm> <run>   # move one row and its traces to superseded/ (never deleted) for a disclosed re-measurement
# Env: T5_PROJECTS (prisma tailwind-css), T5_RANGE_DAYS (60; Tailwind is set up with 180), T5_SMOKE=<n> (harness check on the first n
#      questions in runs-smoke/, never a published row), T5_DRY_RUN=1 (a replay without the freeze; its store is never published).
#      The run parameters (model sonnet, 12 turns, 600 s, one retry, one job, arms git mda mda-index qmd graphify) are fixed for counted
#      runs: T5_MODEL/T5_MAX_TURNS/T5_TIMEOUT/T5_RETRIES/T5_JOBS/T5_ARMS are honoured only under T5_SMOKE.
# shellcheck source=lib.sh
. "$(dirname "$0")/lib.sh"
set -euo pipefail
cmd="${1:?setup|questions|replay|validate|arms|preflight|run|status|grade|analysis|table|redo}"; shift
T5="$RUN/t5"; RES="$RESULTS/T5"; PROJECTS="${T5_PROJECTS:-prisma tailwind-css}"; RANGE_DAYS="${T5_RANGE_DAYS:-60}"; N_Q=40
if [ -n "${T5_SMOKE:-}" ]; then
  MODEL="${T5_MODEL:-sonnet}"; ARMS="${T5_ARMS:-git mda mda-index qmd graphify}"; JOBS="${T5_JOBS:-1}"; MAX_TURNS="${T5_MAX_TURNS:-12}"; TIMEOUT="${T5_TIMEOUT:-600}"; RETRIES="${T5_RETRIES:-1}"
else
  for v in T5_MODEL T5_ARMS T5_JOBS T5_MAX_TURNS T5_TIMEOUT T5_RETRIES; do [ -z "${!v:-}" ] || die "$v is set: the run parameters of a counted T5 run are fixed (override only under T5_SMOKE)"; done
  MODEL=sonnet; ARMS="git mda mda-index qmd graphify"; JOBS=1; MAX_TURNS=12; TIMEOUT=600; RETRIES=1
fi
SOURCES="$RUN/docsqa-data/sources.json"
PREAMBLE="The current directory is a git checkout of a documentation site; you may run git. Dates are the UTC calendar days on which changes landed on the main branch. Answer through the structured output only: pages as repository-relative paths, sections as heading paths joined by ' › ', date as YYYY-MM-DD; leave the fields the question does not need empty."
SCHEMA='{"type":"object","properties":{"pages":{"type":"array","items":{"type":"string"}},"sections":{"type":"array","items":{"type":"string"}},"date":{"type":"string"},"evidence":{"type":"string"}},"required":["pages","sections","date","evidence"],"additionalProperties":false}'
GIT_PROBE_Q="Which markdown file of this documentation was changed most recently on the main branch, and on what UTC day? Use the repository's history."
unset_nested_session; unset_provider_keys
export ARM_WITH_GIT=1

src_field() { jq -r --arg id "$1" --arg f "$2" '.sources[] | select(.id == $id) | .[$f] | if type == "array" then join(" ") else . end' "$SOURCES"; }
pdir() { echo "$T5/$1"; }
graphify_build() { case "$1" in prisma) echo "$RUN/graphify/prisma-haiku" ;; *) echo "$RUN/graphify/$1" ;; esac; }   # Prisma: the Haiku graph (its Sonnet build never completed)
graphify_archive() { case "$1" in prisma) echo "$RESULTS/arms/graphs/graphify-haiku-prisma.graph.json.gz" ;; *) echo "$RESULTS/arms/graphs/graphify-$1.graph.json.gz" ;; esac; }
graphify_arm() { case "$1" in prisma) echo graphify-haiku ;; *) echo graphify ;; esac; }
day_shift() { python3 -c 'import sys, datetime; d = datetime.date.fromisoformat(sys.argv[1]); print((d + datetime.timedelta(days=int(sys.argv[2]))).isoformat())' "$1" "$2"; }
store_sha() { sha256 "$1/.markdownattractor/index.sqlite"; }
frozen_sha() { if [ -f "$RES/FROZEN.md" ]; then sha256 "$RES/FROZEN.md"; else echo none; fi; }
# Every committed version of T5/FROZEN.md (newest first): a replay step recorded under an earlier
# version is accepted — a freeze rewritten after the replay for a harness-only change does not
# make the replay void — and the preflight says which version it was.
frozen_versions() { local h; for h in $(git -C "$REPO" log --format=%H -- evals/results/docsqa/T5/FROZEN.md); do git -C "$REPO" show "$h:evals/results/docsqa/T5/FROZEN.md" | shasum -a 256 | cut -c1-64; done; frozen_sha; }
frozen_known() { local v; v="$(frozen_versions)"; grep -qxF "$1" <<<"$v"; }   # no `grep -q` on a live pipe under pipefail (SIGPIPE upstream)
freeze_ok() { "$REPO/scripts/eval/freeze.sh" --protocol final --table T5 --check >/dev/null 2>&1; }
# the committed step list is the one every phase reads (the freeze hashes it)
steps_file() { local f="$RES/$1/steps.jsonl"; [ -f "$f" ] || die "no committed step list $f (run setup)"; echo "$f"; }

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
    # [start_day, end_day]; one step per UTC day whose TREE differs from the previous step's tree
    # in a markdown file under a content root, at the day's last commit; the pinned revision is
    # the final step. Each step records the tree diff against the previous step (every markdown
    # file, any path) — what the replay will observe.
    python3 - "$SRC" "$rev" "$start_day" "$end_day" "$roots" "$RES/$project/steps.jsonl" "$RES/$project/range.json" "$repo" "$since" <<'PY' || die "step list failed"
import datetime, json, subprocess, sys
src, rev, start, end, roots, out, rng, repo, since = sys.argv[1:10]
roots = roots.split()
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def is_md(p): return p.endswith((".md", ".mdx", ".markdown"))
def under_root(p): return any(p == r or p.startswith(r.rstrip("/") + "/") for r in roots)
commits = []  # oldest first: (sha, committer datetime)
for line in git("rev-list", "--first-parent", "--reverse", "--format=%H %cI", rev).splitlines():
    if line.startswith("commit "): continue
    sha, ci = line.split(" ", 1)
    day = datetime.datetime.fromisoformat(ci).astimezone(datetime.timezone.utc)
    if start <= day.date().isoformat() <= end: commits.append((sha, day))
if not commits: sys.exit("no first-parent commits in range")
first = commits[0][0]
if subprocess.run(["git", "-C", src, "cat-file", "-e", first + "^"], capture_output=True).returncode != 0: sys.exit(f"parent of the first in-range commit {first[:10]} is outside the clone; widen --shallow-since")
last_of_day, n_of_day = {}, {}
for sha, day in commits:
    d = day.date().isoformat(); last_of_day[d] = (sha, day); n_of_day[d] = n_of_day.get(d, 0) + 1
def tree_diff(a, b):
    rows = []
    for row in git("diff", "--name-status", "-M", a, b).splitlines():
        parts = row.split("\t"); st = parts[0][0]; path = parts[-1]; old = parts[1] if st == "R" and len(parts) == 3 else None
        if is_md(path) or (old and is_md(old)): rows.append({"status": st, "path": path, **({"from": old} if old else {})})
    return rows
steps, prev = [], None
for d in sorted(last_of_day):
    sha, when = last_of_day[d]
    base = prev if prev else sha + "^"
    changes = tree_diff(base, sha)
    if sha != rev and not any(under_root(c["path"]) or under_root(c.get("from", "")) for c in changes):
        continue   # a day without a markdown change under the roots is not a step (its changes fold into the next step's diff)
    steps.append({"step": len(steps) + 1, "sha": sha, "date": when.astimezone(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "day": d,
                  "commits_that_day": n_of_day[d], "changes": changes, "diff_base": base})
    prev = sha
if steps[-1]["sha"] != rev: sys.exit(f"the last step {steps[-1]['sha'][:10]} is not the pinned revision")
with open(out, "w") as fh:
    for s in steps: fh.write(json.dumps(s) + "\n")
json.dump({"repository": repo, "end_sha": rev, "start_day": start, "end_day": end, "range_days": (datetime.date.fromisoformat(end) - datetime.date.fromisoformat(start)).days,
           "shallow_since": since, "content_roots": roots, "first_parent_commits": len(commits), "steps": len(steps),
           "date_semantics": "committer date of the day's last first-parent commit, UTC calendar day; a step's changes are the tree diff against the previous step (the first step against its parent commit)"}, open(rng, "w"), indent=1)
print(f"{len(steps)} steps over {len(commits)} first-parent commits, {start}..{end}; markdown changes under the roots: {sum(1 for s in steps for c in s['changes'] if under_root(c['path']))}")
PY
    cp "$RES/$project/steps.jsonl" "$P/steps.jsonl"
    echo "steps → $RES/$project/steps.jsonl · range → $RES/$project/range.json" ;;

  questions)
    # Candidates are enumerated from the step trees (never the store) and the historical page
    # universe — every markdown file under a content root present at any step, deleted or
    # renamed ones included — kept when the ground truth is answerable (1–12 members; a date
    # inside the range), then drawn in sha256(seed ‖ candidate key) order, 14 which / 13 when /
    # 13 added with a round-robin over kinds; a short type is topped up and the counts recorded.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; roots="$(src_field "$project" content_roots)"; steps="$(steps_file "$project")"
    python3 - "$SRC" "$steps" "$roots" "$SEED" "$N_Q" "$RES/$project/questions.jsonl" "$MDA" <<'PY' || die "question generation failed"
import collections, datetime, hashlib, json, os, subprocess, sys, tempfile
src, steps_f, roots, seed, n_q, out, mda = sys.argv[1:8]
roots = roots.split(); steps = [json.loads(l) for l in open(steps_f)]
days = [s["day"] for s in steps]; end = days[-1]
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def is_md(p): return p.endswith((".md", ".mdx", ".markdown"))
def under_root(p): return any(p == r or p.startswith(r.rstrip("/") + "/") for r in roots)
def tree(sha): return {p for p in git("ls-tree", "-r", "--name-only", sha).splitlines() if is_md(p) and under_root(p)}
def parse(sha, path):
    blob = subprocess.run(["git", "-C", src, "show", f"{sha}:{path}"], capture_output=True, text=True)
    if blob.returncode != 0: return None
    with tempfile.NamedTemporaryFile("w", suffix=os.path.splitext(path)[1], delete=False, encoding="utf-8") as fh: fh.write(blob.stdout); tmp = fh.name
    try:
        r = subprocess.run([mda, "--json", "parse", tmp], capture_output=True, text=True)
        if r.returncode != 0: return None
        return {" › ".join(s["heading_path"]): s["hash"] for s in json.loads(r.stdout)["sections"] if s["heading_path"]}
    finally: os.unlink(tmp)
# the page calendar from consecutive step trees (the first step: everything present is pre-existing, nothing is "changed")
trees = {s["step"]: tree(s["sha"]) for s in steps}
changed_on = collections.defaultdict(set); added_on = {}; universe = set()
for s in steps:
    universe |= trees[s["step"]]
    if s["step"] == 1: continue
    prev = trees[s["step"] - 1]
    for c in s["changes"]:
        p, st, frm = c["path"], c["status"], c.get("from")
        if st == "D" or not under_root(p): continue
        changed_on[p].add(s["day"])
        if p not in prev: added_on.setdefault(p, s["day"])   # added or renamed-to (a re-added page counts as added again only the first time)
def dir_of(p, depth):
    parts = p.split("/")
    for r in roots:
        rp = r.rstrip("/").split("/")
        if parts[:len(rp)] == rp: return "/".join(parts[:len(rp) + depth]) if len(parts) > len(rp) + depth else None
    return None
cands = []
# (a) which pages changed between D1 and D2 (inclusive), under a directory, windows of up to 14 days between step days
for i, d1 in enumerate(days[1:], 1):
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
                    cands.append({"type": "which", "kind": "pages", "key": f"which-pages|{d}|{d1}|{d2}", "q": f"Which documentation pages under `{d}/` changed between {d1} and {d2} (inclusive)? Count a page that was added, modified or renamed to in that window, even if it was deleted later; give the repository-relative path it had.",
                                  "truth": {"pages": sorted(ps)}, "window": [d1, d2], "dir": d})
# (a') which sections of a page changed at a step (tree of step n vs tree of step n−1); (b') when a section was added
sec_added = {}
for s in steps[1:]:
    prev_sha = steps[s["step"] - 2]["sha"]
    for c in s["changes"]:
        p = c["path"]
        if c["status"] == "D" or not under_root(p): continue
        now = parse(s["sha"], p)
        if now is None: continue
        old_path = c.get("from") or p
        before = parse(prev_sha, old_path) if old_path in trees[s["step"] - 1] else {}
        if before is None: before = {}
        changed = sorted(h for h, hh in now.items() if before.get(h) != hh)
        if before and 1 <= len(changed) <= 12:
            cands.append({"type": "which", "kind": "sections", "key": f"which-sections|{p}|{s['day']}", "q": f"Which sections of `{p}` changed on {s['day']} (heading paths)?", "truth": {"sections": changed}, "window": [s["day"], s["day"]], "page": p})
        if before:
            for h in now:
                if h not in before: sec_added.setdefault((p, h), s["day"])
# (b) when was a page last changed (pages changed in range and still present at the end)
final = trees[steps[-1]["step"]]
for p, ds in changed_on.items():
    if p in final: cands.append({"type": "when", "kind": "page", "key": f"when-page|{p}", "q": f"On what day was `{p}` last changed?", "truth": {"date": max(ds)}, "page": p})
for (p, h), d in sec_added.items():
    if p in final: cands.append({"type": "when", "kind": "section", "key": f"when-section|{p}|{h}", "q": f"On what day was the section `{h}` added to `{p}`?", "truth": {"date": d}, "page": p, "section": h})
# (c) which pages were added between D1 and D2 (calendar months clipped to the observed range, which starts at the second step's day), under a directory
months = sorted({d[:7] for d in days[1:]})
for m in months:
    d1 = max(days[1], m + "-01"); last = (datetime.date.fromisoformat(m + "-28") + datetime.timedelta(days=4)).replace(day=1) - datetime.timedelta(days=1); d2 = min(end, last.isoformat())
    for depth in (0, 1):
        by_dir = collections.defaultdict(set)
        for p, d in added_on.items():
            if d1 <= d <= d2:
                dd = dir_of(p, depth)
                if dd: by_dir[dd].add(p)
        for dd, ps in by_dir.items():
            if 1 <= len(ps) <= 12:
                cands.append({"type": "added", "kind": "pages", "key": f"added|{dd}|{d1}|{d2}", "q": f"Which documentation pages under `{dd}/` were added between {d1} and {d2} (inclusive)? Count renamed-to paths as added; give the repository-relative path each page had when it appeared.", "truth": {"pages": sorted(ps)}, "window": [d1, d2], "dir": dd})
key = lambda k: hashlib.sha256((seed + "\0" + k).encode()).hexdigest()
for c in cands: c["order"] = key(c["key"])
cands.sort(key=lambda c: c["order"])
want = {"which": 14, "when": 13, "added": 13}; n = int(n_q)
pool = {t: [c for c in cands if c["type"] == t] for t in want}
chosen = []
for t, k in want.items():
    kinds = sorted({c["kind"] for c in pool[t]}); picked = []
    while len(picked) < k and any(c not in picked for c in pool[t]):
        for kind in kinds:
            nxt = next((c for c in pool[t] if c["kind"] == kind and c not in picked), None)
            if nxt is not None and len(picked) < k: picked.append(nxt)
    chosen += picked
if len(chosen) < n:
    rest = [c for c in cands if c not in chosen]; chosen += rest[:n - len(chosen)]
chosen.sort(key=lambda c: c["order"])
with open(out, "w", encoding="utf-8") as fh:
    for i, c in enumerate(chosen, 1):
        c = dict(c); c["id"] = f"t5-{i:02d}-{c['type']}"; c["scoring"] = "set F1 ≥ 0.8" if c["type"] != "when" else "within one day"
        c["evidence"] = {"steps": [s["sha"][:12] for s in steps if (c.get("window") and c["window"][0] <= s["day"] <= c["window"][1]) or (c.get("truth", {}).get("date") == s["day"])][:20], "date_semantics": "committer date of the day's last first-parent commit, UTC day; changes = tree diff between consecutive steps"}
        fh.write(json.dumps(c, ensure_ascii=False) + "\n")
counts = collections.Counter(c["type"] for c in chosen)
print(f"universe {len(universe)} pages · candidates: which {len(pool['which'])} (pages {sum(1 for c in pool['which'] if c['kind']=='pages')}, sections {sum(1 for c in pool['which'] if c['kind']=='sections')}) · when {len(pool['when'])} · added {len(pool['added'])}; drawn {len(chosen)}: {dict(counts)} → {out}")
PY
    echo "questions → $RES/$project/questions.jsonl ($(sha256 "$RES/$project/questions.jsonl" | cut -c1-12)…)" ;;

  replay)
    # Under the T5 freeze only (the counted replay); every step record carries the binary's hash
    # and the FROZEN.md hash, and a replay refuses to continue records made under another binary
    # or another freeze. A step is complete only when nothing is pending and nothing failed.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; mkdir -p "$P/replay"; steps="$(steps_file "$project")"
    [ -n "${T5_DRY_RUN:-}" ] || freeze_ok || die "T5/FROZEN.md does not check clean: a counted replay runs under the freeze (T5_DRY_RUN=1 for harness development; its store is never published)"
    cmp -s "$steps" "$P/steps.jsonl" 2>/dev/null || cp "$steps" "$P/steps.jsonl"
    ! "$MDA" --json status --root "$SRC" 2>/dev/null | jq -e '.daemon != null' >/dev/null || die "a daemon is running on $SRC"
    bin_sha="$(sha256 "$MDA")"; fz="$(frozen_sha)"; [ -z "${T5_DRY_RUN:-}" ] || fz="dry-run"
    for f in "$P"/replay/[0-9]*.json; do
      [ -f "$f" ] || continue
      [ "$(jq -r '.mda_sha256' "$f")" = "$bin_sha" ] || die "$f was made by another binary: a replay runs one binary end to end; start over in a fresh directory"
      frozen_known "$(jq -r '.frozen_sha256' "$f")" || die "$f was made under an unknown freeze (or a dry run): start over in a fresh directory"
    done
    while IFS= read -r s; do
      n="$(jq -r .step <<<"$s")"; sha="$(jq -r .sha <<<"$s")"; at="$(jq -r .date <<<"$s")"
      [ -f "$P/replay/$n.json" ] && continue
      git -C "$SRC" checkout -q --detach "$sha"
      python3 - "$SRC" "$P/steps.jsonl" "$n" <<'PY' || die "mtimes failed"
import datetime, json, os, subprocess, sys
src, steps_f, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
steps = [json.loads(l) for l in open(steps_f)]
step = steps[n - 1]; ts = datetime.datetime.fromisoformat(step["date"].replace("Z", "+00:00")).timestamp()
if n == 1:   # history before the range is flattened to the first step
    files = [p for p in subprocess.run(["git", "-C", src, "ls-files"], check=True, capture_output=True, text=True).stdout.splitlines() if p.endswith((".md", ".mdx", ".markdown"))]
    # A reused working copy carries birth times from an earlier replay (macOS moves a file's
    # birth time down to an earlier mtime and never back up), which `created_at` would inherit:
    # every file's birth time must be at or after the clone's own creation.
    clone_born = os.stat(os.path.join(src, ".git")).st_birthtime
    stale = [p for p in files if os.path.isfile(os.path.join(src, p)) and os.stat(os.path.join(src, p)).st_birthtime < clone_born - 1]
    if stale: sys.exit(f"{len(stale)} files carry birth times older than the clone (an earlier replay touched this copy): re-clone before a replay (e.g. {stale[0]})")
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
      for ((k = 0; k < 20; k++)); do
        pend="$("$MDA" --json status --root "$SRC" | jq -r '.counts.pending')"; [ "$pend" = 0 ] && break
        MDA_NOW="$at" "$MDA" index --root "$SRC" >> "$P/replay/$n.log" 2>&1 || true
      done
      # Sections whose summarisation failed are retried twice (`--retry-failed`); a card that still
      # fails is product behaviour (the section stays raw-searchable, its dates are unaffected) and
      # is recorded in the step record, not a reason to stop the replay. Pending work is.
      for ((k = 0; k < 2; k++)); do
        [ "$("$MDA" --json status --root "$SRC" | jq -r '.counts.failed')" = 0 ] && break
        MDA_NOW="$at" "$MDA" index --retry-failed --root "$SRC" >> "$P/replay/$n.log" 2>&1 || true
      done
      t1=$(date +%s); after="$("$MDA" --json status --root "$SRC" | jq -c '.counts')"
      [ "$(jq -r .pending <<<"$after")" = 0 ] || die "step $n did not complete: $(jq -c '{pending, failed}' <<<"$after") (no step record written; fix and rerun)"
      jq -n --argjson n "$n" --arg sha "$sha" --arg at "$at" --argjson before "$before" --argjson after "$after" --argjson secs "$((t1 - t0))" --arg store "$(store_sha "$SRC")" --arg done_at "$(date -u +%FT%TZ)" --arg bin "$bin_sha" --arg fz "$fz" \
        '{step: $n, sha: $sha, mda_now: $at, seconds: $secs, counts_before: $before, counts_after: $after, cost_usd_delta: (($after.total_cost_usd // 0) - ($before.total_cost_usd // 0)), store_sha256: $store, mda_sha256: $bin, frozen_sha256: $fz, replayed_at: $done_at}' > "$P/replay/$n.json.tmp" && mv "$P/replay/$n.json.tmp" "$P/replay/$n.json"
      echo "step $n $sha ($at): $((t1 - t0)) s · docs $(jq -r .docs <<<"$after") · sections $(jq -r .sections <<<"$after") · summarized $(jq -r .summarized <<<"$after") · failed $(jq -r .failed <<<"$after") · +\$$(jq -r .cost_usd_delta "$P/replay/$n.json")"
    done < "$P/steps.jsonl"
    if [ ! -f "$P/replay/embeddings.json" ]; then
      last_at="$(tail -1 "$P/steps.jsonl" | jq -r .date)"
      t0=$(date +%s); MDA_NOW="$last_at" "$MDA" rebuild --embeddings --root "$SRC" > "$P/replay/embeddings.log" 2>&1 || die "rebuild --embeddings failed"
      jq -n --argjson secs "$(( $(date +%s) - t0 ))" --arg store "$(store_sha "$SRC")" --arg at "$last_at" --argjson emb "$("$MDA" --json status --root "$SRC" | jq -c '.embeddings')" '{seconds: $secs, mda_now: $at, store_sha256: $store, embeddings: $emb}' > "$P/replay/embeddings.json"
      echo "vectors: $(( $(date +%s) - t0 )) s (clock at the final step)"
    fi
    mkdir -p "$RES/$project"; jq -c . "$P"/replay/[0-9]*.json | jq -s 'sort_by(.step)' > "$RES/$project/replay.json"
    echo "replay complete: $(jq length "$RES/$project/replay.json") steps · total $(jq '[.[].seconds] | add' "$RES/$project/replay.json") s · \$$(jq '[.[].cost_usd_delta] | add' "$RES/$project/replay.json") · store sha256 $(store_sha "$SRC" | cut -c1-12)… → $RES/$project/replay.json" ;;

  validate)
    # Both directions, from the step trees: every markdown file of the final tree that the
    # walker indexes must be a live document whose sections equal `mda parse` of the file and
    # whose dates match; every live document must be such a file; every file present at an
    # earlier step and absent at the end must be a tombstone dated at the deleting step; nothing
    # else may be tombstoned. The report is bound to the store's hash and the step list's hash.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; mkdir -p "$RES/$project"; steps="$(steps_file "$project")"
    ignores="$(grep -A20 '^ignore = \[' "$SRC/.markdownattractor/config.toml" | sed -n '2,/^\]/p' | grep -oE '"[^"]+"' | tr -d '"' | tr '\n' ' ')"
    python3 - "$SRC/.markdownattractor/index.sqlite" "$steps" "$SRC" "$RES/$project/validation.json" "$MDA" "$ignores" "$(store_sha "$SRC")" "$(sha256 "$steps")" <<'PY' || die "validation failed to run"
import datetime, fnmatch, json, os, sqlite3, subprocess, sys
db, steps_f, src, out, mda, ignores, store_sha, steps_sha = sys.argv[1:9]
ignores = ignores.split(); steps = [json.loads(l) for l in open(steps_f)]
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def is_md(p): return p.endswith((".md", ".mdx", ".markdown"))
# What the walker indexes (crates/mda-core/src/walk.rs): no hidden entry (a component starting
# with "."), nothing matched by .gitignore / .ignore / .git/info/exclude (asked of git itself:
# `check-ignore --no-index`, tracked or not), nothing matched by config.ignore; the count of
# files excluded by each rule is reported.
excluded = {"hidden": 0, "gitignore": 0, "config_ignore": 0}
def git_ignored(paths):
    if not paths: return set()
    r = subprocess.run(["git", "-C", src, "check-ignore", "--no-index", "--stdin"], input="\n".join(paths) + "\n", capture_output=True, text=True)
    return set(r.stdout.splitlines())
def hidden(p): return any(c.startswith(".") for c in p.split("/"))
def config_ignored(p):
    for g in ignores:
        if g.endswith("/"):
            if p.startswith(g) or ("/" + g) in ("/" + p): return True
        elif fnmatch.fnmatch(p, g) or fnmatch.fnmatch(os.path.basename(p), g): return True
    return False
_gi_cache = {}
def ignored(p):
    if hidden(p): return True
    if p in _gi_cache and _gi_cache[p]: return True
    return config_ignored(p)
def prime_gitignore(paths):
    for p in git_ignored(sorted(paths)): _gi_cache[p] = True
def norm(t):
    if t is None: return None
    try: return datetime.datetime.fromisoformat(t.replace("Z", "+00:00")).astimezone(datetime.timezone.utc).replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")
    except ValueError: return t
first = steps[0]["date"]
# regular files only (mode 100644/100755): the walker skips symlinks (walk.rs: links are skipped)
def md_tree(sha):
    out = set()
    for row in git("ls-tree", "-r", sha).splitlines():
        meta, path = row.split("\t", 1); mode = meta.split(" ")[0]
        if is_md(path) and mode != "120000": out.add(path)
    return out
trees = {s["step"]: md_tree(s["sha"]) for s in steps}
first_seen, deleted_at, changes = {}, {}, {}
for s in steps:
    t = trees[s["step"]]
    for p in t: first_seen.setdefault(p, s["date"])
    if s["step"] == 1: continue
    prev = trees[s["step"] - 1]
    touched = {c["path"] for c in s["changes"] if c["status"] != "D"}
    for p in t:
        if p not in prev: changes.setdefault(p, []).append(s["date"]); deleted_at.pop(p, None)
        elif p in touched: changes.setdefault(p, []).append(s["date"])
    for p in prev - t: deleted_at[p] = s["date"]
final = trees[steps[-1]["step"]]
prime_gitignore(final | set(deleted_at))
for p in final | set(deleted_at):
    if hidden(p): excluded["hidden"] += 1
    elif _gi_cache.get(p): excluded["gitignore"] += 1
    elif config_ignored(p): excluded["config_ignore"] += 1
expected_live = {p for p in final if not ignored(p)}
expected_tomb = {p: d for p, d in deleted_at.items() if p not in final and not ignored(p)}
con = sqlite3.connect(db)
mism, n_docs, n_secs, n_tomb, created_sources = [], 0, 0, 0, {}
live_rows = {rel: (doc_id, created, src_, updated, seen) for doc_id, rel, created, src_, updated, seen in con.execute("SELECT doc_id, rel_path, created_at, created_at_source, updated_at, first_seen_at FROM docs WHERE deleted_at IS NULL")}
tomb_rows = {rel: d for rel, d in con.execute("SELECT rel_path, deleted_at FROM docs WHERE deleted_at IS NOT NULL")}
for p in sorted(expected_live - set(live_rows)): mism.append({"path": p, "kind": "missing_document"})
for p in sorted(set(live_rows) - expected_live): mism.append({"path": p, "kind": "unexpected_document"})
for p in sorted(set(expected_tomb) - set(tomb_rows)): mism.append({"path": p, "kind": "missing_tombstone", "expected": expected_tomb[p]})
for p in sorted(set(tomb_rows) - set(expected_tomb)): mism.append({"path": p, "kind": "unexpected_tombstone", "stored": tomb_rows[p]})
for p, d in tomb_rows.items():
    if p in expected_tomb:
        n_tomb += 1
        if norm(d) != expected_tomb[p]: mism.append({"path": p, "kind": "deleted_at", "stored": d, "expected": expected_tomb[p]})
for rel in sorted(expected_live & set(live_rows)):
    doc_id, created, src_, updated, seen = live_rows[rel]; n_docs += 1
    exp_updated = changes[rel][-1] if rel in changes else first
    if norm(updated) != exp_updated: mism.append({"path": rel, "kind": "updated_at", "stored": updated, "expected": exp_updated})
    exp_seen = first_seen[rel]
    if norm(seen) != exp_seen: mism.append({"path": rel, "kind": "first_seen_at", "stored": seen, "expected": exp_seen})
    created_sources[src_] = created_sources.get(src_, 0) + 1
    if norm(created) != exp_seen: mism.append({"path": rel, "kind": "created_at", "stored": f"{created} ({src_})", "expected": exp_seen})
    allowed = set(changes.get(rel, [])) | {first}
    stored_secs = [(hp, sup) for hp, sup in con.execute("SELECT heading_path, updated_at FROM sections WHERE doc_id = ? ORDER BY idx", (doc_id,))]
    n_secs += len(stored_secs)
    for hp, sup in stored_secs:
        if norm(sup) not in allowed: mism.append({"path": rel, "kind": "section_updated_at", "section": hp, "stored": sup, "expected_one_of": sorted(allowed)[-3:]})
    r = subprocess.run([mda, "--json", "parse", os.path.join(src, rel)], capture_output=True, text=True)
    if r.returncode != 0: mism.append({"path": rel, "kind": "parse_failed"}); continue
    parsed = sorted(json.dumps(s["heading_path"], separators=(",", ":")) for s in json.loads(r.stdout)["sections"])
    stored = sorted(json.dumps(json.loads(hp), separators=(",", ":")) for hp, _ in stored_secs)
    if parsed != stored: mism.append({"path": rel, "kind": "sections_differ", "stored": len(stored_secs), "parsed": len(parsed)})
rep = {"project": os.path.basename(os.path.dirname(src)), "steps": len(steps), "expected_documents": len(expected_live), "docs_checked": n_docs, "sections_checked": n_secs, "expected_tombstones": len(expected_tomb), "tombstones_checked": n_tomb,
       "ignored_markdown_in_final_tree": sorted(p for p in final if ignored(p))[:20], "excluded_by_rule": excluded, "mismatches": len(mism), "mismatch_kinds": {k: sum(1 for m in mism if m["kind"] == k) for k in sorted({m["kind"] for m in mism})},
       "mismatch_sample": mism[:50], "created_at_sources": created_sources, "store_sha256": store_sha, "steps_sha256": steps_sha, "validated_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
       "passed": len(mism) == 0 and n_docs > 0 and n_docs == len(expected_live),
       "semantics": "expectations from the step trees: a live document for every markdown file of the final tree the walker indexes (symlinks, hidden entries, .gitignore/.ignore matches and config.ignore patterns excluded, as walk.rs does), its sections equal to `mda parse` of the file; updated_at = the date of the last step whose tree changed the file, else the first step; first_seen_at and created_at (source birthtime or first_seen — macOS moves the birth time down to an earlier mtime) = the first step the file is present at; section updated_at ∈ {first step} ∪ the file's change steps; a tombstone dated at the first step the file is absent after being present, and no other tombstones"}
json.dump(rep, open(out, "w"), indent=1)
print(f"{'PASS' if rep['passed'] else 'FAIL'}: {n_docs}/{len(expected_live)} docs, {n_secs} sections, {n_tomb}/{len(expected_tomb)} tombstones checked; {len(mism)} mismatches {rep['mismatch_kinds']}")
PY
    ;;

  arms)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; SRC="$P/src"; rev="$(src_field "$project" revision)"
    [ "$(git -C "$SRC" rev-parse HEAD)" = "$rev" ] || die "the replay clone is not at the pinned revision (replay incomplete?)"
    # Equal evidence ends at the pinned revision: the clone's `main` is the fetched tip (weeks past
    # the range) and `git log main` would show history the store never saw. Every copy gets `main`
    # at the pinned revision as its checked-out branch and no remote-tracking refs.
    pin_refs() { git -C "$1" update-ref refs/heads/main "$rev"; git -C "$1" symbolic-ref HEAD refs/heads/main; git -C "$1" update-ref -d refs/remotes/origin/main 2>/dev/null || true; git -C "$1" update-ref -d refs/remotes/origin/HEAD 2>/dev/null || true; git -C "$1" remote remove origin 2>/dev/null || true; }
    pin_refs "$SRC"
    for a in git qmd; do
      if [ ! -d "$P/$a/src" ]; then mkdir -p "$P/$a/src"; rsync -a --exclude .markdownattractor "$SRC/" "$P/$a/src/"; echo "$a copy: $P/$a/src"; fi
    done
    G="$(graphify_build "$project")"; [ -f "$G/graph.json" ] || die "no graphify build at $G"
    if [ ! -d "$P/graphify/src" ]; then
      mkdir -p "$P/graphify"; rsync -a "$G/" "$P/graphify/"; rsync -a "$SRC/.git/" "$P/graphify/src/.git/"
      (cd "$P/graphify/src" && git reset -q --hard "$rev")   # the build's tracked files were unmodified; its .claude/, CLAUDE.md, graphify-out/ stay untracked beside the full tree
      echo "graphify copy: $P/graphify (history attached)"
    fi
    for d in "$P/git/src" "$P/qmd/src" "$P/graphify/src"; do pin_refs "$d"; done
    for d in "$SRC" "$P/git/src" "$P/qmd/src" "$P/graphify/src"; do
      [ "$(git -C "$d" for-each-ref --format='%(refname) %(objectname)' | tr '\n' ' ')" = "refs/heads/main $rev " ] || die "$d: refs are not exactly main at $rev: $(git -C "$d" for-each-ref --format='%(refname)' | tr '\n' ' ')"
    done
    [ "$(sha256 "$P/graphify/graph.json")" = "$(gunzip -c "$(graphify_archive "$project")" | shasum -a 256 | cut -c1-64)" ] || die "the served graph $P/graphify/graph.json differs from the archived $(graphify_archive "$project")"
    qi="t5-qmd-$project"
    if [ ! -f "$HOME/.config/qmd/$qi.yml" ]; then
      t0=$(date +%s); qmd --index "$qi" collection add "$P/qmd/src" --name "$qi" --mask '**/*.{md,mdx,markdown}' >/dev/null && qmd --index "$qi" update >/dev/null && qmd --index "$qi" embed >/dev/null
      echo "qmd index $qi built in $(( $(date +%s) - t0 )) s"
    fi
    echo "arms ready: git $P/git/src · mda/mda-index $SRC · qmd $P/qmd/src ($qi, fingerprint $(qmd_fingerprint "$qi" | cut -c1-12)…) · graphify $P/graphify (graph = archived)" ;;

  status)
    project="${1:?project}"; P="$(pdir "$project")"
    echo "steps: $(grep -c . "$RES/$project/steps.jsonl" 2>/dev/null || echo 0) · replayed: $(find "$P/replay" -maxdepth 1 -name '[0-9]*.json' 2>/dev/null | wc -l | tr -d ' ') · validation: $(jq -r '.passed // "n/a"' "$RES/$project/validation.json" 2>/dev/null || echo n/a) · questions: $(grep -c . "$RES/$project/questions.jsonl" 2>/dev/null || echo 0) · rows: $(ls "$P/runs/rows" 2>/dev/null | wc -l | tr -d ' ')" ;;

  *) ;;
esac
[ "$cmd" = run ] || [ "$cmd" = grade ] || [ "$cmd" = analysis ] || [ "$cmd" = table ] || [ "$cmd" = preflight ] || [ "$cmd" = redo ] || exit 0

# ---------------------------------------------------------------- the answer phase (T2's machinery, structured answers)
assemble() { local out="$1"; : > "$out/runs.jsonl"; local f; for f in "$out"/rows/*.json; do [ -f "$f" ] || continue; jq -c . "$f" >> "$out/runs.jsonl"; done; }
run_once() { # corpus trace question args... -> exit code (124 = timeout); stderr to <trace>.err
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
# One attempt's numbers from its trace (T2's accounting: rule 0.7 source tokens; tool calls with
# their outcome; the structured answer when the attempt produced one). Never fails: a truncated
# or empty trace yields a record with nulls and `parsed: false`.
attempt_of() { # trace want-regex -> json
  jq -c -s --arg want "$2" '
    (map(select(.type == "result")) | last) as $res |
    ([.[] | select(.type == "stream_event" and .event.type == "message_delta") | .event.usage
      | {inp: ((.input_tokens // 0) + (.cache_read_input_tokens // 0) + (.cache_creation_input_tokens // 0)), out: (.output_tokens // 0)}]) as $turns |
    ([range(1; $turns | length) as $t | ($turns[$t].inp - $turns[$t - 1].inp - $turns[$t - 1].out)]) as $deltas |
    (map(select(.type == "assistant")) | map(.message.content[]? | select(.type == "tool_use") | {id, name, input})) as $uses |
    (map(select(.type == "user")) | map(.message.content[]? | select(type == "object" and .type == "tool_result") | {id: .tool_use_id, error: (.is_error // false)})) as $results |
    ($uses | map(. as $u | {name: $u.name, ok: (($results | map(select(.id == $u.id and (.error | not))) | length) > 0)})) as $calls |
    {parsed: true, structured: ($res.structured_output // null), answer_text: (($res.result // "") | tostring | .[:600]),
     turns: ($res.num_turns // null), turns_with_usage: ($turns | length),
     input_tokens: (if $res == null then null else (($res.usage.input_tokens // 0) + ($res.usage.cache_read_input_tokens // 0) + ($res.usage.cache_creation_input_tokens // 0)) end),
     output_tokens: ($res.usage.output_tokens // null),
     source_tokens: (if ($turns | length) < 2 then 0 else ($deltas | map(if . < 0 then 0 else . end) | add) end),
     source_tokens_signed: (if ($turns | length) < 2 then 0 else ($deltas | add) end),
     tool_calls: ($calls | length), tools: ($calls | map(.name)), failed_calls: ($calls | map(select(.ok | not)) | length),
     git_commands: ($uses | map(select(.name == "Bash") | .input.command // "" | .[:80])),
     arm_tool_used: (($calls | map(select(.ok and (.name | test($want)))) | length) > 0),
     cost_usd: ($res.total_cost_usd // null), models: (map(select(.type == "assistant")) | map(.message.model // empty) | unique),
     result_ok: (($res != null) and (($res.is_error // false) | not) and ($res.structured_output != null))}' "$1" 2>/dev/null \
  || echo '{"parsed": false, "structured": null, "answer_text": "", "turns": null, "turns_with_usage": 0, "input_tokens": null, "output_tokens": null, "source_tokens": 0, "source_tokens_signed": 0, "tool_calls": 0, "tools": [], "failed_calls": 0, "git_commands": [], "arm_tool_used": false, "cost_usd": null, "models": [], "result_ok": false}'
}
launch_env() { # project arm
  local P; P="$(pdir "$1")"
  case "$2" in
    mda|mda-index) ARM_CORPUS_OVERRIDE="$P/src" ;; git) ARM_CORPUS_OVERRIDE="$P/git/src" ;;
    qmd) ARM_CORPUS_OVERRIDE="$P/qmd/src"; export ARM_QMD_INDEX="t5-qmd-$1" ;;
    graphify) ARM_CORPUS_OVERRIDE="$P/graphify/src"; export ARM_GRAPHIFY_DIR="$P/graphify" ;;
  esac; export ARM_CORPUS_OVERRIDE
}
clear_env() { unset ARM_CORPUS_OVERRIDE ARM_QMD_INDEX ARM_GRAPHIFY_DIR; }
arm_name() { case "$2" in graphify) graphify_arm "$1" ;; *) echo "$2" ;; esac; }
runs_dir() { echo "$(pdir "$1")/runs${T5_SMOKE:+-smoke}"; }

case "$cmd" in
  run)
    project="${1:?project}"; runs="${2:-3}"; arms="${3:-$ARMS}"; arms="${arms//,/ }"; ident "$project"; P="$(pdir "$project")"; out="$(runs_dir "$project")"
    [ -f "$RES/$project/questions.jsonl" ] || die "no committed questions for $project"; questions="$RES/$project/questions.jsonl"
    val="$RES/$project/validation.json"; [ "$(jq -r .passed "$val" 2>/dev/null)" = true ] || die "the replay of $project is not validated (or voided): no answer runs"
    [ "$(jq -r .store_sha256 "$val")" = "$(store_sha "$P/src")" ] || die "the mda store changed since validation ($val): validate again"
    [ "$(jq -r .steps_sha256 "$val")" = "$(sha256 "$RES/$project/steps.jsonl")" ] || die "validation.json was made against another step list"
    if [ -n "${T5_SMOKE:-}" ]; then   # harness check on the first n questions, in its own directory; never a published row
      mkdir -p "$out"; head -n "$T5_SMOKE" "$questions" > "$out/questions.jsonl"; questions="$out/questions.jsonl"; echo "SMOKE: $T5_SMOKE question(s) → $out"
    else
      freeze_ok || die "T5/FROZEN.md does not check clean: counted runs need the freeze"
      pf="$RESULTS/preflight/T5-$project.json"; [ "$(jq -r .passed "$pf" 2>/dev/null)" = true ] || die "no passing preflight at $pf"
      [ "$(jq -r .store_sha256 "$pf")" = "$(store_sha "$P/src")" ] || die "the preflight certified another store"
    fi
    mkdir -p "$out/rows" "$out/traces"
    common=(--print --no-session-persistence --model "$MODEL" --max-turns "$MAX_TURNS" --output-format stream-json --verbose
            --include-partial-messages --permission-mode dontAsk --strict-mcp-config --json-schema "$SCHEMA")
    launches='{}'; trap '[ "${#ARM_TMP[@]:-0}" = 0 ] || rm -f "${ARM_TMP[@]}"' EXIT
    for arm in $arms; do launch_env "$project" "$arm"; arm_launch "$(arm_name "$project" "$arm")" "$project" "$MODEL"; clear_env; launches="$(jq -c --arg a "$arm" --argjson l "$ARM_LAUNCH" '. + {($a): $l}' <<<"$launches")"; done
    manifest="$(jq -n --arg project "$project" --arg model "$MODEL" --argjson runs "$runs" --arg arms "$arms" --arg qf "$questions" --arg qsha "$(sha256 "$questions")" \
      --argjson ids "$(jq -c '[.id]' "$questions" | jq -s 'add')" --argjson launches "$launches" --arg mda "$("$MDA" --version)" --arg sha "$(git -C "$REPO" rev-parse HEAD)" \
      --argjson max_turns "$MAX_TURNS" --argjson timeout "$TIMEOUT" --argjson retries "$RETRIES" --arg at "$(date -u +%FT%TZ)" --arg mda_sha "$(sha256 "$MDA")" --arg preamble "$PREAMBLE" --argjson schema "$SCHEMA" \
      --arg store "$(store_sha "$P/src")" --arg val "$(sha256 "$val")" --arg steps "$(sha256 "$RES/$project/steps.jsonl")" --arg fz "$(frozen_sha)" --arg smoke "${T5_SMOKE:-}" \
      --argjson accepted "$(jq -c '.accepted_probe_failures // []' "$RESULTS/preflight/T5-$project.json" 2>/dev/null || echo '[]')" \
      '{table: "T5", project: $project, smoke: ($smoke != ""), accepted_probe_failures: $accepted, model: $model, runs: $runs, arms: ($arms | split(" ")), question_ids: $ids, questions_file: $qf, questions_sha256: $qsha,
        launches: $launches, mda: $mda, mda_sha256: $mda_sha, preamble: $preamble, answer_schema: $schema, store_sha256: $store, validation_sha256: $val, steps_sha256: $steps, frozen_sha256: $fz, source_commit: $sha,
        max_turns: $max_turns, timeout_s: $timeout, retries: $retries, recorded_at: $at,
        rules: {failures: "a missing, errored, timed-out run or one without a structured answer is a row scored 0 correct; its effort (every attempt summed) is kept (rule 0.3)", tokens: "transcript usage per turn from message_delta, summed over attempts (rule 0.7)", servers: "every run starts its MCP server cold (rule 0.9)", git: "every arm but mda-index has .git and Bash restricted to git (plan §4 T5)"}}')"
    fingerprint() { jq -S 'del(.recorded_at, .source_commit) | .launches |= map_values(.claude_flags |= map(if test("mda-arm-mcp|qmd-skill") then "<tmp>" else . end))' "$@"; }
    if [ -f "$out/manifest.json" ]; then
      fingerprint "$out/manifest.json" > "$out/.m.a"; fingerprint <<<"$manifest" > "$out/.m.b"
      diff "$out/.m.a" "$out/.m.b" > "$out/.m.diff" || die "$out/manifest.json describes another configuration (see $out/.m.diff)"
      rm -f "$out/.m.a" "$out/.m.b" "$out/.m.diff"; echo "resuming $out"
    else echo "$manifest" > "$out/manifest.json"; fi
    one() { # id q arm run — always leaves a row (an error row at worst)
      local id="$1" q="$2" arm="$3" r="$4" key trace attempts='[]' rc t0 t1 a att merged
      key="$id-$arm-$r"; trace="$out/traces/$key.jsonl"
      launch_env "$project" "$arm"; arm_launch "$(arm_name "$project" "$arm")" "$project" "$MODEL"; clear_env
      t0=$(date +%s)
      for ((a = 1; a <= RETRIES + 1; a++)); do
        rc=0; run_once "$ARM_CORPUS" "$trace.a$a" "$q" "${common[@]}" --setting-sources "$ARM_SETTING_SOURCES" "${ARM_ARGS[@]}" || rc=$?
        att="$(attempt_of "$trace.a$a" "$ARM_WANT")"
        attempts="$(jq -c --argjson rc "$rc" --argjson att "$att" --arg err "$( (tail -c 300 "$trace.a$a.err" 2>/dev/null || true) | tr '\n' ' ')" --arg at "$(date -u +%FT%TZ)" --argjson n "$a" '. + [{attempt: $n, exit_code: $rc, timed_out: ($rc == 124), stderr: $err, at: $at} + $att]' <<<"$attempts")"
        [ -s "$trace.a$a.err" ] || rm -f "$trace.a$a.err"
        if [ "$rc" = 0 ] && [ "$(jq -r .result_ok <<<"$att")" = true ]; then break; fi
      done
      t1=$(date +%s)
      # the scored row: the LAST attempt's answer, every attempt's effort summed (Codex M8 F8)
      merged="$(jq -c --arg id "$id" --arg arm "$arm" --argjson run "$r" --arg q "$q" --argjson wall "$((t1 - t0))" --arg trace "$(basename "$trace")" '
        (last) as $l |
        {id: $id, arm: $arm, run: $run, q: $q, trace: $trace, attempts: ., attempts_made: length, exit_code: $l.exit_code, wall_s: $wall,
         structured: $l.structured, answer_text: $l.answer_text, turns: $l.turns, models: ([.[].models[]] | unique),
         tool_calls: (map(.tool_calls) | add), failed_calls: (map(.failed_calls) | add), tools: ([.[].tools[]]), git_commands: ([.[].git_commands[]]),
         source_tokens: (map(.source_tokens) | add), source_tokens_signed: (map(.source_tokens_signed) | add),
         input_tokens: (if any(.[]; .input_tokens == null) then null else (map(.input_tokens) | add) end), output_tokens: (if any(.[]; .output_tokens == null) then null else (map(.output_tokens) | add) end),
         cost_usd: (if any(.[]; .cost_usd == null) then null else (map(.cost_usd) | add) end), cost_usd_last_attempt: $l.cost_usd,
         arm_tool_used: (any(.[]; .arm_tool_used)), parsed: (all(.[]; .parsed)),
         error: (($l.result_ok | not) or ($l.exit_code != 0)), stderr: (if ($l.result_ok | not) then $l.stderr else null end)}' <<<"$attempts" 2>/dev/null)" \
        || merged="$(jq -n --arg id "$id" --arg arm "$arm" --argjson run "$r" --arg q "$q" --arg trace "$(basename "$trace")" '{id: $id, arm: $arm, run: $run, q: $q, trace: $trace, error: true, parsed: false, tool_calls: 0, source_tokens: 0, structured: null, note: "row assembly failed; see the attempt traces"}')"
      printf '%s\n' "$merged" > "$out/rows/$key.json.tmp" && mv "$out/rows/$key.json.tmp" "$out/rows/$key.json"
      [ "${#ARM_TMP[@]}" = 0 ] || rm -f "${ARM_TMP[@]}"
      if [ "$(jq -r .error "$out/rows/$key.json")" = true ]; then echo "$id $arm run $r: ERROR (exit $(jq -r .exit_code "$out/rows/$key.json"), $(jq -r .attempts_made "$out/rows/$key.json") attempt(s))"; else echo "$id $arm run $r: $(jq -r '"\(.tool_calls) calls · \(.source_tokens) source tokens · \(.wall_s) s · tool \(.arm_tool_used) · \(.attempts_made) attempt(s)"' "$out/rows/$key.json")"; fi
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
    n_rows="$(grep -c . "$out/runs.jsonl")"; n_err="$(jq -s 'map(select(.error)) | length' "$out/runs.jsonl")"
    echo "$out: $n_rows of $n_total rows ($n_done were present; $n_err error rows, kept)"
    [ "$n_rows" = "$n_total" ] || { echo "INCOMPLETE: rerun to fill the missing rows" >&2; exit 3; }
    [ "$n_err" = 0 ] || exit 2 ;;

  grade)
    # Deterministic (Codex M8 F11): page answers are resolved against the historical page
    # universe of the project (every markdown path under a root at any step) after stripping
    # `./`, `<checkout_dir>/` or `<project>/`; an answer that is the unique suffix of one
    # universe path resolves to it, else it stays as given (and counts as wrong). Section answers
    # are resolved against the heading paths of the page at the final checkout: an exact path, or
    # the unique path ending in the answer; matching is deterministic (sorted). Dates: within one
    # day. A failed run (no structured answer) scores 0; its effort stays. Refuses an incomplete
    # run set: every manifest cell must have a row.
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; out="$(runs_dir "$project")"; [ -f "$out/manifest.json" ] || die "no manifest"; assemble "$out"
    expected="$(jq -r '(.question_ids | length) * (.arms | length) * .runs' "$out/manifest.json")"; have="$(grep -c . "$out/runs.jsonl")"
    [ "$have" = "$expected" ] || die "$have of $expected rows present: rerun \`t5.sh run\` to fill the missing rows before grading (rule 0.3: no silent gaps)"
    cdir="$(src_field "$project" checkout_dir)"; roots="$(src_field "$project" content_roots)"
    python3 - "$out/runs.jsonl" "$(jq -r .questions_file "$out/manifest.json")" "$P/src" "$RES/$project/steps.jsonl" "$roots" "$cdir" "$project" "$out/grades.jsonl" "$MDA" <<'PY' || die "grading failed"
import datetime, json, os, re, subprocess, sys
runs_f, q_f, src, steps_f, roots, cdir, project, out, mda = sys.argv[1:10]
roots = roots.split(); steps = [json.loads(l) for l in open(steps_f)]
qs = {json.loads(l)["id"]: json.loads(l) for l in open(q_f, encoding="utf-8")}
def git(*a): return subprocess.run(["git", "-C", src, *a], check=True, capture_output=True, text=True).stdout
def is_md(p): return p.endswith((".md", ".mdx", ".markdown"))
def under_root(p): return any(p == r or p.startswith(r.rstrip("/") + "/") for r in roots)
universe = set()
for s in steps: universe |= {p for p in git("ls-tree", "-r", "--name-only", s["sha"]).splitlines() if is_md(p) and under_root(p)}
universe = sorted(universe)
def norm_page(p):
    p = p.strip().strip("`'\"")
    while p.startswith("./"): p = p[2:]
    for pre in (cdir + "/", project + "/"):
        if p.startswith(pre): p = p[len(pre):]
    if p in universe: return p
    hits = [c for c in universe if c.endswith("/" + p)]
    return hits[0] if len(hits) == 1 else p
head_cache = {}
def headings(page):
    if page not in head_cache:
        r = subprocess.run([mda, "--json", "parse", os.path.join(src, page)], capture_output=True, text=True)
        head_cache[page] = sorted({" › ".join(s["heading_path"]) for s in json.loads(r.stdout)["sections"] if s["heading_path"]}) if r.returncode == 0 else []
    return head_cache[page]
def fold(s): return re.sub(r"\s+", " ", s.strip().strip("`'\"")).lower()
def resolve_section(ans, page):
    hs = headings(page); a = fold(ans)
    exact = [h for h in hs if fold(h) == a]
    if exact: return exact[0]
    suffix = [h for h in hs if fold(h).endswith(" › " + a)]
    return suffix[0] if len(suffix) == 1 else ans
def f1(a, b):
    a, b = set(a), set(b)
    if not a and not b: return 1.0
    tp = len(a & b)
    if tp == 0: return 0.0
    pr, rc = tp / len(a), tp / len(b); return 2 * pr * rc / (pr + rc)
n = 0
with open(out, "w", encoding="utf-8") as fh:
    for l in open(runs_f, encoding="utf-8"):
        r = json.loads(l); q = qs[r["id"]]; s = r.get("structured") or {}
        g = {"id": r["id"], "arm": r["arm"], "run": r["run"], "type": q["type"], "kind": q.get("kind"), "error": bool(r.get("error")), "tool_calls": r.get("tool_calls", 0), "source_tokens": r.get("source_tokens", 0),
             "wall_s": r.get("wall_s"), "cost_usd": r.get("cost_usd"), "attempts_made": r.get("attempts_made"), "arm_tool_used": r.get("arm_tool_used"), "git_used": any(c.startswith("git") for c in r.get("git_commands", []))}
        if r.get("error") or not s: g.update({"correct": 0, "score": None, "detail": "no structured answer"})
        elif q["type"] == "when":
            d = (s.get("date") or "").strip()[:10]
            try: delta = abs((datetime.date.fromisoformat(d) - datetime.date.fromisoformat(q["truth"]["date"])).days)
            except ValueError: delta = None
            g.update({"correct": int(delta is not None and delta <= 1), "score": delta, "detail": f"answer {d or '∅'} vs {q['truth']['date']}"})
        elif "sections" in q["truth"]:
            ans = sorted({resolve_section(x, q["page"]) for x in (s.get("sections") or []) if x.strip()}); sc = f1(ans, q["truth"]["sections"])
            g.update({"correct": int(sc >= 0.8), "score": round(sc, 3), "detail": f"{len(ans)} answered vs {len(q['truth']['sections'])}"})
        else:
            ans = sorted({norm_page(p) for p in (s.get("pages") or []) if p.strip()}); sc = f1(ans, q["truth"]["pages"])
            g.update({"correct": int(sc >= 0.8), "score": round(sc, 3), "detail": f"{len(ans)} answered vs {len(q['truth']['pages'])}"})
        fh.write(json.dumps(g, ensure_ascii=False) + "\n"); n += 1
print(f"{n} rows graded → {out}")
PY
    if [ -z "${T5_SMOKE:-}" ]; then mkdir -p "$RES/$project"; cp "$out/grades.jsonl" "$RES/$project/grades.jsonl"; cp "$out/manifest.json" "$RES/$project/manifest.json"; fi
    jq -s 'group_by(.arm) | map({(.[0].arm): {rows: length, correct: (map(.correct) | add), errors: (map(select(.error)) | length), git_used: (map(select(.git_used)) | length), tool_used: (map(select(.arm_tool_used)) | length)}}) | add' "$out/grades.jsonl" ;;

  analysis)
    project="${1:?project}"; shift; ident "$project"; src="$RES/$project"; [ -z "${T5_SMOKE:-}" ] || src="$(runs_dir "$project")"
    comps="$(jq -r '.arms | map(select(. != "mda" and . != "mda-index")) | join(",")' "$src/manifest.json")"
    arm="${T5_ANALYSIS_ARM:-mda}"   # T5_ANALYSIS_ARM=mda-index: the time index alone against the same comparators (informational companion)
    "$MDA" ${1:+"$1"} eval --replay-analysis "$src/grades.jsonl" --manifest "$src/manifest.json" --mda-arm "$arm" --comparators "$comps" ;;

  redo)
    project="${1:?project}"; id="${2:?id}"; arm="${3:?arm}"; r="${4:?run}"; out="$(runs_dir "$project")"; key="$id-$arm-$r"
    sup="$out/superseded/$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$sup"
    for f in "$out/rows/$key.json" "$out"/traces/"$key".jsonl*; do [ -e "$f" ] && mv "$f" "$sup/"; done
    echo "moved $key to $sup (disclose the replacement on the page)" ;;

  preflight)
    project="${1:?project}"; ident "$project"; P="$(pdir "$project")"; rev="$(src_field "$project" revision)"
    report="$RESULTS/preflight/T5-$project.json"; safe_target "$report"; checks='[]'; failed=0; accepted='[]'
    rec() { checks="$(jq -c --arg n "$1" --arg s "$2" --arg d "$3" '. + [{name: $n, status: $s, detail: $d}]' <<<"$checks")"; [ "$2" != FAIL ] || failed=$((failed + 1)); echo "$2 $1: $3"; }
    jq -n --arg p "$project" --arg at "$(date -u +%FT%TZ)" '{table: "T5", project: $p, run_at: $at, status: "in progress", passed: false}' > "$report"
    if env | grep -qE '^[A-Z_]*_API_KEY='; then rec env FAIL "a provider key is exported"; else rec env ok "no provider key · claude $(claude --version 2>/dev/null | head -1)"; fi
    if out="$("$REPO/scripts/eval/freeze.sh" --protocol final --table T5 --check 2>&1)"; then rec frozen ok "$out"; else rec frozen FAIL "$out"; fi
    val="$RES/$project/validation.json"
    if [ "$(jq -r .passed "$val" 2>/dev/null)" = true ] && [ "$(jq -r .store_sha256 "$val")" = "$(store_sha "$P/src")" ] && [ "$(jq -r .steps_sha256 "$val")" = "$(sha256 "$RES/$project/steps.jsonl")" ]; then rec validation ok "$(jq -r '"\(.docs_checked)/\(.expected_documents) docs, \(.sections_checked) sections, \(.tombstones_checked)/\(.expected_tombstones) tombstones, 0 mismatches; bound to store \(.store_sha256[:12])…"' "$val")"; else rec validation FAIL "validation.json missing, not passed, or not bound to the current store and step list"; fi
    n_rec="$(ls "$P"/replay/[0-9]*.json 2>/dev/null | wc -l | tr -d ' ')"
    if [ "$n_rec" = 0 ]; then rec replay FAIL "no step records"; else
      known="$(frozen_versions | tr '\n' ' ')"
      bad="$(jq -r --arg known "$known" --arg bin "$(sha256 "$MDA")" '. as $r | select((($known | split(" ")) | index($r.frozen_sha256)) == null or $r.mda_sha256 != $bin) | .step' "$P"/replay/[0-9]*.json | tr '\n' ' ')"
      under="$(jq -r '.frozen_sha256' "$P"/replay/[0-9]*.json | sort -u | tr '\n' ' ')"
      [ -z "$bad" ] && rec replay ok "$n_rec step records under this binary and a committed T5 freeze (FROZEN.md sha256 $under; current $(frozen_sha))" || rec replay FAIL "steps $bad were replayed under an unknown freeze or another binary (a dry run is never counted)"
    fi
    [ "$(store_sha "$P/src")" = "$(jq -r .store_sha256 "$P/replay/embeddings.json" 2>/dev/null)" ] && rec store ok "mda store unchanged since the replay ($(store_sha "$P/src" | cut -c1-12)…)" || rec store FAIL "mda store differs from the end of the replay"
    for a in $ARMS; do launch_env "$project" "$a"; d="$ARM_CORPUS_OVERRIDE"; clear_env; refs="$(git -C "$d" for-each-ref --format='%(refname) %(objectname)' 2>/dev/null | tr '\n' ' ')"; [ "$refs" = "refs/heads/main $rev " ] && [ "$(git -C "$d" rev-parse HEAD)" = "$rev" ] && rec "copy-$a" ok "$d: main at $rev, no other refs" || rec "copy-$a" FAIL "$d: HEAD $(git -C "$d" rev-parse HEAD 2>/dev/null || echo none), refs: $refs"; done
    [ "$(sha256 "$P/graphify/graph.json" 2>/dev/null)" = "$(gunzip -c "$(graphify_archive "$project")" | shasum -a 256 | cut -c1-64)" ] && rec graph ok "served graph = archived $(basename "$(graphify_archive "$project")")" || rec graph FAIL "served graph differs from the archive"
    [ -f "$HOME/.config/qmd/t5-qmd-$project.yml" ] && rec qmd-index ok "t5-qmd-$project fingerprint $(qmd_fingerprint "t5-qmd-$project")" || rec qmd-index FAIL "no index t5-qmd-$project"
    # activation probes (rule 0.5): the first three questions of the T2 sample through probe.sh,
    # each arm on its copy (a question naming the page proves nothing: T4 lesson); the git arm's
    # tool is `git` itself, which a documentation question never needs, so it is probed three
    # times with a fixed history question that is not in the sample.
    mkdir -p "$RESULTS/preflight/T5-$project/probes"; : > "$RESULTS/preflight/T5-$project/probes/summary.jsonl"
    for a in $ARMS; do
      n_ok=0
      if [ "$a" = git ]; then
        for k in 1 2 3; do
          launch_env "$project" git; arm_launch git "$project" "$MODEL"; clear_env; tr="$RESULTS/preflight/T5-$project/probes/git-history-$k.jsonl"
          (cd "$ARM_CORPUS" && claude --print --no-session-persistence --model "$MODEL" --max-turns 8 --output-format stream-json --verbose --permission-mode dontAsk --strict-mcp-config --setting-sources "$ARM_SETTING_SOURCES" "${ARM_ARGS[@]}" -- "$GIT_PROBE_Q" </dev/null > "$tr" 2>"$tr.err") || true
          [ "${#ARM_TMP[@]}" = 0 ] || rm -f "${ARM_TMP[@]}"; [ -s "$tr.err" ] || rm -f "$tr.err"
          att="$(attempt_of "$tr" '^Bash$')"; jq -c --arg q "$GIT_PROBE_Q" '{arm: "git", question: $q, activated: .arm_tool_used, tools, git_commands}' <<<"$att" >> "$RESULTS/preflight/T5-$project/probes/summary.jsonl"
          [ "$(jq -r .arm_tool_used <<<"$att")" = true ] && n_ok=$((n_ok + 1))
        done
      else
        for qid in $(head -3 "$RESULTS/T2/$project/questions.jsonl" | jq -r .id); do
          launch_env "$project" "$a"; rc=0
          line="$("$REPO/scripts/eval/probe.sh" "$(arm_name "$project" "$a")" "$project" "$qid" "$RESULTS/preflight/T5-$project/probes/$a-${qid//[^A-Za-z0-9_.-]/_}.jsonl" "$MODEL" 2>>"$P/probes.err")" || rc=$?
          clear_env; [ -z "$line" ] || echo "$line" >> "$RESULTS/preflight/T5-$project/probes/summary.jsonl"
          [ "$rc" = 0 ] && n_ok=$((n_ok + 1))
        done
      fi
      if [ "$n_ok" = 3 ]; then rec "probes-$a" ok "3 of 3 probes made a successful call of the arm's tool (on its copy)"
      elif [[ " ${T5_ACCEPT_PROBE_FAIL:-} " == *" $a "* ]]; then
        # A declared exception (T1 precedent, Supabase graphify-haiku 2/3): the arm is published
        # with its probe result disclosed; the reason travels in the report and the manifest.
        [ -n "${T5_ACCEPT_REASON:-}" ] || die "T5_ACCEPT_PROBE_FAIL needs T5_ACCEPT_REASON"
        accepted="$(jq -c --arg a "$a" --argjson n "$n_ok" --arg why "$T5_ACCEPT_REASON" '. + [{arm: $a, probes_activated: $n, of: 3, reason: $why}]' <<<"$accepted")"
        rec "probes-$a" ACCEPTED-FAIL "$n_ok of 3 probes activated; accepted and disclosed: $T5_ACCEPT_REASON"
      else rec "probes-$a" FAIL "$n_ok of 3 probes activated (see $P/probes.err and the probe traces)"; fi
    done
    jq -n --arg p "$project" --arg at "$(date -u +%FT%TZ)" --argjson checks "$checks" --argjson failed "$failed" --argjson accepted "$accepted" --arg sha "$(git -C "$REPO" rev-parse HEAD)" --arg store "$(store_sha "$P/src")" --arg fz "$(frozen_sha)" --arg qi "$(qmd_fingerprint "t5-qmd-$project" 2>/dev/null || echo none)" \
      '{table: "T5", project: $p, run_at: $at, source_commit: $sha, frozen_sha256: $fz, store_sha256: $store, qmd_index_fingerprint: $qi, checks: $checks, failed: $failed, accepted_probe_failures: $accepted, status: (if $failed == 0 then (if ($accepted | length) > 0 then "passed with accepted probe failures" else "passed" end) else "failed" end), passed: ($failed == 0)}' > "$report"
    echo "$report: $([ "$failed" = 0 ] && echo PASSED || echo "FAILED ($failed)")"; [ "$failed" = 0 ] ;;

  table)
    # Rendered from the archived analyses (manifest-complete: a missing run is a failed run there),
    # never from the present grade rows alone (Codex M8 F16).
    python3 - "$RES" "$PROJECTS" <<'PY'
import json, os, sys
res, projects = sys.argv[1], sys.argv[2].split()
def fmt(x, d=1): return "–" if x is None else (f"{x:.{d}f}" if isinstance(x, float) else str(x))
print("| Project | Steps (days) | Range | Replay (min) | Replay cost (list) | Validation |")
print("|---|---|---|---|---|---|")
for p in projects:
    d = os.path.join(res, p)
    try: rng = json.load(open(f"{d}/range.json")); rep = json.load(open(f"{d}/replay.json")); val = json.load(open(f"{d}/validation.json"))
    except FileNotFoundError: print(f"| {p} | – | – | – | – | not replayed |"); continue
    print(f"| {p} | {rng['steps']} | {rng['start_day']} → {rng['end_day']} ({rng['first_parent_commits']} commits) | {sum(r['seconds'] for r in rep)/60:.0f} | ${sum(r['cost_usd_delta'] for r in rep):.2f} | {'passed' if val['passed'] else 'FAILED'}: {val['docs_checked']}/{val['expected_documents']} docs, {val['sections_checked']} sections, {val['tombstones_checked']}/{val['expected_tombstones']} tombstones, {val['mismatches']} mismatches |")
print()
for p in projects:
    d = os.path.join(res, p); an = f"{d}/analysis.json"
    if not os.path.exists(an): continue
    J = json.load(open(an)); A = J["analysis"] if "analysis" in J else J
    grades = [json.loads(l) for l in open(f"{d}/grades.jsonl")] if os.path.exists(f"{d}/grades.jsonl") else []
    man = json.load(open(f"{d}/manifest.json")); runs_per = man["runs"]
    print(f"**{p}** — per arm (question-level medians over {runs_per} runs; a missing or failed run is 0 correct with its effort as recorded; correct = share of questions whose median run is correct)")
    print()
    print("| Arm | Correct (all) | which | when | added | Tool calls (median) | Source tokens (median) | Wall s (median) | Cost/run (median) | Failed runs | git used | arm tool used |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    arms_sorted = sorted(A["arms"], key=lambda a: ({"mda": 0, "mda-index": 1}.get(a["arm"], 2), a["arm"]))
    for a in arms_sorted:
        rows = [g for g in grades if g["arm"] == a["arm"]]; total = runs_per * a["questions"]
        bt = a["correct_by_type"]
        print(f"| {a['arm']} | {a['correct_rate']:.2f} | {fmt(bt.get('which'), 2)} | {fmt(bt.get('when'), 2)} | {fmt(bt.get('added'), 2)} | {fmt(a['tool_calls'])} | {fmt(a['source_tokens'], 0)} | {fmt(a['wall_s'])} | ${fmt(a['cost_usd'], 3)} | {a['failed']}/{total} | {sum(1 for r in rows if r.get('git_used'))}/{len(rows)} | {sum(1 for r in rows if r.get('arm_tool_used'))}/{len(rows)} |")
    print()
    for an2, who in ((an, "mda"), (f"{d}/analysis.mda-index.json", "mda-index")):
        if not os.path.exists(an2): continue
        J2 = json.load(open(an2)); A2 = J2["analysis"] if "analysis" in J2 else J2
        print(f"**{p}** — {who} vs each arm (paired bootstrap over the questions, {A2.get('draws', 10000)} draws, seed {A2.get('seed', 20260922)}; difference = {who} − arm" + ("; the claim is reserved for mda, this companion is informational)" if who != "mda" else ")"))
        print()
        print("| Pair | Δ correct [95% CI] | Δ tool calls [95% CI] | Δ source tokens [95% CI] | Claim (correct with less effort) |")
        print("|---|---|---|---|---|")
        for pr in A2["pairs"]:
            c = pr["correct"]; t = pr["tool_calls"]; s = pr["source_tokens"]
            verdict = ("**yes**" if pr["claim"] else "no") if who == "mda" else ("would hold" if pr["claim"] else "no")
            print(f"| {who} vs {pr['comparator']} | {c['diff']:+.3f} [{c['lower']:+.3f}, {c['upper']:+.3f}] (n {c['n']}) | {t['diff']:+.1f} [{t['lower']:+.1f}, {t['upper']:+.1f}] (n {t['n']}) | {s['diff']:+.0f} [{s['lower']:+.0f}, {s['upper']:+.0f}] (n {s['n']}) | {verdict} |")
        print()
print("Generated by `scripts/eval/t5.sh table` from `evals/results/docsqa/T5/<project>/{range,replay,validation,manifest}.json`, `grades.jsonl` and `analysis*.json`. Correctness: set F1 ≥ 0.8 for which/added questions, within one day for when questions; a failed or missing run is 0 correct with its effort as recorded (every attempt summed). Dates are first-parent committer dates (UTC days); changes are tree diffs between consecutive daily steps. The claim is made for a pair only when the lower bound of Δ correct ≥ −0.05, the upper bounds of both effort differences are < 0, every question of both arms has an effort value, and mda answered at least one question correctly (plan §4 T5, eligibility clarified 2026-10-15).")
PY
    ;;
esac
