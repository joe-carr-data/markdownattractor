#!/usr/bin/env bash
# SessionStart hook: make sure the `mda` binary for this plugin version is present in
# ${CLAUDE_PLUGIN_DATA}/bin. Never blocks the session: any failure prints one line and exits 0.
#
# Resolution order:
#   1. MDA_BIN env var (developer override)
#   2. ${CLAUDE_PLUGIN_DATA}/bin/mda-<version> reporting that version   (fast path, < 20 ms)
#   3. ${CLAUDE_PLUGIN_ROOT}/target/release/mda                (contributors using --plugin-dir)
#   4. download the prebuilt archive for this OS/arch from GitHub Releases, verify SHA-256
#
# The binary's file name carries the plugin version. Sessions that still run an older copy of
# the plugin (every open session keeps the copy it started with, and runs this hook again
# after each compaction) therefore install and run their own file and can never put an older
# binary in the place of a newer one. Up to 0.2.6 all versions shared `bin/mda`; that file is
# left alone for the sessions that still use it.
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -u

ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
# CLAUDE_PLUGIN_DATA is trusted only when it is this plugin's own directory. A shell started
# by Claude Code's Bash tool can carry another plugin's value (seen: the codex plugin's), and
# the binary and the models would then be looked for, and downloaded, in that plugin's folder.
DATA="$HOME/.claude/plugins/data/markdownattractor-markdownattractor"
case "$(basename "${CLAUDE_PLUGIN_DATA:-}")" in
  markdownattractor*) DATA="$CLAUDE_PLUGIN_DATA" ;;   # the folder's own name, not an ancestor's
esac
# The version becomes part of a file name: first line only, and nothing but a release number.
WANTED="$(head -n 1 "$ROOT/VERSION" 2>/dev/null | tr -d '[:space:]')"
case "$WANTED" in ''|*[!0-9A-Za-z.+-]*) WANTED=unknown ;; esac
BIN="$DATA/bin/mda-$WANTED"
REPO="joe-carr-data/markdownattractor"

say() { printf 'markdownattractor: %s\n' "$*"; }

version_of() { "$1" --version 2>/dev/null | awk '{print $2}'; }

# When this project was indexed before, make sure its daemon is up. `mda start` is idempotent
# and returns in well under a second when the daemon already answers. Never blocks the session.
ensure_daemon() {
  if [ "$1" = "$BIN" ]; then
    # Mark this version as in use, and drop other versions' binaries nothing has used for two
    # weeks (an archive keeps its build date, hence the mark).
    touch "$BIN" 2>/dev/null
    for other in "$DATA"/bin/mda-[0-9]*; do
      # Only files this hook installs (mda-<release number>), never this version's.
      case "${other##*/mda-}" in *[!0-9A-Za-z.+-]*) continue ;; esac
      [ "$other" != "$BIN" ] && [ -f "$other" ] && [ -n "$(find "$other" -mtime +14 2>/dev/null)" ] && rm -f "$other"
    done
  fi
  project="${CLAUDE_PROJECT_DIR:-$PWD}"
  # The launcher calls this hook to fetch a missing binary; it starts no daemon from there.
  # An index, not only the folder: `mda nudge off` alone creates the folder with a config.
  if [ "${MDA_NO_DAEMON:-0}" != "1" ] && [ -f "$project/.markdownattractor/index.sqlite" ]; then
    # Detached: the hook returns at once whatever `start` has to wait for.
    ( MDA_MODEL_DIR="${MDA_MODEL_DIR:-$DATA/models}" "$1" start --no-example --root "$project" >/dev/null 2>&1 </dev/null & ) 2>/dev/null
  fi
  exit 0
}

# 1. explicit override
if [ -n "${MDA_BIN:-}" ] && [ -x "$MDA_BIN" ]; then
  ensure_daemon "$MDA_BIN"
fi

if [ "$WANTED" = unknown ]; then
  say "cannot read the plugin's VERSION file in $ROOT — reinstall the plugin"
  exit 0
fi

# 2. fast path
if [ -x "$BIN" ] && [ "$(version_of "$BIN")" = "$WANTED" ]; then
  ensure_daemon "$BIN"
fi

mkdir -p "$DATA/bin" 2>/dev/null || { say "cannot create $DATA/bin — run /mda doctor"; exit 0; }

# 3. local build (contributors)
if [ -x "$ROOT/target/release/mda" ] && [ "$(version_of "$ROOT/target/release/mda")" = "$WANTED" ]; then
  cp "$ROOT/target/release/mda" "$BIN" && chmod +x "$BIN" && ensure_daemon "$BIN"
fi

# 4. download
case "$(uname -s)" in
  Darwin) os=darwin ;;
  Linux) os=linux ;;
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  *) say "unsupported OS $(uname -s) — build with: cargo install --git https://github.com/$REPO mda-cli"; exit 0 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=arm64 ;;
  x86_64|amd64) arch=x64 ;;
  *) say "unsupported arch $(uname -m) — build with: cargo install --git https://github.com/$REPO mda-cli"; exit 0 ;;
esac

target="mda-${os}-${arch}"
base="https://github.com/$REPO/releases/download/v${WANTED}"
tmp="$(mktemp -d 2>/dev/null || echo "/tmp/mda-bootstrap.$$")"
trap 'rm -rf "$tmp"' EXIT

if ! command -v curl >/dev/null 2>&1; then
  say "curl not found; put the release binary for v$WANTED at $BIN (that exact name)"
  exit 0
fi

if ! curl -fsSL --connect-timeout 10 --max-time 120 -o "$tmp/$target.tar.gz" "$base/$target.tar.gz" \
   || ! curl -fsSL --connect-timeout 10 --max-time 30 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"; then
  say "no prebuilt binary for v$WANTED ($target) could be downloaded — to install by hand, put it at $BIN (that exact name)"
  exit 0
fi

expected="$(grep " $target.tar.gz\$" "$tmp/SHA256SUMS" | awk '{print $1}')"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$target.tar.gz" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "$tmp/$target.tar.gz" | awk '{print $1}')"
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  say "checksum mismatch for $target.tar.gz — refusing to install; run /mda doctor"
  exit 0
fi

# Windows archives carry mda.exe; the launcher runs it under the same name without the suffix.
extracted="$tmp/mda"
[ "$os" = windows ] && extracted="$tmp/mda.exe"
if tar -xzf "$tmp/$target.tar.gz" -C "$tmp" && [ -f "$extracted" ]; then
  # Checked before it gets its name: a binary of another version is never installed.
  chmod +x "$extracted"
  got="$(version_of "$extracted")"
  if [ "$got" = "$WANTED" ]; then
    mv "$extracted" "$BIN" && ensure_daemon "$BIN"
  fi
  say "the downloaded binary reports ${got:-nothing}, expected $WANTED — not installed; run /mda doctor"
else
  say "could not extract $target.tar.gz — run /mda doctor"
fi
exit 0
