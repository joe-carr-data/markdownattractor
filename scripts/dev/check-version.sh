#!/usr/bin/env bash
# Fail when the four places that carry the version disagree:
# VERSION, .claude-plugin/plugin.json, .claude-plugin/marketplace.json (top level and plugins[0]),
# and Cargo.toml [workspace.package]. Optional first argument: a tag (v1.2.3) that must match too.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
ver="$(tr -d '[:space:]' < VERSION)"
plugin="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' .claude-plugin/plugin.json | head -1)"
mk_top="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' .claude-plugin/marketplace.json | head -1)"
mk_plugin="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' .claude-plugin/marketplace.json | sed -n 2p)"
cargo="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p;}' Cargo.toml | head -1)"
ok=1
for pair in "plugin.json=$plugin" "marketplace.json=$mk_top" "marketplace.json plugins[0]=$mk_plugin" "Cargo.toml=$cargo"; do
  name="${pair%%=*}"; value="${pair#*=}"
  if [ "$value" != "$ver" ]; then
    echo "version mismatch: VERSION is $ver but $name says '$value'" >&2
    ok=0
  fi
done
if [ "${1:-}" != "" ]; then
  tag="${1#v}"
  if [ "$tag" != "$ver" ]; then
    echo "version mismatch: tag $1 but VERSION is $ver" >&2
    ok=0
  fi
fi
[ "$ok" = 1 ] || exit 1
echo "version $ver (VERSION, plugin.json, marketplace.json, Cargo.toml agree)"
