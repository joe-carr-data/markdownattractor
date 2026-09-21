#!/usr/bin/env bash
# Stop hook (soft): if crates/ changed but docs/ did not, nudge. Never blocks.
# Plain stdout of a Stop hook only reaches the debug log, so the nudge is emitted as a
# `systemMessage` JSON field (shown to the user as a warning). No `decision` field is ever emitted.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -euo pipefail
ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"

INPUT="$(cat 2>/dev/null || true)"
# Already continuing because of a stop hook: stay silent.
case "$INPUT" in *'"stop_hook_active"'*'true'*) exit 0;; esac

CHANGES="$(git -C "$ROOT" status --porcelain 2>/dev/null || true)"
[ -n "$CHANGES" ] || exit 0

if printf '%s\n' "$CHANGES" | grep -Eq '^.{3}crates/' && ! printf '%s\n' "$CHANGES" | grep -Eq '^.{3}docs/'; then
  echo '{"systemMessage":"crates/ changed but docs/ did not — update aha/STATUS/index?"}'
fi
exit 0
