# ADR-0012 — List the MCP server in the official MCP Registry as an `mcpb` bundle

Status: **Accepted** · 2026-11-23 · Owner decision ("go registry")

## Context

The launch plan (2026-11-22) lists markdownattractor in the official MCP Registry (registry.modelcontextprotocol.io) for discoverability. The registry stores metadata only; the server must be published to a package type it accepts: `npm`, `pypi`, `nuget`, `cargo`, `oci` or `mcpb` (registry docs, `package-types.mdx`, read 2026-11-22). We have prebuilt binaries on GitHub Releases and no npm, PyPI or NuGet account to use; `cargo` needs a crates.io account and a Rust toolchain on every user's machine; `oci` (ghcr.io) runs the server in a container, a poor fit for a server that reads the user's local markdown.

`mcpb` (MCP Bundles) points at a file on GitHub Releases: no third-party account. The rules: the asset URL must contain "mcp" (so `markdownattractor-mcp-<version>.mcpb`), `fileSha256` is required, and `mcp-publisher login github-oidc` authenticates from GitHub Actions with `id-token: write` and no stored secret, granting the `io.github.joe-carr-data/*` namespace.

One product gap: a client that installs the bundle (Claude Desktop today) runs `mda mcp`, which serves an existing index and starts nothing. In Claude Code the plugin's hooks start the daemon; outside it, nothing would, and the tools would answer from an empty index.

## Decision

1. **`mda mcp --start`**: before serving, start the daemon for the root if none is running (the same path as `mda start`, without the first-run example), then serve. A public CLI flag, so it goes through a Codex review.
2. **One `mcpb` bundle per release**, built in `release.yml` after the build matrix: `manifest.json` (MCPB 0.3, `server.type: "binary"`) with `user_config` for the docs folder passed as `MDA_ROOT`, `args: ["mcp", "--start"]`; a small launcher (`packaging/mcpb/mda-launch`) runs the macOS or Linux build for the machine (`uname -s`, `uname -m`), and Windows runs `mda.exe` through `platform_overrides`; the bundle also offers an optional Anthropic API key field for the cards.
3. **Publish from CI** with `mcp-publisher` (pinned v1.8.9, checksum-verified) and GitHub OIDC, after the release asset is live; `server.json` in the repository, its version, URL and `fileSha256` filled from the tag. Name `io.github.joe-carr-data/markdownattractor`.

## Consequences

- Positive: listed in the official registry and the clients that read it, with no new account and no stored token; Claude Desktop users can install it as a bundle.
- Negative: two new CI tools (`@anthropic-ai/mcpb` via npm, no account; `mcp-publisher`); a large bundle (five builds, about 30 MB each before compression); the registry is in preview and the MCPB spec is moving; architecture selection inside one bundle is our workaround, not a spec feature.
- Follow-ups: the plan `docs/plans/2026-11-mcp-registry.md`; a release dry run must build and validate the bundle before any publishing run.

## Alternatives considered

- `cargo` — needs a crates.io account and makes every user compile ~30 MB of Rust; the registry docs themselves point binary servers to `mcpb`.
- `npm` wrapper — needs an npm account; ruled out.
- `oci` on ghcr.io — no account needed, but a container cannot read the user's files without volume setup.
- Not listing — the Claude Code plugin install keeps working; the listing is discoverability only.
