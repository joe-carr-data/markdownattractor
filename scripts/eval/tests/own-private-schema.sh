#!/usr/bin/env bash
# The private corpus's committed T6 files conform to the typed, field-specific export schema of
# scripts/eval/t6_private_export.py (M9 plan, pre-mortem F8–F13; harness review F1–F3): only the
# allowed files, only named keys, every string an opaque id, a digest, a timestamp, a fixed arm/
# tool/model name or a closed enum. Run by `make check` (target t6-tests). Passes trivially while
# no private export exists.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../../.." && pwd)"
priv="$repo/evals/results/own/T6/private"
[ -d "$priv" ] && [ -n "$(find "$priv" -type f | head -1)" ] || { echo "own-private-schema: no private export yet"; exit 0; }
python3 -I "$repo/scripts/eval/t6_private_export.py" check "$priv"
