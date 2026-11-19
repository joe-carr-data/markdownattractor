#!/usr/bin/env bash
# Record demo video 3, "ask, edit, ask again": Claude Code answers a question from the index with
# its source, a second shell edits the runbook, and Claude is asked what changed.
#
#   scripts/demo/make-corpus.py ~/acme-docs && (cd ~/acme-docs && mda index && mda start)   # once
#   scripts/demo/record-live.sh ~/acme-docs out.mp4
#
# Needs: vhs, ffmpeg, tmux, python3 with Pillow, claude (logged in), mda on PATH (or MDA_BIN), a running daemon on DEST
# and cards on it (about $0.30 with the api backend). The session is real, so the model's wording
# and speed differ between takes: watch each take before use, and check that the captions (added
# here, at times that follow the tape's fixed pauses) still sit on the right moments.
# The take changes one phrase in DEST/runbooks/deploy.md and puts it back afterwards, with the
# file time make-corpus.py gave it. Nothing is written into the repository.
set -euo pipefail

DEST="$(cd "${1:?usage: record-live.sh DEST OUT.mp4}" && pwd)"
OUT="${2:?usage: record-live.sh DEST OUT.mp4}"
MDA="${MDA_BIN:-$(command -v mda)}"
MODEL="${DEMO_MODEL:-sonnet}"
HERE="$(cd "$(dirname "$0")" && pwd)"
RUNBOOK="$DEST/runbooks/deploy.md"
BEFORE="for ten minutes"
AFTER="for thirty minutes"
Q1="How long do we watch the error rate after a release? Keep it short."
Q2="Can you double-check?"
EDIT="sed -i '' 's/$BEFORE/$AFTER/' runbooks/deploy.md"

for tool in vhs ffmpeg tmux claude; do
  command -v "$tool" >/dev/null 2>&1 || { echo "record: $tool not found" >&2; exit 1; }
done
[ -x "$MDA" ] || { echo "record: mda not found (set MDA_BIN)" >&2; exit 1; }
grep -q "$BEFORE" "$RUNBOOK" 2>/dev/null \
  || { echo "record: $DEST is not a fresh demo corpus (make-corpus.py)" >&2; exit 1; }
"$MDA" status --root "$DEST" --json | grep -q '"pid": *[0-9]' \
  || { echo "record: no daemon is running on $DEST; run mda start there first" >&2; exit 1; }

work="$(mktemp -d)"
sock="mda-demo-$$"
restore() {
  tmux -L "$sock" kill-server 2>/dev/null || true
  sed -i '' "s/$AFTER/$BEFORE/" "$RUNBOOK"
  touch -t 202611161000 "$RUNBOOK"
  rm -rf "$work"
}
trap restore EXIT
mkdir "$work/bin"
ln -s "$MDA" "$work/bin/mda"

# Only this server, so the take does not depend on whatever else the recorder has installed.
printf '{ "mcpServers": { "markdownattractor": { "command": "%s", "args": ["mcp"], "env": { "MDA_ROOT": "%s" } } } }\n' \
  "$MDA" "$DEST" >"$work/mcp.json"

# Its own tmux server and configuration: nothing of the recorder's shows.
cat >"$work/tmux.conf" <<'EOF'
set -g status off
set -g escape-time 0
set -g focus-events on
set -g default-terminal "tmux-256color"
set -ga terminal-overrides ",*:Tc"
set -g pane-border-status top
set -g pane-border-format "#[bold] #{@label} "
set -g pane-border-style "fg=#6c7086"
set -g pane-active-border-style "fg=#cba6f7"
bind -n C-g select-pane -t :.+
EOF
cat >"$work/start.sh" <<EOF
#!/usr/bin/env bash
cd "$DEST" && clear
tmux -L "$sock" -f "$work/tmux.conf" new-session -d -s demo -c "$DEST" \\
  "env -u CLAUDECODE -u CLAUDE_CODE_CHILD_SESSION -u CLAUDE_CODE_ENTRYPOINT -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY claude --model $MODEL --strict-mcp-config --mcp-config $work/mcp.json --allowedTools 'mcp__markdownattractor__*'"
tmux -L "$sock" set -p -t demo:0.0 @label "Claude Code"
tmux -L "$sock" split-window -t demo:0.0 -v -l 3 -c "$DEST" "env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY PS1='acme-docs %# ' zsh -f"
tmux -L "$sock" set -p -t demo:0.1 @label "a teammate's terminal"
tmux -L "$sock" select-pane -t demo:0.0
exec tmux -L "$sock" attach -t demo
EOF
chmod +x "$work/start.sh"

