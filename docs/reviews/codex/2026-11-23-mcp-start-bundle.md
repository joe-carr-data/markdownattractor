# Codex review — `mda mcp --start`, the MCP bundle and the registry jobs (ADR-0012)

| | |
|---|---|
| Date | 2026-11-23 |
| Scope | `git diff 6ff06fc..b81e8b8` on `docs/adr-mcp-registry`: `mda mcp --start` (`commands/mcp.rs`, `commands/start.rs`, `tests/mcp_cli.rs`), `packaging/mcpb/`, `server.json`, `.github/workflows/release.yml` |
| Reviewer | Codex CLI, `gpt-6.2-astra`, fresh thread `01a2058c-c3c6-7c81-a1d7-8e2c4c0a9c11`, read-only; it did not run tests or build a bundle |
| Triaged by | Claude, same day |
| Outcome | 10 findings (1 High, 8 Medium, 1 Low): **10 accepted** (three closed by removing the bundle's API key field). Finding 1 exposed an older hole in the plugin itself, fixed here |

## Findings and triage

1. **High — a repository's config could redirect the user's API key.** The bundle passed the key in the environment, and a selected folder's own `.markdownattractor/config.toml` can set `api_base_url` (and `api_key_env`), so the daemon would send the key, or any environment variable named there, to a server the repository chose. **Accepted, and wider than the bundle:** the Claude Code plugin had the same exposure with `ANTHROPIC_API_KEY` since the `api` backend shipped. Fixed in `Config::load`: a non-default `api_base_url` or `api_key_env` from the file is refused unless the user's environment holds the same value (`MDA_API_BASE_URL`, `MDA_API_KEY_ENV`); test `a_repository_config_cannot_redirect_the_api_key`. The bundle no longer carries a key at all.
2. **Medium — log writes could follow symlinks.** `launch` followed a pre-existing `logs` symlink and a `daemon.out` symlink. **Accepted:** the state folder goes through `config::state_dir` (refuses a symlink), and `logs` and `daemon.out` are refused when they are symlinks.
3. **Medium — a manual dry run against a matching tag could publish.** The tag conditions did not check the event. **Accepted:** both now require `github.event_name == 'push'`.
4. **Medium — the bundle job ran npm code with a write token.** **Accepted:** `permissions: contents: read` and `persist-credentials: false`. The MCPB CLI is pinned to 2.1.2; its transitive dependencies are not locked (accepted risk: no token is reachable from that job).
5. **Medium — an omitted optional key stayed a literal `${user_config.anthropic_api_key}`.** **Accepted, closed by removing the field.**
6. **Medium — a changed key was not picked up by a running daemon.** **Accepted, closed by removing the field.**
7. **Medium — `MDA_ROOT` counted as an explicit root.** clap's `env` filled `args.root`, so an inherited `MDA_ROOT=$HOME` would index the home folder on `--start`. **Accepted:** `MDA_ROOT` is read as a fallback outside clap and does not lift the home/filesystem-root guard; the bundle passes the chosen folder as `--root`.
8. **Medium — two first-time clients raced on creating the database.** **Accepted:** `ensure_running` no longer opens the database; the daemon creates it under its own lock. Test `two_simultaneous_first_starts_share_one_daemon` (green three runs in a row).
9. **Medium — the registry could record a digest the public asset does not have.** **Accepted:** the registry job downloads the public asset and compares its SHA-256 with the built bundle before publishing.
10. **Low — with `--json`, an `mda mcp` startup error went to stdout.** **Accepted:** `main` never renders `mda mcp` errors as JSON on stdout.

Plain `mda mcp` keeps its behaviour (Codex confirmed); the existing MCP test is unchanged and green.
