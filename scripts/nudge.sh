#!/usr/bin/env bash
# PreToolUse hook on Read|Grep|Glob: when the call targets markdown and an index exists for
# this project, add one line of context reminding Claude that mda_search exists.
# Never blocks, never changes the tool input. Off switch: /mda nudge off (writes DATA/nudge.off).
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -u
DATA="${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugins/data/markdownattractor-markdownattractor}"
[ -f "$DATA/nudge.off" ] && exit 0

input="$(cat 2>/dev/null || true)"
[ -n "$input" ] || exit 0

# Is the call about markdown? Look at file_path / path / pattern / glob fields.
case "$input" in
  *.md\"*|*.md\'*|*.markdown\"*|*'"type":"md"'*) ;;
  *) exit 0 ;;
esac

# Only nudge when this project actually has an index.
project="${CLAUDE_PROJECT_DIR:-$PWD}"
[ -f "$project/.markdownattractor/index.sqlite" ] || exit 0

cat <<'JSON'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"markdownattractor index is available for this project: prefer `mda search \"<question>\" --json` (then `mda open <section_id>` for exact lines) before reading or grepping markdown files whole. Disable this reminder with `/mda nudge off`."}}
JSON
exit 0
