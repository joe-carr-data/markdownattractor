# Plan — MCP Registry listing via an `mcpb` bundle (ADR-0012)

Status: proposed, 2026-11-23. Waits for the owner's go (ADR-0012 adds a public CLI flag).

## Goal

markdownattractor appears in the official MCP Registry as `io.github.joe-carr-data/markdownattractor`, installable as an MCP bundle, published from CI on each release with no stored secret.

## Tasks

- [ ] `mda mcp --start`: ensure a daemon for the root, then serve; integration test (no daemon → one started; one running → reused; `--start` absent → unchanged behaviour). Codex review.
- [ ] `packaging/mcpb/manifest.json` (MCPB 0.3, binary server, `user_config` docs folder → `MDA_ROOT`, `args: ["mcp", "--start"]`, platform overrides) and `packaging/mcpb/mda-linux` launcher (`uname -m`).
- [ ] `server.json` (schema 2025-12-11, description ≤ 100 characters, one `mcpb` package).
- [ ] `release.yml`: a `bundle` job after `build` (universal macOS binary with `lipo`, the launcher, `mcpb validate`, `mcpb pack`, sha256) whose artifact joins the release; a `registry` job after `publish`, publishing runs only (`permissions: id-token: write, contents: read`; pinned, verified `mcp-publisher`; `login github-oidc`; `publish`).
- [ ] Release dry run (`gh workflow run Release --ref main`): the bundle builds and validates; the registry job is skipped.
- [ ] README: a one-line "Also in the MCP Registry" under Install, after the first listing is live.

## Exit criteria

- A dry run builds a valid bundle on all three platforms' binaries.
- After the next owner-approved release, `https://registry.modelcontextprotocol.io/v0/servers?search=markdownattractor` returns the server at that version, and the bundle's sha256 matches.
- `mda mcp` without `--start` behaves exactly as before (existing MCP tests unchanged).

## Not in scope

npm, PyPI, crates.io or container packages; the claude.ai directory submission (owner, by form).
