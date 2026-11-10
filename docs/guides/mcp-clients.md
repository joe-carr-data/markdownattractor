# Use markdownattractor from other MCP clients

The Claude Code plugin is the easiest install, but the index is served by a plain MCP server (`mda mcp`, stdio), so any MCP client can use it. Outside Claude Code nothing is automatic: you install the binary, index the folder and add the server yourself. This guide shows how.

## Status of each client (2026-11-10, v0.2.0, macOS arm64)

| Client | Status | What was checked |
|---|---|---|
| Codex CLI 0.167 | **Verified end to end** | A `codex exec` session answered a question on the demo corpus with `mda_search` and `mda_open` only, citing file and lines. |
| Gemini CLI 0.24 | **Connection verified** | `gemini mcp list` shows the server `Connected`. No model turn was run. |
| Claude Desktop, Cursor, opencode | **Not yet verified** | The configuration below follows each client's documented format. The server command itself was checked with a bare MCP handshake (minimal environment, working directory `/`): it lists all seven tools. |

If one of the unverified entries does not work for you, please open an issue with the client's version.

## 1. Install the binary

Download the archive for your platform from the [latest release](https://github.com/joe-carr-data/markdownattractor/releases/latest), check it and put `mda` on your `PATH`. On an Apple Silicon Mac:

```bash
curl -fsSLO https://github.com/joe-carr-data/markdownattractor/releases/latest/download/mda-darwin-arm64.tar.gz
curl -fsSLO https://github.com/joe-carr-data/markdownattractor/releases/latest/download/SHA256SUMS
shasum -a 256 --ignore-missing -c SHA256SUMS      # Linux: sha256sum --ignore-missing -c SHA256SUMS
tar -xzf mda-darwin-arm64.tar.gz
mkdir -p ~/.local/bin && mv mda ~/.local/bin/
mda --version
```

The other archives are `mda-darwin-x64`, `mda-linux-x64`, `mda-linux-arm64` and `mda-windows-x64`. To build from source instead: `cargo install --git https://github.com/joe-carr-data/markdownattractor mda-cli`.

## 2. Index the folder

```bash
cd /path/to/your/project
mda start          # watches the folder, indexes on save, writes summaries in the background
```

Search works on raw text as soon as the files are parsed, within seconds. Summaries ("cards") need a summarization backend: your own Anthropic API key or a free local model ([local model guide](local-model.md)); choose with `mda backend`. Without one, run `mda index --no-summarize` and the tools still answer from raw text.

The index lives in `.markdownattractor/` inside the folder. Your markdown files are never modified.

## 3. Add the server to your client

Every client needs the same three facts: the command (`mda`, as an absolute path if your client does not inherit your `PATH`), the argument `mcp`, and the folder in `MDA_ROOT`. Replace `/path/to/your/project` below.

### Codex CLI

In `~/.codex/config.toml`:

```toml
[mcp_servers.markdownattractor]
command = "mda"
args = ["mcp"]
env = { MDA_ROOT = "/path/to/your/project" }
# Non-interactive runs (`codex exec`) refuse MCP tool calls that need approval.
# The seven tools only read the index; this line approves them for this server.
default_tools_approval_mode = "approve"
```

### Gemini CLI

In `.gemini/settings.json` in the project (or `~/.gemini/settings.json`):

```json
{
  "mcpServers": {
    "markdownattractor": {
      "command": "mda",
      "args": ["mcp"],
      "env": { "MDA_ROOT": "/path/to/your/project" }
    }
  }
}
```

### Claude Desktop (not yet verified)

In `~/Library/Application Support/Claude/claude_desktop_config.json` (Windows: `%APPDATA%\Claude\claude_desktop_config.json`), then restart the app. Use the absolute path of `mda`; the app does not read your shell's `PATH`.

```json
{
  "mcpServers": {
    "markdownattractor": {
      "command": "/Users/you/.local/bin/mda",
      "args": ["mcp"],
      "env": { "MDA_ROOT": "/path/to/your/project" }
    }
  }
}
```

### Cursor (not yet verified)

In `.cursor/mcp.json` in the project (or `~/.cursor/mcp.json`): the same `mcpServers` object as Claude Desktop.

### opencode (not yet verified)

In `opencode.json` in the project:

```json
{
  "mcp": {
    "markdownattractor": {
      "type": "local",
      "command": ["mda", "mcp"],
      "environment": { "MDA_ROOT": "/path/to/your/project" }
    }
  }
}
```

## 4. Ask

The server tells the client how to use it: search first, read the hits, open only the lines needed. A first question to try:

> Using the markdownattractor tools, why did we choose Postgres? Cite the file and line range.

## What is different outside Claude Code

- **No hooks.** Nothing starts the daemon for you; run `mda start` once per folder (it survives until `mda stop` or a reboot).
- **No `/mda` commands or search-first skill.** Use `mda …` in a terminal. The server's own instructions carry the search-first guidance.
- **One folder per server entry.** `MDA_ROOT` names it. For several folders, add several entries with different names.
- **The embedding model** (for semantic search) is downloaded by `mda start` or `mda index` into `~/.cache/markdownattractor/models`, never by a query. Until then search is lexical.
