#!/usr/bin/env bash
# The small helpers of the T6 harness (scripts/eval/t6_scan.py; Codex pass 3): one candidate hash
# for every caller (stable under key order and non-ASCII), whole-file binary detection, a collision
# filter that tests every item, a tree manifest that lists symlinks. Run by `make check` (t6-tests).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; H="$here/../t6_scan.py"
tmp="$(mktemp -d -t mda-t6-helpers.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
# cand-hash: key order and ASCII escaping do not matter; any field change does
a='{"id":"x-c001","q":"Qué pasa?","section":"Sección — texto","required_facts":[{"fact":"f1","span":"s1"}],"page":"p.md","heading":"[\"H\"]","section_id":"sid"}'
b='{"section_id":"sid","heading":"[\"H\"]","page":"p.md","required_facts":[{"span":"s1","fact":"f1"}],"section":"Sección — texto","q":"Qué pasa?","id":"x-c001"}'
c='{"id":"x-c001","q":"Qué pasa?","section":"Sección — texto","required_facts":[{"fact":"f1","span":"s1"}],"page":"p.md","heading":"[\"H2\"]","section_id":"sid"}'
ha="$(python3 -I "$H" cand-hash "$a")"; hb="$(python3 -I "$H" cand-hash "$b")"; hc="$(python3 -I "$H" cand-hash "$c")"
[ "$ha" = "$hb" ] || fail "cand-hash differs under key order / escaping"
[ "$ha" != "$hc" ] || fail "cand-hash ignores a heading change"
printf '%s\n%s\n' "$a" "$c" > "$tmp/cands.jsonl"; [ "$(python3 -I "$H" cand-hash-file "$tmp/cands.jsonl" | head -1 | cut -f2)" = "$ha" ] || fail "cand-hash-file differs from cand-hash"
# is-binary: a NUL after 100,000 ASCII bytes is still binary; a text file is not; an unreadable file is an error
{ printf 'public\n'; head -c 100000 /dev/zero | tr '\0' 'a'; printf '\0'; } > "$tmp/late.bin"
python3 -I "$H" is-binary "$tmp/late.bin" || fail "a late NUL was not detected"
printf 'plain text\n' > "$tmp/t.txt"; if python3 -I "$H" is-binary "$tmp/t.txt"; then fail "text classified as binary"; fi
python3 -I "$H" is-binary "$tmp/missing" && fail "a missing file did not error" || [ $? = 2 ] || fail "a missing file gave a wrong status"
# collide: overlapping items are both found; a non-public item is not
printf 'long heading\nlong heading expanded\nnot in the baseline at all\n' > "$tmp/items.txt"
printf 'Intro\n## Long Heading Expanded\nbody\n' > "$tmp/base.txt"
[ "$(python3 -I "$H" collide "$tmp/items.txt" "$tmp/base.txt" | sort | tr '\n' '|')" = "long heading|long heading expanded|" ] || fail "collide: $(python3 -I "$H" collide "$tmp/items.txt" "$tmp/base.txt" | tr '\n' '|')"
# tree-manifest: files hashed, symlinks listed with their target, excludes honoured, order stable
mkdir -p "$tmp/tree/a" "$tmp/tree/.claude"; printf 'x' > "$tmp/tree/a/f.md"; ln -s ../a/f.md "$tmp/tree/a/l.md"; printf 'y' > "$tmp/tree/.claude/s.json"
m="$(python3 -I "$H" tree-manifest "$tmp/tree" .claude)"
printf '%s\n' "$m" | grep -q "^file	a/f.md	$(printf x | shasum -a 256 | cut -c1-64)$" || fail "manifest file row"
printf '%s\n' "$m" | grep -q "^link	a/l.md	../a/f.md$" || fail "manifest link row"
! printf '%s\n' "$m" | grep -q ".claude" || fail "exclude not honoured"
echo "t6-helpers: cand-hash, is-binary, collide, tree-manifest: ok"
