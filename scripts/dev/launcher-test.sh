#!/usr/bin/env bash
# Tests for the plugin launcher (scripts/mda) when the binary is missing: it fetches it once
# through bootstrap.sh, keeps stdout clean (stdout may be an MCP connection), and fails with a
# hint when the fetch fails. Offline: `curl` is a stub that serves a local archive.
#   scripts/dev/launcher-test.sh target/debug/mda
set -euo pipefail
BIN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fail() { echo "launcher-test: FAIL: $*" >&2; exit 1; }

# A plugin layout with no build tree, as an installed plugin has.
mkdir -p "$T/plugin/scripts" "$T/serve" "$T/stub" "$T/home"
cp "$REPO/scripts/mda" "$REPO/scripts/bootstrap.sh" "$T/plugin/scripts/"
cp "$REPO/VERSION" "$T/plugin/VERSION"

case "$(uname -s)" in Darwin) os=darwin ;; *) os=linux ;; esac
case "$(uname -m)" in arm64|aarch64) arch=arm64 ;; *) arch=x64 ;; esac
target="mda-$os-$arch"
cp "$BIN" "$T/serve/mda" && tar -czf "$T/serve/$target.tar.gz" -C "$T/serve" mda && rm "$T/serve/mda"
if command -v sha256sum >/dev/null 2>&1; then sum="$(sha256sum "$T/serve/$target.tar.gz" | awk '{print $1}')"
else sum="$(shasum -a 256 "$T/serve/$target.tar.gz" | awk '{print $1}')"; fi
echo "$sum  $target.tar.gz" > "$T/serve/SHA256SUMS"

# The stub: `curl … -o <out> <url>` copies the file named by the URL's last component.
cat > "$T/stub/curl" <<'STUB'
#!/usr/bin/env bash
[ "${STUB_CURL_FAIL:-0}" = 1 ] && exit 22
out=""; url=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift 2 ;; --connect-timeout|--max-time) shift 2 ;; -*) shift ;; *) url="$1"; shift ;; esac; done
cp "$STUB_SERVE/$(basename "$url")" "$out"
STUB
chmod +x "$T/stub/curl"

run() { # run <data-dir> <extra env…> -- <launcher args…>
  local data="$1"; shift; local envs=()
  while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
  env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" STUB_SERVE="$T/serve" \
    CLAUDE_PLUGIN_ROOT="$T/plugin" CLAUDE_PLUGIN_DATA="$data" CLAUDE_PROJECT_DIR="$T/home" \
    ${envs[@]+"${envs[@]}"} "$T/plugin/scripts/mda" "$@"
}
want="mda $(tr -d '[:space:]' < "$REPO/VERSION")"

# 1. Missing binary: fetched once, stdout holds only the command's own output.
out="$(run "$T/data1" -- --version 2>"$T/err1")" || fail "launcher failed with a fetchable binary: $(cat "$T/err1")"
[ "$out" = "$want" ] || fail "stdout was '$out', wanted exactly '$want'"
[ -x "$T/data1/bin/mda" ] || fail "the binary was not installed"
grep -q "fetching it once" "$T/err1" || fail "no note on stderr about the fetch"
# 2. Second call: the fast path, silent.
out="$(run "$T/data1" -- --version 2>"$T/err2")"; if [ "$out" != "$want" ] || [ -s "$T/err2" ]; then fail "second call was not silent"; fi
# 3. The fetch fails: exit 127 and a hint, nothing on stdout.
set +e; out="$(run "$T/data2" STUB_CURL_FAIL=1 -- --version 2>"$T/err3")"; code=$?; set -e
if [ "$code" != 127 ] || [ -n "$out" ]; then fail "failed fetch: exit $code, stdout '$out'"; fi
grep -q "could not be fetched" "$T/err3" || fail "no hint after a failed fetch"
# 4. MDA_NO_FETCH=1: no attempt.
set +e; run "$T/data3" MDA_NO_FETCH=1 -- --version >/dev/null 2>"$T/err4"; code=$?; set -e
if [ "$code" != 127 ] || grep -q "fetching it once" "$T/err4"; then fail "MDA_NO_FETCH did not stop the fetch"; fi
# 5. The fetch starts no daemon, even in an indexed project.
mkdir -p "$T/home/.markdownattractor"
run "$T/data4" -- --version >/dev/null 2>&1; sleep 1
if [ -e "$T/home/.markdownattractor/daemon.sock" ] || pgrep -f "$T/data4/bin/mda daemon" >/dev/null; then fail "the fetch started a daemon"; fi
echo "launcher-test: a missing binary is fetched once with a clean stdout; a failed fetch exits 127 with a hint; no daemon is started"
