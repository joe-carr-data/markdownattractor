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
# 5. a synthetic private export conforms; a leaked string does not
mkdir -p "$tmp/priv/panel"; echo '{"literals": ["every cited page checked whole, in batches of four"]}' > "$tmp/priv/schema.json"
echo '{"id":"private-c001","split":"own","category":"single-section","question_sha256":"'"$(printf x | shasum -a 256 | cut -c1-64)"'","reference_sha256":"'"$(printf y | shasum -a 256 | cut -c1-64)"'","evidence_sha256":"'"$(printf z | shasum -a 256 | cut -c1-64)"'","required_facts_n":3}' > "$tmp/priv/questions.jsonl"
python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/priv" "$tmp/priv/schema.json" >/dev/null || fail "a conforming private export was rejected"
echo '{"id":"private-c002","split":"own","category":"single-section","question_sha256":"not a digest: Some Secret Heading","reference_sha256":"x","evidence_sha256":"y","required_facts_n":2}' >> "$tmp/priv/questions.jsonl"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/priv" "$tmp/priv/schema.json" >/dev/null 2>&1; then fail "a leaking private export was accepted"; fi
echo '{"id":"private-c003","leak":"x"}' > "$tmp/priv/questions.jsonl"
if python3 -I "$repo/scripts/eval/t6_private_export.py" check "$tmp/priv" "$tmp/priv/schema.json" >/dev/null 2>&1; then fail "an unknown key was accepted"; fi
# 6. no DocsQA read happened: the unreadable location was never created
[ ! -e "$DOCSQA_DATA" ] || fail "something touched the DocsQA location"
echo "t6-smoke: setup, inventory, raw index, own-root cards, resolver, completeness gate, private schema: ok (DocsQA unreadable throughout)"
