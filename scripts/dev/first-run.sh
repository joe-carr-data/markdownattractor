#!/usr/bin/env bash
# The first-run gate (plan 2026-11-v0.3, Phase B): from nothing installed to a useful search
# hit, with no credentials, in under two minutes. Scripted and timed.
#
#   scripts/dev/first-run.sh            # what a new user gets: the plugin's own bootstrap
#                                       # hook downloads the release binary for ./VERSION
#   MDA_BIN=target/release/mda scripts/dev/first-run.sh   # a local build instead (says so)
#
# Everything lives in one temporary directory (plugin data, HOME, the demo corpus) and is
# removed at the end. No provider key reaches any child process. Exit 0 only when every
# question's expected section is in the top 5 and the whole run took under LIMIT seconds.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
LIMIT="${LIMIT:-120}"
T="$(mktemp -d "${TMPDIR:-/tmp}/mda-first-run.XXXXXX")"
MDA=""
cleanup() {
  [ -n "$MDA" ] && run "$MDA" stop --root "$T/corpus" >/dev/null 2>&1 || true
  rm -rf "$T"
}
trap cleanup EXIT
# A clean user: empty HOME and configuration, no keys.
run() {
  env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u ANTHROPIC_WORKSPACE_ID \
    HOME="$T/home" XDG_CONFIG_HOME="$T/home/.config" XDG_CACHE_HOME="$T/home/.cache" \
    MDA_MODEL_DIR="$T/data/models" NO_COLOR=1 "$@"
}
mkdir -p "$T/home" "$T/data"
python3 "$REPO/scripts/demo/make-corpus.py" "$T/corpus" >/dev/null

t0="$(date +%s)"
if [ -n "${MDA_BIN:-}" ]; then
  MDA="$(cd "$(dirname "$MDA_BIN")" && pwd)/$(basename "$MDA_BIN")"
  how="a local build ($MDA_BIN); the download step is not timed"
else
  # The SessionStart hook, as a marketplace install runs it: download, checksum, install.
  run env CLAUDE_PLUGIN_ROOT="$REPO" CLAUDE_PLUGIN_DATA="$T/data" CLAUDE_PROJECT_DIR="$T/corpus" \
    bash "$REPO/scripts/bootstrap.sh"
  MDA="$T/data/bin/mda"
  how="the release binary for v$(tr -d '[:space:]' < "$REPO/VERSION"), downloaded by the bootstrap hook"
fi
[ -x "$MDA" ] || { echo "first-run: no binary at $MDA" >&2; exit 1; }
t_bin="$(date +%s)"

run "$MDA" start --no-example --root "$T/corpus" > "$T/start.out"
cat "$T/start.out"
if grep -qiE 'warning|error' "$T/start.out"; then
  echo "first-run: FAIL — start words the missing key as a fault" >&2; exit 1
fi

# question | the file whose section must be among the first five hits
ok=0; n=0; t_hit=""
while IFS='|' read -r q want; do
  n=$((n + 1))
  found=""
  for _ in $(seq 1 100); do   # the daemon's first scan takes a moment
    if run "$MDA" --json search "$q" --root "$T/corpus" -k 5 2>/dev/null \
      | python3 -c 'import json,sys
try: hits = json.load(sys.stdin)["hits"]
except ValueError: sys.exit(1)
sys.exit(0 if any(h["rel_path"] == sys.argv[1] for h in hits) else 1)' "$want"; then
      found=1; break
    fi
    [ "$n" -gt 1 ] && break
    sleep 0.2
  done
  [ -n "$found" ] && ok=$((ok + 1))
  [ -z "$t_hit" ] && [ -n "$found" ] && t_hit="$(date +%s)"
  printf '  %s  %s → %s\n' "$([ -n "$found" ] && echo hit || echo MISS)" "$q" "$want"
done <<'Q'
how do we roll back a deploy|runbooks/deploy.md
why did we choose PostgreSQL|adr/0001-postgres-over-mysql.md
who do I page for an outage|onboarding/faq.md
Q
t1="$(date +%s)"
total=$((t1 - t0))
echo "first-run: $how"
echo "first-run: binary ready after $((t_bin - t0)) s · first hit after $(( ${t_hit:-$t1} - t0 )) s · $ok of $n questions hit in the top 5 · $total s in all (limit $LIMIT s) · no credentials"
[ "$ok" = "$n" ] && [ "$total" -lt "$LIMIT" ] || { echo "first-run: FAIL" >&2; exit 1; }
echo "first-run: PASS"
