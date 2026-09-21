#!/usr/bin/env bash
# SessionStart hook: print docs/STATUS.md and the first 20 lines of the active plan.
# Stdout of SessionStart is added to Claude's context.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -euo pipefail
ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
STATUS="$ROOT/docs/STATUS.md"

mkdir -p "$ROOT/.claude"
: > "$ROOT/.claude/.session-start"   # mtime marker; session-end.sh compares STATUS.md against it

[ -f "$STATUS" ] || exit 0
echo "=== docs/STATUS.md ==="
cat "$STATUS"

# "Active plan: docs/plans/foo.md" — take the first token after the label that looks like a path.
PLAN="$(sed -n 's/.*Active plan:[[:space:]]*\([^[:space:]]*docs\/plans\/[^[:space:])]*\.md\).*/\1/p' "$STATUS" | head -1 || true)"
if [ -n "$PLAN" ] && [ -f "$ROOT/$PLAN" ]; then
  echo
  echo "=== $PLAN (first 20 lines) ==="
  head -20 "$ROOT/$PLAN"
fi
exit 0
