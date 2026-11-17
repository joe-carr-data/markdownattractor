#!/usr/bin/env bash
# The first-run gate (plan 2026-11-v0.3, Phase B): from nothing installed to a useful search
# hit, with no credentials, in under two minutes. Scripted and timed.
#
#   scripts/dev/first-run.sh            # what a new user gets: the plugin's own bootstrap
#                                       # hook downloads the release binary for ./VERSION
#   MDA_BIN=target/release/mda scripts/dev/first-run.sh   # a local build instead (says so)
#
# Everything lives in one temporary directory (plugin data, HOME, the demo corpus) and is
# removed at the end. Every child process runs in an emptied environment (`env -i` plus PATH
# and the temporary HOME), so no credential of the caller reaches it. Exit 0 only when every
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
  env -i PATH="$PATH" TMPDIR="$T/tmp" LANG="${LANG:-C}" \
    HOME="$T/home" XDG_CONFIG_HOME="$T/home/.config" XDG_CACHE_HOME="$T/home/.cache" \
    MDA_MODEL_DIR="$T/markdownattractor-data/models" NO_COLOR=1 "$@"
}
mkdir -p "$T/home" "$T/markdownattractor-data" "$T/tmp" "$T/plugin/scripts"
run python3 "$REPO/scripts/demo/make-corpus.py" "$T/corpus" >/dev/null
WANTED="$(tr -d '[:space:]' < "$REPO/VERSION")"

t0="$(date +%s)"
if [ -n "${MDA_BIN:-}" ]; then
  MDA="$(cd "$(dirname "$MDA_BIN")" && pwd)/$(basename "$MDA_BIN")"
  how="a local build ($MDA_BIN); the download step is not timed"
else
  # The SessionStart hook, as a marketplace install runs it: download, checksum, install.
  # It runs from a plugin layout that holds only the hook and VERSION, so its fallback to a
  # local `target/release/mda` cannot stand in for the download.
  cp "$REPO/scripts/bootstrap.sh" "$T/plugin/scripts/"; cp "$REPO/VERSION" "$T/plugin/"
  run env CLAUDE_PLUGIN_ROOT="$T/plugin" CLAUDE_PLUGIN_DATA="$T/markdownattractor-data" CLAUDE_PROJECT_DIR="$T/corpus" \
    bash "$T/plugin/scripts/bootstrap.sh"
  MDA="$T/markdownattractor-data/bin/mda"
  how="the release binary for v$WANTED, downloaded by the bootstrap hook"
  [ -x "$MDA" ] || { echo "first-run: FAIL — the hook installed no binary (is v$WANTED released?)" >&2; exit 1; }
  got="$(run "$MDA" --version | awk '{print $2}')"
  [ "$got" = "$WANTED" ] || { echo "first-run: FAIL — installed $got, wanted $WANTED" >&2; exit 1; }
fi
[ -x "$MDA" ] || { echo "first-run: no binary at $MDA" >&2; exit 1; }
t_bin="$(date +%s)"

run "$MDA" start --no-example --root "$T/corpus" > "$T/start.out"
cat "$T/start.out"
if grep -qiE 'warning|error' "$T/start.out"; then
  echo "first-run: FAIL — start words the missing key as a fault" >&2; exit 1
fi

# question | file | heading: that section must be among the first five hits
ok=0; n=0; t_hit=""
while IFS='|' read -r q want head; do
  n=$((n + 1))
  found=""
  for _ in $(seq 1 100); do   # the daemon's first scan takes a moment
    if run "$MDA" --json search "$q" --root "$T/corpus" -k 5 2>/dev/null \
      | run python3 -c 'import json,sys
try: hits = json.load(sys.stdin)["hits"]
except ValueError: sys.exit(1)
sys.exit(0 if any(h["rel_path"] == sys.argv[1] and (h["heading_path"] or [""])[-1] == sys.argv[2] for h in hits) else 1)' "$want" "$head"; then
      found=1; break
    fi
    [ "$n" -gt 1 ] && break
    sleep 0.2
  done
  [ -n "$found" ] && ok=$((ok + 1))
  [ -z "$t_hit" ] && [ -n "$found" ] && t_hit="$(date +%s)"
  printf '  %s  %s → %s\n' "$([ -n "$found" ] && echo hit || echo MISS)" "$q" "$want › $head"
done <<'Q'
how do we roll back a deploy|runbooks/deploy.md|Rollback
why did we choose PostgreSQL|adr/0001-postgres-over-mysql.md|Decision
who do I page for an outage|onboarding/faq.md|Who do I page?
Q
t1="$(date +%s)"
total=$((t1 - t0))
echo "first-run: $how"
echo "first-run: binary ready after $((t_bin - t0)) s · first hit after $(( ${t_hit:-$t1} - t0 )) s · $ok of $n questions hit in the top 5 · $total s in all (limit $LIMIT s) · no credentials"
[ "$ok" = "$n" ] && [ "$total" -lt "$LIMIT" ] || { echo "first-run: FAIL" >&2; exit 1; }
echo "first-run: PASS"
