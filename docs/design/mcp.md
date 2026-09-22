# Design — MCP server

As built in Phase 2 (2026-09-22). Decisions: ADR-0004. This document describes *what the code does*; if it disagrees with the code, the code wins and this file gets fixed.

## Shape

`mda mcp` serves the index over stdio with `rmcp` 3. The plugin declares it in `.mcp.json` (`${CLAUDE_PLUGIN_ROOT}/scripts/mda mcp`, `MDA_ROOT=${CLAUDE_PROJECT_DIR}`, `MDA_MODEL_DIR=${CLAUDE_PLUGIN_DATA}/models`; the plugin reference substitutes these placeholders in an MCP stdio server's `command`, `args` and `env`), so Claude Code starts it with the plugin and the tools appear as `mcp__plugin_markdownattractor_markdownattractor__<tool>`. The root is `--root`, else `$MDA_ROOT`, else the nearest indexed ancestor of the working directory.

One `Engine` behind a mutex, one optional embedder built from the config. stdout is the protocol channel; logs go to stderr. Results are `structured_content` (plus the same JSON as text) built from the CLI's own `--json` types, so a skill reading the CLI and a tool call see one format. The one exception is `mda_search`: its hits keep the CLI's field names but drop the ranking diagnostics and the snippet-when-carded, because a search result is paid on every question (the CLI `--json` shape is unchanged).

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `mda_search` | `query`, `k` (default **5**, ≤ 50), `since`, `until` (`7d`, `2026-09-01`), `path_prefix`, `raw` | `{ query, partial, hits[] }`: the **lean hit view** (`HitView`): `section_id`, `rel_path`, `heading_path`, `line_start`/`line_end`, `token_estimate`, `tldr` (or `snippet` only when the section has no card), `matched`, `pending` (only when true), `updated_at` to the second. The CLI's ranking diagnostics (`score`, `vector`, `vector_score`, `via_or_fallback`) are not sent; `partial` says the hits are OR matches. Five hits ≈ 400 tokens where the old eight-hit payload was ≈ 1,400 (`docs/benchmarks.md`). |
| `mda_card` | `section_id` | the stored section without its text: state, summary, provenance, `fail_reason` |
| `mda_open` | `section_id` | exact source lines, re-checked at read time; `stale: true` and the current `section_id` when the file changed |
| `mda_timeline` | `since`, `until`, `path_prefix`, `limit` | events oldest first with document paths (created, changed, renamed with `moved_from`, deleted, carded, failed) |
| `mda_recent` | `n` | most recently updated documents with section and pending counts |
| `mda_stale` | — | documents with pending or failed sections, or changed or missing on disk |
| `mda_status` | — | store counts, embedding setting/readiness/coverage, the daemon's live status when it runs |

`initialize` carries the search-first instructions (search, read cards, open sections, retry with `raw=true`, then grep and say so). Argument errors come back as `invalid_params`; engine errors as `internal_error`; a bad `section_id` is an error result, not a crash.

## What it never does

- Download the embedding model: a query on a machine without the model is lexical, and `mda_status.embeddings.ready` says so.
- Write into the root: `mda_open` may re-index a changed file into the store, nothing else.
- Talk to a model: summarization is the daemon's or `mda index`'s job.

## Tested

`crates/mda-cli/tests/mcp_cli.rs` spawns the real binary through an `rmcp` client, lists the tools and exercises every one on an indexed temporary root.

## Watched live in Claude Code (2026-09-22)

Headless session on a scratch copy of this repository's `docs/` (indexed raw-only first), with the plugin loaded from the checkout:

```
claude -p "<call mda_status, then mda_search …>" --plugin-dir /Users/jcarr/markdownattractor \
  --allowedTools "mcp__plugin_markdownattractor_markdownattractor__*" --output-format stream-json --verbose
```

What the transcript showed:

- The `init` event lists the server as `plugin:markdownattractor:markdownattractor`, status `connected`, and the plugin as `markdownattractor@inline` (the identity of a `--plugin-dir` plugin; its data directory is `~/.claude/plugins/data/markdownattractor-inline/`, a marketplace install gets `markdownattractor-markdownattractor/`). The seven tools are listed as `mcp__plugin_markdownattractor_markdownattractor__mda_{card,open,recent,search,stale,status,timeline}`.
- The SessionStart hook copied the local release build into `${CLAUDE_PLUGIN_DATA}/bin/mda` (path 3 of `bootstrap.sh`) and, once the `--no-example` flag existed in the binary, started the daemon for the root.
- Claude loaded the two tools through `ToolSearch` (MCP tools are deferred in that build), called `mda_status`, then `mda_search` with `k=3`, and answered with the hit's `rel_path`, heading path and line range verbatim. Four turns, no file reads.
- `mda_status.embeddings.ready` was `false`: the plugin's model directory is `${CLAUDE_PLUGIN_DATA}/models`, empty until the first embedding pass (daemon or `mda index`) downloads the model. A query never downloads, as designed; until then the hits are lexical.
- One first-run bug surfaced on the way: `mda index .` treated `.` as a file and failed with "is a directory". A directory argument is now the root.

The `.mcp.json` placeholders (`${CLAUDE_PLUGIN_ROOT}`, `${CLAUDE_PROJECT_DIR}`, `${CLAUDE_PLUGIN_DATA}`) were substituted as the plugin reference describes; no change was needed there.
