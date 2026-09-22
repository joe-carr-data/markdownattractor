# Design — distribution

As built in Phase 4 (2026-09-22). Decisions: ADR-0005. This document describes *what the pipeline does*; if it disagrees with `.github/workflows/release.yml` or `scripts/bootstrap.sh`, those win and this file gets fixed.

## The contract between the hook and the release

`scripts/bootstrap.sh` (SessionStart) resolves the binary in this order: `$MDA_BIN`, `${CLAUDE_PLUGIN_DATA}/bin/mda` at the wanted version, `${CLAUDE_PLUGIN_ROOT}/target/release/mda` (contributors), then a download from

```
https://github.com/joe-carr-data/markdownattractor/releases/download/v<VERSION>/mda-<os>-<arch>.tar.gz
https://github.com/joe-carr-data/markdownattractor/releases/download/v<VERSION>/SHA256SUMS
```

with `os ∈ {darwin, linux, windows}`, `arch ∈ {arm64, x64}`, the checksum verified before anything is moved, and the binary (`mda`, or `mda.exe` on Windows) extracted from the archive root into `${CLAUDE_PLUGIN_DATA}/bin/mda`. `VERSION` in the plugin root is the wanted version; the hook never blocks a session and points at `mda doctor` on any failure.

## What a tag produces

`git tag vX.Y.Z && git push --tags` runs `.github/workflows/release.yml`:

| Job | Does |
|---|---|
| `version` | `scripts/dev/check-version.sh vX.Y.Z`: `VERSION`, `plugin.json`, `marketplace.json` (both places) and `Cargo.toml` must all say `X.Y.Z`. |
| `build` (×5) | `cargo build --release -p mda-cli --target <triple>` on the native runner, `--version` smoke test, `mda-<os>-<arch>.tar.gz` with the binary, `LICENSE` and `README.md` at the root. |
| `publish` | `SHA256SUMS`, then **the real `bootstrap.sh` installs the linux-x64 archive from a local mirror** of the layout and `--version` must match; then `gh release create` with generated notes. |

`workflow_dispatch` runs the same jobs without publishing (dry run; artifacts stay on the workflow run).

| Archive | Runner | Features | Notes |
|---|---|---|---|
| `mda-darwin-arm64` | macos-latest | default (embeddings) | signed and notarised when the `APPLE_*` secrets exist |
| `mda-darwin-x64` | macos-latest, cross | `--no-default-features` | lexical-only: no ONNX Runtime binaries for Intel macOS; not smoke-tested on the arm64 runner |
| `mda-linux-x64` | ubuntu-24.04 | default | glibc ≥ 2.39 (the static ONNX Runtime needs GCC 13's libstdc++) |
| `mda-linux-arm64` | ubuntu-24.04-arm | default | glibc ≥ 2.39 |
| `mda-windows-x64` | windows-latest | default | `mda.exe` |

## Versioning

One version, four files, one script: `scripts/dev/bump-version.sh X.Y.Z` rewrites `VERSION`, both manifests and `Cargo.toml`, refreshes `Cargo.lock`, and runs the check. `scripts/dev/check-version.sh` also runs in `ci.yml` on every push, so the four cannot drift between releases. `plugin.json`'s version is what makes Claude Code fetch an update (plan §9.4).

## macOS signing

`scripts/dev/notarize-macos.sh` imports the Developer ID certificate into a temporary keychain, signs with the hardened runtime and a timestamp, and submits to `notarytool --wait`. A bare binary cannot be stapled; Gatekeeper verifies online. The step is skipped, with a log line, until the owner adds the six secrets (`APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`). `curl` downloads carry no quarantine attribute, so unsigned binaries installed by the hook run without a prompt; a binary downloaded through a browser would need `xattr -d com.apple.quarantine`.

## Not built

Homebrew tap, crates.io publish, musl builds (no ONNX Runtime binaries), a `claude-plugins-community` submission. All additive; listed in the Phase 4 plan follow-ups.
