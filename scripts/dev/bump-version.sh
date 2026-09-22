#!/usr/bin/env bash
# Set the version everywhere it lives, then verify. Usage: scripts/dev/bump-version.sh 0.2.0
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
new="${1:?usage: bump-version.sh <x.y.z>}"
case "$new" in *[!0-9.]*|"") echo "not a semver triple: $new" >&2; exit 1;; esac
old="$(tr -d '[:space:]' < VERSION)"
printf '%s\n' "$new" > VERSION
# JSON: every "version": "<old>" in the two manifests.
sed -i.bak "s/\"version\": \"$old\"/\"version\": \"$new\"/g" .claude-plugin/plugin.json .claude-plugin/marketplace.json
# Cargo: only the workspace.package line.
sed -i.bak "/^\[workspace.package\]/,/^\[/ s/^version = \"$old\"/version = \"$new\"/" Cargo.toml
rm -f .claude-plugin/plugin.json.bak .claude-plugin/marketplace.json.bak Cargo.toml.bak
# Refresh Cargo.lock for the workspace members without touching other pins.
cargo metadata --format-version 1 --offline >/dev/null 2>&1 || cargo metadata --format-version 1 >/dev/null
scripts/dev/check-version.sh
echo "bumped $old -> $new; now: git commit -am \"chore(release): v$new\" && git tag v$new"
