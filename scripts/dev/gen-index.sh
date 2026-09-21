#!/usr/bin/env bash
# Regenerate docs/index.md deterministically from the files under docs/.
# Order: STATUS.md, aha.md pinned; then plans/, design/, adr/, reviews/, handoffs/ (last 5), then the rest.
# Within a group: newest first (git commit date, else mtime), then path descending (dated/numbered names).
# "What" column: existing description in the current index.md wins (hand-written text is never lost);
# otherwise the first non-empty line after the H1, or the H1 itself, truncated to 100 chars.
# The "## Not yet written" section of the current index.md is copied verbatim to the bottom.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -euo pipefail
ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
DOCS="$ROOT/docs"
INDEX="$DOCS/index.md"
[ -d "$DOCS" ] || exit 0
cd "$DOCS"

FILES="$(find . -type f -name '*.md' ! -path './archive/*' ! -name 'index.md' | sed 's|^\./||' | LC_ALL=C sort)"
[ -n "$FILES" ] || exit 0

TMP="$(mktemp)"
# Tagged lines for one awk pass:  G<TAB>date<TAB>docs/path | M<TAB>date<TAB>path | E<TAB>path<TAB>what | F<TAB>path
{
  git -C "$ROOT" log --format='__DATE__ %ad' --date=short --name-only -- docs 2>/dev/null \
    | awk '/^__DATE__ /{d=$2; next} NF {print "G\t" d "\t" $0}' || true
  # shellcheck disable=SC2086
  if stat -f '%Sm%t%N' -t '%Y-%m-%d' $FILES 2>/dev/null | awk -F'\t' '{print "M\t" $1 "\t" $2}'; then :; else
    stat -c '%y%t%n' $FILES 2>/dev/null | awk -F'\t' '{print "M\t" substr($1,1,10) "\t" $2}'
  fi
  if [ -f "$INDEX" ]; then
    awk -F'|' '/^## Not yet written/ {exit}
      /^\| *[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9] *\|/ {
        doc=$3; sub(/.*\(/, "", doc); sub(/\).*/, "", doc)
        w=$4; gsub(/^[ \t]+|[ \t]+$/, "", w)
        if (doc != "" && w != "") print "E\t" doc "\t" w }' "$INDEX"
  fi
  printf '%s\n' "$FILES" | awk '{print "F\t" $0}'
} | awk -F'\t' '
  $1=="G" { p=$3; sub(/^docs\//, "", p); if (!(p in gd)) gd[p]=$2; next }
  $1=="M" { fm[$3]=$2; next }
  $1=="E" { ex[$2]=$3; next }
  $1=="F" {
    f=$2
    g=7
    if (f=="STATUS.md") g=0; else if (f=="aha.md") g=1
    else if (f ~ /^plans\//) g=2; else if (f ~ /^design\//) g=3; else if (f ~ /^adr\//) g=4
    else if (f ~ /^reviews\//) g=5; else if (f ~ /^handoffs\//) g=6
    d = (f in gd) ? gd[f] : ((f in fm) ? fm[f] : "0000-00-00")
    if (f in ex) w=ex[f]
    else {
      h1=""; w=""; seen=0
      while ((getline line < f) > 0) {
        if (!seen) { if (line ~ /^# /) { seen=1; h1=line } ; continue }
        if (line ~ /[^ \t]/) { w=line; break }
      }
      close(f)
      if (w=="") w=h1; if (w=="") w=f
      sub(/^[#>*[:space:]-]+/, "", w); gsub(/\|/, "\\|", w)
      if (length(w) > 100) w = substr(w,1,97) "…"
    }
    printf "%d\t%s\t%s\t%s\n", g, d, f, w
  }' | LC_ALL=C sort -t"$(printf '\t')" -k1,1n -k2,2r -k3,3r \
  | awk -F'\t' '$1==6 {if (++n>5) next} {printf "| %s | [%s](%s) | %s |\n", $2, $3, $3, $4}' > "$TMP"

OUT="$(mktemp)"
{
  echo "# docs/ — index"
  echo
  echo "Newest first. One line each. Regenerated on every change under \`docs/\` by \`scripts/dev/gen-index.sh\` (see \`project-plan.md\` §17.3)."
  echo
  echo "| Date | Doc | What it is |"
  echo "|---|---|---|"
  cat "$TMP"
  if [ -f "$INDEX" ] && grep -q '^## Not yet written' "$INDEX"; then
    echo
    sed -n '/^## Not yet written/,$p' "$INDEX"
  fi
} > "$OUT"
rm -f "$TMP"
if [ -f "$INDEX" ] && cmp -s "$OUT" "$INDEX"; then rm -f "$OUT"; else mv "$OUT" "$INDEX"; fi
exit 0
