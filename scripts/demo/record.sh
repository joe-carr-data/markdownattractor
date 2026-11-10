#!/usr/bin/env bash
# Record demo video 1 as a real Claude Code session on the demo corpus, with VHS.
#
#   scripts/demo/make-corpus.py ~/acme-docs && (cd ~/acme-docs && mda index)   # once
#   scripts/demo/record.sh ~/acme-docs out.mp4 ["question"]
#
# Needs: vhs, ffmpeg, claude (logged in), mda on PATH (or MDA_BIN), an indexed corpus at DEST.
# The session is real: the model's answer differs between takes, so watch each take before use.
# Nothing is written into the repository; the tape and backdrop go to a temporary directory.
set -euo pipefail

DEST="$(cd "${1:?usage: record.sh DEST OUT.mp4 [question]}" && pwd)"
OUT="${2:?usage: record.sh DEST OUT.mp4 [question]}"
QUESTION="${3:-Why did we pick Postgres over MySQL? Three lines, with sources.}"
MDA="${MDA_BIN:-$(command -v mda)}"
HERE="$(cd "$(dirname "$0")" && pwd)"

for tool in vhs ffmpeg claude; do
  command -v "$tool" >/dev/null 2>&1 || { echo "record: $tool not found" >&2; exit 1; }
done
[ -x "$MDA" ] || { echo "record: mda not found (set MDA_BIN)" >&2; exit 1; }
[ -d "$DEST/.markdownattractor" ] || { echo "record: $DEST is not indexed; run mda index there first" >&2; exit 1; }
case "$QUESTION" in *\"*) echo "record: the question must not contain a double quote" >&2; exit 1 ;; esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Only this server, so the take does not depend on whatever else the recorder has installed.
printf '{ "mcpServers": { "markdownattractor": { "command": "%s", "args": ["mcp"], "env": { "MDA_ROOT": "%s" } } } }\n' \
  "$MDA" "$DEST" >"$work/mcp.json"

# The backdrop behind the terminal window: a violet-to-blue gradient.
ffmpeg -loglevel error -y -f lavfi \
  -i "gradients=s=1440x810:c0=0x7c3aed:c1=0x0ea5e9:x0=0:y0=0:x1=1440:y1=810:d=1" \
  -frames:v 1 "$work/bg.png"

sed -e "s|@DEST@|$DEST|g" -e "s|@WORK@|$work|g" -e "s|@QUESTION@|$QUESTION|g" \
  "$HERE/video1.tape.in" >"$work/video1.tape"
(cd "$work" && vhs video1.tape >/dev/null)
mv "$work/out.mp4" "$OUT"
echo "record: $OUT ($(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT" | cut -d. -f1) s) — watch it before using it"
