#!/usr/bin/env bash
# The private corpus's committed T6 files conform to the allowlisted export schema (M9 plan,
# pre-mortem F8–F13): only approved keys; every string value an opaque id, a digest, a timestamp,
# a fixed arm/tool/model name or a literal that also occurs in the public corpora's files. Also:
# every literal the schema allows occurs in a public corpus's committed file, so no private-only
# string can enter through the schema. Run by `make check` (target t6-tests). Passes trivially
# while no private export exists.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../../.." && pwd)"
priv="$repo/evals/results/own/T6/private"
[ -d "$priv" ] || { echo "own-private-schema: no private export yet"; exit 0; }
[ -f "$priv/schema.json" ] || { echo "own-private-schema: $priv has files but no schema.json" >&2; exit 1; }
python3 -I "$repo/scripts/eval/t6_private_export.py" check "$priv" "$priv/schema.json"
# the schema's literals are a subset of the public corpora's strings (rebuilt and compared)
python3 -I "$repo/scripts/eval/t6_private_export.py" build-schema "$repo/evals/results/own/T6/golden" "$repo/evals/results/own/T6/docs" "$repo/evals/results/own/preflight" \
  | jq -r '.literals[]' | LC_ALL=C sort > "${TMPDIR:-/tmp}/own-lits.$$"
jq -r '.literals[]' "$priv/schema.json" | LC_ALL=C sort | comm -23 - "${TMPDIR:-/tmp}/own-lits.$$" > "${TMPDIR:-/tmp}/own-extra.$$" || true
rm -f "${TMPDIR:-/tmp}/own-lits.$$"
if [ -s "${TMPDIR:-/tmp}/own-extra.$$" ]; then echo "own-private-schema: $(grep -c . "${TMPDIR:-/tmp}/own-extra.$$") schema literal(s) do not occur in any public file" >&2; rm -f "${TMPDIR:-/tmp}/own-extra.$$"; exit 1; fi
rm -f "${TMPDIR:-/tmp}/own-extra.$$"; echo "own-private-schema: schema literals all public"
