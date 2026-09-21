#!/usr/bin/env bash
# Shared by pre-compact.sh and session-end.sh. Not a hook itself.
# write_auto_handoff ROOT TRIGGER -> sets HANDOFF_FILE and writes docs/handoffs/<date>-<time>-auto.md
write_auto_handoff() {
  local root="$1" trigger="$2" dir stamp
  dir="$root/docs/handoffs"
  mkdir -p "$dir"
  stamp="$(date +%Y-%m-%d-%H%M)"
  HANDOFF_FILE="$dir/$stamp-auto.md"
  {
    echo "# Auto handoff $(date '+%Y-%m-%d %H:%M') ($trigger)"
    echo
    echo "Written by scripts/dev/${trigger}.sh. Narrative part (doing / intent / next step) comes from /handoff."
    echo
    echo "## Files touched (git status --short)"
    echo '```'
    git -C "$root" status --short 2>/dev/null || echo "(not a git repository)"
    echo '```'
    echo
    echo "## Uncommitted (git diff --stat)"
    echo '```'
    git -C "$root" diff --stat 2>/dev/null || echo "(not a git repository)"
    echo '```'
  } > "$HANDOFF_FILE"
}
