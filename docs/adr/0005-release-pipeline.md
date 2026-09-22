# ADR-0005 — Release pipeline: a plain GitHub Actions matrix, archives named for `bootstrap.sh`, conditional notarisation

Status: **Accepted** · 2026-09-22 · Plan §9.3, §9.4, Phase 4 plan row 7

## Context

The plugin does not ship its binary in git (plan §9.3): the SessionStart hook `scripts/bootstrap.sh` downloads `https://github.com/joe-carr-data/markdownattractor/releases/download/v<VERSION>/mda-<os>-<arch>.tar.gz` plus `SHA256SUMS`, verifies the checksum, and extracts `mda` (or `mda.exe`) into `${CLAUDE_PLUGIN_DATA}/bin`. Something has to produce those files on every tag. The plan named `cargo-dist` or a hand-written workflow with `cross`.

Facts checked on 2026-09-22:

- `bootstrap.sh` fixes the contract: `mda-{darwin,linux,windows}-{arm64,x64}.tar.gz`, the binary at the archive root, one `SHA256SUMS` in `sha256sum` format. `cargo-dist` names archives by Rust target triple (`mda-aarch64-apple-darwin.tar.xz`) and generates installers we do not need (the hook is the installer).
- One target needs different cargo features: Intel macOS gets a lexical-only build (`--no-default-features`) because `ort-sys` ships no ONNX Runtime binaries for `x86_64-apple-darwin` (Codex F16, ADR-0004). Per-target feature sets are awkward in `cargo-dist`'s configuration and trivial in a matrix.
- GitHub provides native runners for every target we ship: `macos-latest` (arm64, also cross-compiles the Intel lexical-only build with Apple's toolchain), `ubuntu-24.04` (x64) and `ubuntu-24.04-arm` (arm64): the prebuilt static ONNX Runtime needs GCC 13's libstdc++, which 22.04 does not have (the first dry run failed linking `__cxa_call_terminate` on arm64), so the Linux binaries need glibc ≥ 2.39, `windows-latest`. No `cross`, no QEMU.
- Notarisation needs an Apple Developer ID certificate and an app-specific password, which the owner has not set up. Users without a notarised binary see a Gatekeeper prompt for downloaded binaries; `bootstrap.sh` downloads with `curl`, which does not set the quarantine attribute, so the binary runs without the prompt in practice.
- The version lives in four places (`VERSION`, `plugin.json`, `marketplace.json`, `Cargo.toml`). Plan §9.4 asks for one source; the plugin manifest is what Claude Code reads and `VERSION` is what the hook reads, so both stay.

## Decision

1. **A hand-written `release.yml`** (`.github/workflows/release.yml`) on `v*` tags and `workflow_dispatch`: a `version` job runs `scripts/dev/check-version.sh` (with the tag), a five-entry `build` matrix produces `dist/mda-<os>-<arch>.tar.gz` (binary at the root, plus `LICENSE` and `README.md`), a `publish` job writes `SHA256SUMS`, **installs the linux-x64 archive through the real `bootstrap.sh` against a local HTTP mirror of the layout** as the gate, and creates the GitHub Release with `gh release create --generate-notes` on tags only. Dry runs (`workflow_dispatch`) build and check everything and upload artifacts without publishing.
2. **Targets**: darwin-arm64 (full), darwin-x64 (lexical-only, `--no-default-features`, cross-compiled on the arm64 runner and therefore not smoke-tested there), linux-x64 and linux-arm64 (glibc ≥ 2.39, see above), windows-x64 (`mda.exe`). Musl is not built: ONNX Runtime has no musl binaries.
3. **macOS signing and notarisation are conditional** on the six `APPLE_*` secrets (`scripts/dev/notarize-macos.sh`: temporary keychain, `codesign --options runtime --timestamp`, `notarytool submit --wait`). Without the secrets the binaries ship unsigned and the workflow says so in its log; `mda doctor`'s manual-install text covers the Gatekeeper case. Setting the secrets is an owner task and needs no workflow change.
4. **One version, four files, checked in CI**: `scripts/dev/check-version.sh` runs in `ci.yml` on every push and PR and in the release workflow with the tag; `scripts/dev/bump-version.sh <x.y.z>` is the only way the version changes. A release is `bump-version.sh`, commit `chore(release): vX.Y.Z`, tag, push the tag.
5. **No Homebrew tap, no crates.io publish yet.** Both are additive later steps behind the same archives; they are listed as follow-ups in the Phase 4 plan.

## Consequences

- We own the matrix: a new target is one `include` entry and, if it needs it, one line in `bootstrap.sh`'s `case`. There is no installer script to maintain because the plugin hook is the installer.
- The release gate exercises the exact download-verify-extract path users run, so a renamed archive or a checksum format change fails the release, not the user.
- The Intel macOS binary searches lexically only and says so (`mda doctor`, `mda embeddings`); its `--version` is not smoke-tested on the arm64 runner (Rosetta is not guaranteed there).
- Unsigned macOS binaries until the owner adds the certificate; documented in `docs/design/distribution.md`.
- `cargo-dist` remains an option if the target list grows past what one matrix reads well; nothing here forecloses it, since the archive names are set by `bootstrap.sh`, not by the tool.

## Alternatives considered

- **`cargo-dist`** — excellent for the common case, but its archive naming and installer generation are the opposite of what `bootstrap.sh` needs, and per-target feature flags are a configuration fight. Rejected for v1.
- **`cross` / QEMU for arm64 Linux** — unnecessary now that GitHub has arm64 runners; slower and another tool.
- **Shipping the binary in git or as an npm `optionalDependency`** — rejected in plan §9.3.
- **Unconditional notarisation** — would block every release on secrets the owner has not created; conditional keeps releases flowing and upgrades in place.
