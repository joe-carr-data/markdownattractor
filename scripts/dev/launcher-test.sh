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
# A folder named like the URL's release (v1.2.3) is served when it exists.
d="$STUB_SERVE/$(basename "$(dirname "$url")")"; [ -d "$d" ] || d="$STUB_SERVE"
cp "$d/$(basename "$url")" "$out"
STUB
chmod +x "$T/stub/curl"

run() { # run <data-dir> <extra env…> -- <launcher args…>
  local data="$1"; shift; local envs=()
  while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
  env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" STUB_SERVE="$T/serve" \
    CLAUDE_PLUGIN_ROOT="$T/plugin" CLAUDE_PLUGIN_DATA="$data" CLAUDE_PROJECT_DIR="$T/home" \
    ${envs[@]+"${envs[@]}"} "$T/plugin/scripts/mda" "$@"
}
ver="$(tr -d '[:space:]' < "$REPO/VERSION")"; want="mda $ver"

# 1. Missing binary: fetched once, stdout holds only the command's own output.
out="$(run "$T/markdownattractor-1" -- --version 2>"$T/err1")" || fail "launcher failed with a fetchable binary: $(cat "$T/err1")"
[ "$out" = "$want" ] || fail "stdout was '$out', wanted exactly '$want'"
[ -x "$T/markdownattractor-1/bin/mda-$ver" ] || fail "the binary was not installed"
grep -q "fetching it once" "$T/err1" || fail "no note on stderr about the fetch"
# 2. Second call: the fast path, silent.
out="$(run "$T/markdownattractor-1" -- --version 2>"$T/err2")"; if [ "$out" != "$want" ] || [ -s "$T/err2" ]; then fail "second call was not silent"; fi
# 3. The fetch fails: exit 127 and a hint, nothing on stdout.
set +e; out="$(run "$T/markdownattractor-2" STUB_CURL_FAIL=1 -- --version 2>"$T/err3")"; code=$?; set -e
if [ "$code" != 127 ] || [ -n "$out" ]; then fail "failed fetch: exit $code, stdout '$out'"; fi
grep -q "could not be fetched" "$T/err3" || fail "no hint after a failed fetch"
# 4. MDA_NO_FETCH=1: no attempt.
set +e; run "$T/markdownattractor-3" MDA_NO_FETCH=1 -- --version >/dev/null 2>"$T/err4"; code=$?; set -e
if [ "$code" != 127 ] || grep -q "fetching it once" "$T/err4"; then fail "MDA_NO_FETCH did not stop the fetch"; fi
# 5. The fetch starts no daemon, even in an indexed project.
mkdir -p "$T/home/.markdownattractor"
run "$T/markdownattractor-4" -- --version >/dev/null 2>&1; sleep 1
if [ -e "$T/home/.markdownattractor/daemon.sock" ] || pgrep -f "$T/markdownattractor-4/bin/mda-$ver daemon" >/dev/null; then fail "the fetch started a daemon"; fi
# 6. Another plugin's data directory in CLAUDE_PLUGIN_DATA is ignored: the launcher uses the
#    default directory under HOME and writes nothing into the foreign one.
mkdir -p "$T/data/codex-openai-codex"
run "$T/data/codex-openai-codex" -- --version >/dev/null 2>"$T/err6" || fail "launcher failed with a foreign data dir: $(cat "$T/err6")"
[ -x "$T/home/.claude/plugins/data/markdownattractor-markdownattractor/bin/mda-$ver" ] || fail "the binary was not installed in the plugin's own directory"
if [ -e "$T/data/codex-openai-codex/bin" ]; then fail "the launcher wrote into another plugin's data directory"; fi
# 7. A binary another plugin version left under the old shared name is neither run nor replaced:
#    an older session's hook (which installs `bin/mda`) cannot change what this version runs.
mkdir -p "$T/markdownattractor-7/bin"; printf '#!/bin/sh\necho "mda 0.0.1"\n' > "$T/markdownattractor-7/bin/mda"; chmod +x "$T/markdownattractor-7/bin/mda"
out="$(run "$T/markdownattractor-7" -- --version 2>/dev/null)"; [ "$out" = "$want" ] || fail "ran another version's binary: '$out'"
[ "$("$T/markdownattractor-7/bin/mda" --version)" = "mda 0.0.1" ] || fail "the other version's binary was replaced"
# 8. The hook of a plugin copy at another version installs its own file and leaves this one.
#    Its release holds a binary that reports that version (a script here).
hook() { # hook <plugin-root> <data-dir>
  env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" STUB_SERVE="$T/serve" CLAUDE_PLUGIN_ROOT="$1" CLAUDE_PLUGIN_DATA="$2" MDA_NO_DAEMON=1 bash "$1/scripts/bootstrap.sh" 2>&1 || true
}
mkdir -p "$T/old/scripts" "$T/serve/v0.0.2" "$T/pack"; cp "$REPO/scripts/bootstrap.sh" "$REPO/scripts/mda" "$T/old/scripts/"; echo 0.0.2 > "$T/old/VERSION"
printf '#!/bin/sh\necho "mda 0.0.2"\n' > "$T/pack/mda"; chmod +x "$T/pack/mda"; tar -czf "$T/serve/v0.0.2/$target.tar.gz" -C "$T/pack" mda
if command -v sha256sum >/dev/null 2>&1; then s2="$(sha256sum "$T/serve/v0.0.2/$target.tar.gz" | awk '{print $1}')"; else s2="$(shasum -a 256 "$T/serve/v0.0.2/$target.tar.gz" | awk '{print $1}')"; fi
echo "$s2  $target.tar.gz" > "$T/serve/v0.0.2/SHA256SUMS"
before="$(shasum "$T/markdownattractor-7/bin/mda-$ver" | awk '{print $1}')"
hook "$T/old" "$T/markdownattractor-7" >/dev/null
[ "$("$T/markdownattractor-7/bin/mda-0.0.2" --version 2>/dev/null)" = "mda 0.0.2" ] || fail "the other version's hook installed nothing of its own"
[ "$(shasum "$T/markdownattractor-7/bin/mda-$ver" | awk '{print $1}')" = "$before" ] || fail "another version's hook touched this version's binary"
out="$(run "$T/markdownattractor-7" -- --version 2>/dev/null)"; [ "$out" = "$want" ] || fail "after the other hook: '$out'"
out="$(env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" CLAUDE_PLUGIN_ROOT="$T/old" CLAUDE_PLUGIN_DATA="$T/markdownattractor-7" "$T/old/scripts/mda" --version 2>/dev/null)"
[ "$out" = "mda 0.0.2" ] || fail "the other version's launcher ran '$out'"
# 9. A release whose binary reports another version is not installed under this version's name.
mkdir -p "$T/bad/scripts"; cp "$REPO/scripts/bootstrap.sh" "$T/bad/scripts/"; echo 0.0.3 > "$T/bad/VERSION"
said="$(hook "$T/bad" "$T/markdownattractor-7")"
[ ! -e "$T/markdownattractor-7/bin/mda-0.0.3" ] || fail "a binary of another version was installed as 0.0.3"
grep -q "not installed" <<<"$said" || fail "no word about the refused binary: $said"
# 10. Pruning removes only release binaries unused for two weeks: a file the hook did not install,
#     this version's own file and a version used since all stay. A launch counts as use.
old_stamp() { touch -t 202001010000 "$1"; }
cp "$T/pack/mda" "$T/markdownattractor-7/bin/mda-0.0.1"; cp "$T/pack/mda" "$T/markdownattractor-7/bin/mda-backup"; cp "$T/pack/mda" "$T/markdownattractor-7/bin/mda-0.0.4"
for f in mda-0.0.1 mda-backup mda-0.0.2 mda-0.0.4 "mda-$ver"; do old_stamp "$T/markdownattractor-7/bin/$f"; done
env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" CLAUDE_PLUGIN_ROOT="$T/old" CLAUDE_PLUGIN_DATA="$T/markdownattractor-7" "$T/old/scripts/mda" --version >/dev/null 2>&1   # 0.0.2 is used
cp "$REPO/scripts/bootstrap.sh" "$T/plugin/scripts/"; hook "$T/plugin" "$T/markdownattractor-7" >/dev/null
[ ! -e "$T/markdownattractor-7/bin/mda-0.0.1" ] && [ ! -e "$T/markdownattractor-7/bin/mda-0.0.4" ] || fail "unused old versions were kept"
for f in mda-backup mda-0.0.2 "mda-$ver" mda; do [ -e "$T/markdownattractor-7/bin/$f" ] || fail "pruning removed $f"; done
# 11. A data directory that only sits below a folder named for the plugin is not this plugin's.
mkdir -p "$T/markdownattractor-11/other-plugin"
run "$T/markdownattractor-11/other-plugin" -- --version >/dev/null 2>&1 || fail "launcher failed with a nested foreign data dir"
[ ! -e "$T/markdownattractor-11/other-plugin/bin" ] || fail "the launcher wrote below a folder that is only named like the plugin's"
# 12. A VERSION file that is not a release number names no file and fetches nothing.
mkdir -p "$T/odd/scripts"; cp "$REPO/scripts/bootstrap.sh" "$REPO/scripts/mda" "$T/odd/scripts/"; printf '../../x\n' > "$T/odd/VERSION"
said="$(hook "$T/odd" "$T/markdownattractor-12")"
grep -q "VERSION" <<<"$said" || fail "no word about an unreadable VERSION: $said"
[ ! -e "$T/markdownattractor-12" ] || fail "an odd VERSION created files"
# 13. The hook starts a daemon only where an index exists, not for a folder with a config alone.
mkdir -p "$T/proj/.markdownattractor"; echo 'nudge = false' > "$T/proj/.markdownattractor/config.toml"
env -i HOME="$T/home" PATH="$T/stub:/usr/bin:/bin" CLAUDE_PLUGIN_ROOT="$T/plugin" CLAUDE_PLUGIN_DATA="$T/markdownattractor-7" CLAUDE_PROJECT_DIR="$T/proj" bash "$T/plugin/scripts/bootstrap.sh" >/dev/null 2>&1; sleep 1
[ ! -e "$T/proj/.markdownattractor/index.sqlite" ] || fail "the hook started an index in a folder that had only a config"
echo "launcher-test: a missing binary is fetched once with a clean stdout; a failed fetch exits 127 with a hint; no daemon is started; another plugin's data directory is never used; another plugin version's binary is never run or replaced; a binary of the wrong version is not installed; pruning removes only unused release binaries; an odd VERSION and a config-only folder do nothing"
