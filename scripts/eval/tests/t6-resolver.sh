#!/usr/bin/env bash
# The T6 citation resolver (scripts/eval/t6_resolve.py; M9 plan, Codex pre-mortem F6): every
# resolution is contained in the corpus copy and a member of the frozen page inventory;
# traversal, symlinks, stores, arm artifacts, ambiguous suffixes and unknown pages never
# resolve. Run by `make check` (target t6-tests).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; R="$here/../t6_resolve.py"
tmp="$(mktemp -d -t mda-t6-resolve.XXXXXX)"; trap 'rm -rf "$tmp"' EXIT
C="$tmp/copy"; mkdir -p "$C/a/b" "$C/c" "$C/.markdownattractor" "$C/graphify-out" "$tmp/outside"
printf 'x' > "$C/a/b/index.md"; printf 'x' > "$C/c/index.md"; printf 'x' > "$C/a/README.md"; printf 'x' > "$C/c/odd name (v2).md"
printf 'x' > "$C/.markdownattractor/note.md"; printf 'x' > "$C/graphify-out/graph.md"; printf 'x' > "$tmp/outside/secret.md"
ln -s "$tmp/outside/secret.md" "$C/c/link.md"; ln -s "$tmp/outside" "$C/c/dir"
printf '%s\n' "a/b/index.md" "c/index.md" "a/README.md" "c/odd name (v2).md" "c/link.md" "c/dir/secret.md" > "$tmp/pages.txt"
n=0; fail() { echo "FAIL: $*" >&2; exit 1; }
r() { python3 -I "$R" "$C" "$tmp/pages.txt" proj "$1"; }
expect() { local got; got="$(r "$1")"; [ "$got" = "$2" ] || fail "$1 → '$got' (expected '$2')"; n=$((n + 1)); }
expect "a/b/index.md" "a/b/index.md"                 # exact
expect "./a/b/index.md" "a/b/index.md"               # ./ prefix
expect "proj/a/b/index.md" "a/b/index.md"            # collection prefix
expect "README.md" "a/README.md"                     # unique suffix
expect "index.md" ""                                 # ambiguous suffix
expect "c/odd name (v2).md" "c/odd name (v2).md"     # spaces and punctuation
expect "../outside/secret.md" ""                     # traversal
expect "a/../../outside/secret.md" ""                # traversal through the copy
expect "c/link.md" ""                                # symlink to outside, even though listed
expect "c/dir/secret.md" ""                          # through a symlinked directory
expect ".markdownattractor/note.md" ""               # store: not in the inventory
expect "graphify-out/graph.md" ""                    # arm artifact: not in the inventory
expect "/etc/passwd" ""                              # absolute
expect "nope.md" ""                                  # unknown
expect "a/b/index.md#some-heading" "a/b/index.md"    # an anchor names a heading
expect "t6-proj/a/b/index.md" "a/b/index.md"         # the T6 qmd index prefix
expect "t6-proj/a/README.md#x" "a/README.md"         # prefix and anchor
echo "t6-resolver: $n cases ok"