ffmpeg -loglevel error -y -f lavfi \
  -i "gradients=s=1600x900:c0=0x7c3aed:c1=0x0ea5e9:x0=0:y0=0:x1=1600:y1=900:d=1" \
  -frames:v 1 "$work/bg.png"

sed -e "s|@WORK@|$work|g" -e "s|@Q1@|$Q1|g" -e "s|@Q2@|$Q2|g" -e "s|@EDIT@|$EDIT|g" \
  "$HERE/video3.tape.in" >"$work/video3.tape"
if [ -n "${DEMO_RAW:-}" ]; then          # caption an earlier raw take again instead of recording
  cp "$DEMO_RAW" "$work/out.mp4"
else
  (cd "$work" && vhs video3.tape >/dev/null)
  [ -z "${DEMO_KEEP_RAW:-}" ] || cp "$work/out.mp4" "$DEMO_KEEP_RAW"
fi

# Captions. The times are seconds from the first visible frame and follow the tape: typing at
# 30 ms a character plus its pauses.
t_a1=$(awk -v n="${#Q1}" 'BEGIN{print 1.5 + n*0.03 + 0.5}')                  # Enter on question 1
t_edit=$(awk -v t="$t_a1" 'BEGIN{print t + 10}')                             # focus moves below
t_q2=$(awk -v t="$t_edit" -v n="${#EDIT}" 'BEGIN{print t + 0.7 + n*0.03 + 0.5 + 3}')
t_a2=$(awk -v t="$t_q2" -v n="${#Q2}" 'BEGIN{print t + 0.5 + n*0.03 + 0.5}') # Enter on question 2
t_c5=$(awk -v t="$t_a2" 'BEGIN{print t + 4}')                                # the answer is on screen
t_saved=$(awk -v t="$t_edit" -v n="${#EDIT}" 'BEGIN{print t + 0.7 + n*0.03 + 0.5}') # Enter on the edit
python3 "$HERE/captions.py" "$work" "$work/bg.png" \
  "1 · Ask Claude Code about your team's docs" \
  "The answer comes from the index, with its file and lines" \
  "2 · Meanwhile the runbook changes: a teammate, or Claude itself, saves an edit" \
  "3 · Seconds later, ask Claude to double-check" \
  "Claude finds the edit itself: new text, lines and time. Nobody ran a reindex."
ffmpeg -loglevel error -y -i "$work/out.mp4" \
  -i "$work/cap0.png" -i "$work/cap1.png" -i "$work/cap2.png" -i "$work/cap3.png" -i "$work/cap4.png" -i "$work/edit.png" \
  -filter_complex "\
[0:v][1:v]overlay=enable='between(t,0.2,$t_a1)'[a];\
[a][2:v]overlay=enable='between(t,$t_a1,$t_edit)'[b];\
[b][3:v]overlay=enable='between(t,$t_edit,$t_q2)'[c];\
[c][4:v]overlay=enable='between(t,$t_q2,$t_c5)'[d];\
[d][5:v]overlay=enable='gte(t,$t_c5)'[e];\
[e][6:v]overlay=enable='between(t,$t_saved,$t_c5)'" \
  -c:v libx264 -pix_fmt yuv420p -crf 18 "$work/captioned.mp4"
ffmpeg -loglevel error -y -loop 1 -t 3.5 -r 25 -i "$work/end.png" \
  -c:v libx264 -pix_fmt yuv420p -crf 18 "$work/end.mp4"
ffmpeg -loglevel error -y -loop 1 -t 9 -r 25 -i "$work/tech.png" \
  -c:v libx264 -pix_fmt yuv420p -crf 18 "$work/tech.mp4"
ffmpeg -loglevel error -y -loop 1 -t 9 -r 25 -i "$work/bench.png" \
  -c:v libx264 -pix_fmt yuv420p -crf 18 "$work/bench.mp4"

fps="$(ffprobe -v error -select_streams v:0 -show_entries stream=r_frame_rate -of csv=p=0 "$work/captioned.mp4")"
# The take, the install card (3.5 s), then two cards to pause on (9 s each): how it works, and
# the measured figures.
ffmpeg -loglevel error -y -i "$work/captioned.mp4" -i "$work/end.mp4" -i "$work/tech.mp4" -i "$work/bench.mp4" -filter_complex \
  "[1:v]fps=$fps,format=yuv420p[e];[2:v]fps=$fps,format=yuv420p[t];[3:v]fps=$fps,format=yuv420p[b];[0:v]format=yuv420p[m];[m][e][t][b]concat=n=4:v=1:a=0" \
  -c:v libx264 -pix_fmt yuv420p -crf 18 -movflags +faststart "$OUT"
echo "record: $OUT ($(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT" | cut -d. -f1) s) — watch it before using it"
