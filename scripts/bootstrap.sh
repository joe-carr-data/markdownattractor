#!/usr/bin/env bash
# SessionStart hook: make sure the `mda` binary for this plugin version is present in
# ${CLAUDE_PLUGIN_DATA}/bin. Never blocks the session: any failure prints one line and exits 0.
#
# Resolution order:
#   1. MDA_BIN env var (developer override)
#   2. ${CLAUDE_PLUGIN_DATA}/bin/mda with a matching version   (fast path, < 20 ms)
#   3. ${CLAUDE_PLUGIN_ROOT}/target/release/mda                (contributors using --plugin-dir)
#   4. download the prebuilt archive for this OS/arch from GitHub Releases, verify SHA-256
[ "${MARKDOWNATTRACTOR_WORKER:-0}" = "1" ] && exit 0
set -u

ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
DATA="${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugins/data/markdownattractor-markdownattractor}"
WANTED="$(tr -d '[:space:]' < "$ROOT/VERSION" 2>/dev/null || echo unknown)"
BIN="$DATA/bin/mda"
REPO="joe-carr-data/markdownattractor"

say() { printf 'markdownattractor: %s\n' "$*"; }

version_of() { "$1" --version 2>/dev/null | awk '{print $2}'; }

# When this project was indexed before, make sure its daemon is up. `mda start` is idempotent
# and returns in well under a second when the daemon already answers. Never blocks the session.
ensure_daemon() {
  project="${CLAUDE_PROJECT_DIR:-$PWD}"
  if [ -d "$project/.markdownattractor" ]; then
    # Detached: the hook returns at once whatever `start` has to wait for.
    ( MDA_MODEL_DIR="${MDA_MODEL_DIR:-$DATA/models}" "$1" start --no-example --root "$project" >/dev/null 2>&1 </dev/null & ) 2>/dev/null
  fi
  exit 0
}

# 1. explicit override
if [ -n "${MDA_BIN:-}" ] && [ -x "$MDA_BIN" ]; then
  ensure_daemon "$MDA_BIN"
fi

# 2. fast path
if [ -x "$BIN" ] && [ "$(version_of "$BIN")" = "$WANTED" ]; then
  ensure_daemon "$BIN"
fi

mkdir -p "$DATA/bin" 2>/dev/null || { say "cannot create $DATA/bin — run /mda doctor"; exit 0; }

# 3. local build (contributors)
if [ -x "$ROOT/target/release/mda" ]; then
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
  say "curl not found; install the binary manually into $DATA/bin (see /mda doctor)"
  exit 0
fi

if ! curl -fsSL --connect-timeout 10 --max-time 120 -o "$tmp/$target.tar.gz" "$base/$target.tar.gz" \
   || ! curl -fsSL --connect-timeout 10 --max-time 30 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"; then
  say "no prebuilt binary for v$WANTED ($target) yet — run /mda doctor for manual steps"
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
  mv "$extracted" "$BIN" && chmod +x "$BIN"
  [ "$(version_of "$BIN")" = "$WANTED" ] && ensure_daemon "$BIN"
  say "installed binary reports $(version_of "$BIN"), expected $WANTED — run /mda doctor"
else
  say "could not extract $target.tar.gz — run /mda doctor"
fi
exit 0
