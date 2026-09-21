#!/usr/bin/env bash
# PreCompact hook: write a minimal auto handoff from git state, then remind to run /handoff.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -euo pipefail
ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
# shellcheck source=/dev/null
. "$ROOT/scripts/dev/lib-handoff.sh"
write_auto_handoff "$ROOT" "pre-compact"
echo "Auto handoff written: $HANDOFF_FILE"
echo "Reminder: run /handoff for the narrative part (doing / uncommitted intent / next step) and refresh docs/STATUS.md."
exit 0
