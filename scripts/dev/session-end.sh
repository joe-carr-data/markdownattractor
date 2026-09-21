#!/usr/bin/env bash
# SessionEnd hook: same as pre-compact, plus warn if docs/STATUS.md was not modified this session.
# SessionEnd stdout goes to the debug log only, so the warning is also written into the handoff file.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -euo pipefail
ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
# shellcheck source=/dev/null
. "$ROOT/scripts/dev/lib-handoff.sh"
write_auto_handoff "$ROOT" "session-end"
echo "Auto handoff written: $HANDOFF_FILE"

MARK="$ROOT/.claude/.session-start"
STATUS="$ROOT/docs/STATUS.md"
if [ -f "$MARK" ] && [ -f "$STATUS" ] && ! [ "$STATUS" -nt "$MARK" ]; then
  WARN="WARNING: docs/STATUS.md was not modified this session. Run /handoff (or update STATUS.md) before the next session."
  echo "$WARN"
  echo "$WARN" >&2
  printf '\n## Warning\n%s\n' "$WARN" >> "$HANDOFF_FILE"
fi
exit 0
