#!/usr/bin/env bash
# Record demo video 2: a section is appended to a file and a search finds it seconds later.
#
#   scripts/demo/make-corpus.py ~/acme-docs && (cd ~/acme-docs && mda start)   # once; no key needed
#   scripts/demo/record-fresh.sh ~/acme-docs out.mp4
#
# Needs: vhs, ffmpeg, mda on PATH (or MDA_BIN), a running daemon on DEST. The take appends a
# "Canary" section to DEST/runbooks/deploy.md, so DEST is a throwaway copy made by make-corpus.py;
# the script refuses a corpus that already has the section. Nothing is written into the repository.
set -euo pipefail

DEST="$(cd "${1:?usage: record-fresh.sh DEST OUT.mp4}" && pwd)"
OUT="${2:?usage: record-fresh.sh DEST OUT.mp4}"
MDA="${MDA_BIN:-$(command -v mda)}"
HERE="$(cd "$(dirname "$0")" && pwd)"

for tool in vhs ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || { echo "record: $tool not found" >&2; exit 1; }
done
[ -x "$MDA" ] || { echo "record: mda not found (set MDA_BIN)" >&2; exit 1; }
[ -f "$DEST/runbooks/deploy.md" ] || { echo "record: $DEST is not the demo corpus (make-corpus.py)" >&2; exit 1; }
if grep -q '^## Canary' "$DEST/runbooks/deploy.md"; then
  echo "record: $DEST already has the Canary section; make a fresh corpus" >&2; exit 1
fi
"$MDA" status --root "$DEST" --json | grep -q '"pid": *[0-9]' \
  || { echo "record: no daemon is running on $DEST; run mda start there first" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/bin"
ln -s "$MDA" "$work/bin/mda"   # the command on screen is `mda`, whatever the binary's file name

ffmpeg -loglevel error -y -f lavfi \
  -i "gradients=s=1440x810:c0=0x7c3aed:c1=0x0ea5e9:x0=0:y0=0:x1=1440:y1=810:d=1" \
  -frames:v 1 "$work/bg.png"

sed -e "s|@DEST@|$DEST|g" -e "s|@BIN@|$work/bin|g" "$HERE/video2.tape.in" >"$work/video2.tape"
(cd "$work" && vhs video2.tape >/dev/null)
mv "$work/out.mp4" "$OUT"
echo "record: $OUT ($(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT" | cut -d. -f1) s) — watch it before using it"
