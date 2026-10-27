#!/usr/bin/env bash
# T6 reads nothing of DocsQA (M9 plan, pre-mortem F5): with the dataset location unreadable, the
# T6 paths that run without a model still work on a tiny corpus — setup from a git archive of the
# golden docs into a temporary RUN, inventory, a raw index, the own-root card path, the T6
# resolver through t2.sh's grade helpers (a synthetic grades file with no model: nothing is
# graded, the completeness gate refuses an incomplete manifest), and the private export's schema
# check on a synthetic export. Run by `make check` (target t6-tests) with the debug binary.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../../.." && pwd)"
MDA="${1:-${MDA:-$repo/target/debug/mda}}"; [ -x "$MDA" ] || { echo "no mda binary at $MDA (cargo build)" >&2; exit 1; }
tmp="$(mktemp -d -t mda-t6-smoke.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
export RUN="$tmp/run" MDA REPO="$repo" DOCSQA_DATA="$tmp/no-such-dataset" HOME_REAL="$HOME"
mkdir -p "$RUN"
fail() { echo "FAIL: $*" >&2; exit 1; }
# 1. setup golden from the archive at HEAD (sources.json goes to a scratch copy of evals/results/own, never the repo)
export T6_SMOKE=1
cp -R "$repo/evals/results/own" "$tmp/own"; mkdir -p "$tmp/own/T6"
# t6.sh writes sources.json under RES_PUB = $REPO/evals/results/own; point REPO's results at the scratch copy through a shim repo
shim="$tmp/repo"; mkdir -p "$shim/evals/results" "$shim/scripts/eval"; ln -s "$repo/scripts/eval/"* "$shim/scripts/eval/"; ln -s "$repo/skills" "$shim/skills"; ln -s "$repo/prompts" "$shim/prompts"
cp -R "$tmp/own" "$shim/evals/results/own"; ln -s "$repo/.git" "$shim/.git"
REPO="$shim" "$shim/scripts/eval/t6.sh" setup golden >/dev/null || fail "setup golden"
[ -s "$RUN/t6/golden/pages.txt" ] || fail "no inventory"
[ "$(grep -c . "$RUN/t6/golden/pages.txt")" = "$(git -C "$repo" ls-tree -r --name-only HEAD -- evals/golden/docs | grep -c '\.md$')" ] || fail "inventory count differs from the archive"
grep -q '"source_commit"' "$shim/evals/results/own/T6/sources.json" || fail "no sources.json"
# 2. a raw index and the own-root card path on the copy (no model)
"$MDA" index --no-summarize --root "$RUN/t6/golden/src" >/dev/null 2>&1 || fail "raw index"
"$MDA" --json eval --root "$RUN/t6/golden/src" --cards "$repo/evals/golden/cards.json" --export-cards "$tmp/cards.json" --project golden | jq -e '.card_coverage.complete == true and .cards_exported > 0' >/dev/null || fail "own-root card path"
# 3. the resolver through the inventory: an exact page resolves, a traversal does not
[ "$(python3 -I "$repo/scripts/eval/t6_resolve.py" "$RUN/t6/golden/src" "$RUN/t6/golden/pages.txt" golden "runbooks/deploy.md")" = "runbooks/deploy.md" ] || fail "resolver exact"
[ -z "$(python3 -I "$repo/scripts/eval/t6_resolve.py" "$RUN/t6/golden/src" "$RUN/t6/golden/pages.txt" golden "../../../etc/passwd")" ] || fail "resolver traversal"
# 4. grade refuses an incomplete manifest under T2_REQUIRE_COMPLETE (no model call is made)
out="$tmp/runs"; mkdir -p "$out/rows"
jq -n '{project: "golden", model: "none", runs: 1, arms: ["mda"], question_ids: ["golden-c001"], questions_file: "none", questions_sha256: "none"}' > "$out/manifest.json"
if T2_CORPUS="$RUN/t6/golden/src" T2_PAGES="$RUN/t6/golden/pages.txt" T2_RUBRIC=t6 T2_REQUIRE_COMPLETE=1 "$repo/scripts/eval/t2.sh" grade golden "$out" >/dev/null 2>"$tmp/grade.err"; then fail "grade accepted an incomplete manifest"; fi
grep -q "incomplete" "$tmp/grade.err" || fail "grade did not name the gap: $(cat "$tmp/grade.err")"
# 5. materialisation of a hashed sample with MULTILINE evidence (review F21/F22): the full record
#    comes back whole; a tampered evidence hash and a duplicate id are refused
ev="$(printf 'Line one of the section.\n\nLine two, with a date 2026-03-01 and a `code` span.\nLine three.')"
python3 -I "$here/t6-smoke-fixture.py" "$tmp/full.jsonl" "$tmp/sample.jsonl" "$ev"
T2_QUESTIONS_FULL="$tmp/full.jsonl" "$repo/scripts/eval/t2.sh" materialise private "$tmp/sample.jsonl" "$tmp/out.full.jsonl" || fail "materialise refused a valid hashed sample"
[ "$(jq -r .evidence "$tmp/out.full.jsonl" | wc -l | tr -d ' ')" = 4 ] || fail "multiline evidence was not materialised whole"
sed 's/"evidence_sha256": "[0-9a-f]*"/"evidence_sha256": "0000000000000000000000000000000000000000000000000000000000000000"/' "$tmp/full.jsonl" > "$tmp/bad.jsonl"
if T2_QUESTIONS_FULL="$tmp/bad.jsonl" "$repo/scripts/eval/t2.sh" materialise private "$tmp/sample.jsonl" "$tmp/x.jsonl" >/dev/null 2>&1; then fail "a tampered evidence hash was accepted"; fi
cat "$tmp/full.jsonl" "$tmp/full.jsonl" > "$tmp/dup.jsonl"
if T2_QUESTIONS_FULL="$tmp/dup.jsonl" "$repo/scripts/eval/t2.sh" materialise private "$tmp/sample.jsonl" "$tmp/x.jsonl" >/dev/null 2>&1; then fail "a duplicate id was accepted"; fi
# 6. citation extraction (review F23): a backticked odd name yields one citation, not two; bare paths still found
cites="$(printf 'See `c/odd name (v2).md` and docs/a.mdx, plus `x/y.md#anchor`, not docs/a.md.bak nor docs/b.md_old nor docs/c.md/child nor docs/d.md+backup nor docs/e.md=old; (e/f.markdown). A [guide](g/h.md) and `_notes.md` and *i/j.md*. Links: [docs/t.md](l/b.md), [guide](l/c.md "title"), [g](<l/odd name.md>), [g](l/a(v2).md).' | "$repo/scripts/eval/t2.sh" cite | LC_ALL=C sort | tr '\n' '|')"
[ "$cites" = "_notes.md|c/odd name (v2).md|docs/a.mdx|e/f.markdown|g/h.md|i/j.md|l/a(v2).md|l/b.md|l/c.md|l/odd name.md|x/y.md|" ] || fail "citation extraction: got $cites"
# 7. the private export round trip: a synthetic private run directory exports and conforms; a hand
#    edit of a committed file (a heading in a numeric field), an unknown key and an unknown file are
#    each refused; the question digests are the exporter's own serialisation
mkdir -p "$tmp/privrun/runs/panel"; cp "$tmp/full.jsonl" "$tmp/privrun/questions.full.jsonl"
python3 -I "$repo/scripts/eval/t6_private_export.py" export "$tmp/privrun" "$tmp/privout" >/dev/null || fail "a valid private export was refused"
python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null || fail "the exported files do not conform"
diff <(python3 -I "$repo/scripts/eval/t6_private_export.py" digest-questions "$tmp/privrun/questions.full.jsonl") "$tmp/privout/questions.jsonl" >/dev/null || fail "digest-questions is not the exporter's serializer"
sed -i.bak 's/"required_facts_n": 2/"required_facts_n": "Some Secret Heading"/' "$tmp/privout/questions.jsonl"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null 2>&1; then fail "a string in a numeric field was accepted"; fi
mv "$tmp/privout/questions.jsonl.bak" "$tmp/privout/questions.jsonl"
echo '{"id":"private-c003","leak":"x"}' >> "$tmp/privout/questions.jsonl"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null 2>&1; then fail "an unknown key was accepted"; fi
sed -i.bak '$d' "$tmp/privout/questions.jsonl"; rm -f "$tmp/privout/questions.jsonl.bak"
echo '{"a":1}' > "$tmp/privout/notes.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null 2>&1; then fail "an unknown file was accepted"; fi
rm -f "$tmp/privout/notes.json"; echo '{"a":1}' > "$tmp/privout/.hidden.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null 2>&1; then fail "a hidden file was accepted"; fi
rm -f "$tmp/privout/.hidden.json"; printf '%s  secret/path.md\n' "$(printf x | shasum -a 256 | cut -c1-64)" > "$tmp/privout/FROZEN.sha256"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null 2>&1; then fail "a digest file with trailing text was accepted"; fi
printf '%s\n' "$(printf x | shasum -a 256 | cut -c1-64)" > "$tmp/privout/FROZEN.sha256"
python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/privout" >/dev/null || fail "a bare digest file was refused"
# the committed question digests are frozen: an export whose digests differ is refused
sed 's/"q": "What happens on line two?"/"q": "What happens on line three?"/' "$tmp/full.jsonl" > "$tmp/privrun/questions.full.jsonl"
if python3 -I "$repo/scripts/eval/t6_private_export.py" export "$tmp/privrun" "$tmp/privout" >/dev/null 2>&1; then fail "an export with changed question digests replaced the committed ones"; fi
[ -f "$tmp/privout/questions.jsonl" ] || fail "the committed export was lost on a refused replacement"
# the calibration schema: a conforming record passes, a path in the rule is still prose but a wrong field is refused
python3 -I "$repo/scripts/eval/t6_private_export.py" project-pilot <(echo '{"corpus":"golden","at":"2026-10-27T00:00:00Z","index":{"seconds":1,"counts":{"docs":1}},"candidates":{},"runs":{"n":2,"errors":0,"wall_s_total":10,"cost_usd":0.1},"seconds":{"runs":10,"grade":5,"panel":5},"panel":{"sample":2,"incomplete":0,"agreement":{},"resolved_by_panel":0}}') golden > "$tmp/pp.json" || fail "project-pilot failed"
note="the development pilot: disjoint questions, one run per arm, both panel members on every answer; nothing here is a published number"
jq -n --slurpfile p "$tmp/pp.json" --arg note "$note" '{at: "2026-10-27T00:00:00Z", corpora: {golden: $p[0]}, resolved_models: {answering: ["claude-sonnet-5-5"], grader: ["claude-sonnet-5-5"], grounding: ["claude-sonnet-5-5"], panel_fable: ["claude-fable-5-1"], panel_astra: []}, requested_models: {answering: ["sonnet"], grader: ["sonnet"], grounding: ["sonnet"], panel_fable: ["claude-fable-5-1"], panel_astra: ["gpt-6-astra"]}, unresolved_roles: ["panel_astra"], resolution_rule: "either-member-differs-by-more-than-one-point-then-panel-mean", estimates: {runs_per_hour: 100, cost_per_run_usd: 0.05, grade_seconds_per_run: 20, acceptance_rate: 0.8}, note: $note}' > "$tmp/cal.json"
python3 -I "$repo/scripts/eval/t6_private_export.py" check-file calibration "$tmp/cal.json" >/dev/null || fail "a conforming calibration record was refused"
jq '.resolved_models.answering = ["/Users/someone/secret"]' "$tmp/cal.json" > "$tmp/cal2.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check-file calibration "$tmp/cal2.json" >/dev/null 2>&1; then fail "an unlisted model id in the calibration record was accepted"; fi
jq '.resolved_models.grader = ["sonnet"]' "$tmp/cal.json" > "$tmp/cal3.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check-file calibration "$tmp/cal3.json" >/dev/null 2>&1; then fail "an alias as a resolved id was accepted"; fi
jq '.resolution_rule = "whatever the owner wrote"' "$tmp/cal.json" > "$tmp/cal4.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check-file calibration "$tmp/cal4.json" >/dev/null 2>&1; then fail "a prose resolution rule was accepted"; fi
jq '.unresolved_roles = []' "$tmp/cal.json" > "$tmp/cal5.json"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check-file calibration "$tmp/cal5.json" >/dev/null 2>&1; then fail "an empty role not declared unresolved was accepted"; fi
# an interrupted replacement: a lone .replaced backup is restored, then the export proceeds; both present → refused
cp "$tmp/full.jsonl" "$tmp/privrun/questions.full.jsonl"
mv "$tmp/privout" "$tmp/privout.replaced"
python3 -I "$repo/scripts/eval/t6_private_export.py" export "$tmp/privrun" "$tmp/privout" >/dev/null || fail "the export did not recover from a lone backup"
[ -d "$tmp/privout" ] && [ ! -d "$tmp/privout.replaced" ] || fail "the backup was not consumed"
mkdir -p "$tmp/privout.replaced"
if python3 -I "$repo/scripts/eval/t6_private_export.py" export "$tmp/privrun" "$tmp/privout" >/dev/null 2>&1; then fail "an export proceeded with both the directory and its backup present"; fi
rm -rf "$tmp/privout.replaced"
# no questions in the run while digests are committed → refused
mv "$tmp/privrun/questions.full.jsonl" "$tmp/privrun/q.away"
if python3 -I "$repo/scripts/eval/t6_private_export.py" export "$tmp/privrun" "$tmp/privout" >/dev/null 2>&1; then fail "an export without the frozen questions replaced the committed digests"; fi
[ -f "$tmp/privout/questions.jsonl" ] || fail "the committed digests were lost"
mv "$tmp/privrun/q.away" "$tmp/privrun/questions.full.jsonl"
# 8. no DocsQA read happened: the unreadable location was never created
[ ! -e "$DOCSQA_DATA" ] || fail "something touched the DocsQA location"
echo "t6-smoke: setup, inventory, raw index, own-root cards, resolver, completeness gate, multiline materialisation, citations, private export round trip (hidden files, digest file, frozen digests, interrupted replacement), calibration schema: ok (DocsQA unreadable throughout)"
