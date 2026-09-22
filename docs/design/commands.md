# Design — the command surface

As built through Phase 4 (2026-09-22). One line per command: what it does, what `--json` returns, and what it never does. If this disagrees with `mda --help` or the code, the code wins and this file gets fixed. Every command takes `--root <dir>` (default: the nearest ancestor with `.markdownattractor/`, else the current directory) and `--json`.

## Lifecycle

| Command | Does | Never |
|---|---|---|
| `start [path] [--foreground] [--no-example]` | Spawns the daemon for the root (idempotent). On a **first run** (no cards yet, working backend) it waits up to 60 s for the first 10 cards, picks a question from one of them and prints the top hit with its line range and `mda open` id; JSON `example` / `example_skipped`. The SessionStart hook passes `--no-example`. | Blocks a session: the hook's call is detached and skips the example. |
| `stop` · `restart` | Graceful stop over the socket; unfinished sections stay pending. `restart` never waits for an example. | Sends signals. |
| `status` | Store counts, config, embedding coverage, and the daemon's live block when one answers. | Talks to a model. |
| `watch [--count n]` | Streams daemon events (JSON lines with `--json`). | |
| `doctor` | Root, state dir, backend (reachability), embeddings, daemon; exit 1 on a failure. | Modifies anything. |
| `pause` · `resume` | Stop / restart model calls; indexing continues. | |

## Indexing

| Command | Does | Never |
|---|---|---|
| `index [path\|dir] [--no-summarize] [--retry-failed] [--limit n]` | Parses and indexes (a file, or a directory as the root), then summarizes pending sections and embeds new cards. Delegates to a running daemon. | Summarizes a hash twice. |
| `rebuild --embeddings` | Re-embeds every card with the configured model (downloads it once). | |
| `parse <file>` · `schema section` | Inspection helpers (`parse` takes `.md`, `.markdown` or `.mdx`; `design/ingestion.md`). | Touches the store. |

## Search

| Command | Does |
|---|---|
| `search <query> [-k 8] [--raw] [--since] [--until] [--in prefix]` | Hybrid hits (cards, raw text, vectors when the model is on disk), recency-weighted, filtered; prints the `mda open` id for each. |
| `open <section_id>` | The exact source lines, re-checked at read time; `stale: true` when the file changed. |
| `card <section_id>` | The stored card, or the pending/failed state. |
| `explain <query>` | The three lists behind a search and the fused result. |
| `timeline [--since] [--until] [--in] [--limit]` · `recent [n]` · `stale` | Time questions; `stale` exits 1 when anything is not final. |

## Configuration

| Command | Does |
|---|---|
| `backend [api\|local\|claude-cli]` | Show or switch; `claude-cli` needs `--i-accept-the-policy` (ADR-0002). |
| `embeddings [local-small\|off]` | Show or switch; a running daemon keeps its setting until `restart`. |
| `nudge [on\|off] [--global]` | Per root: `nudge` in `config.toml`. `--global`: the marker at `$MDA_NUDGE_FILE` (the plugin launcher sets it to `${CLAUDE_PLUGIN_DATA}/nudge.off`). The `PreToolUse` hook checks the marker, then the root's config. |

## Maintenance

| Command | Does | Never |
|---|---|---|
| `cost [--since 7d]` | The usage ledger (every model attempt, whatever its outcome) as window totals, per day, and per model/outcome; the all-time card total. Says that tokens saved on reads are **not measured yet** (`tokens_saved: null`). | Estimates savings it has not measured. |
| `diagnostics [--out file]` | A JSON bundle: versions, features, OS/arch, config (workspace id and home directory redacted; the config never holds a key), store counts and schema, embedding check, daemon live status (hot paths dropped), `doctor` checks, the last 40 daemon log lines (home redacted). | Includes document content or paths outside the root. |
| `eval --golden <dir> [-k] [--record]` | Retrieval metrics on a golden set (`docs/benchmarks.md`). | Writes under `evals/` unless `--record`. |
| `mcp` | Serves the index over stdio (`docs/design/mcp.md`). | Prints anything but protocol on stdout. |

Not built yet (plan §6): `root`, `ignore`, `prune`, `summarization_model`, `escalation_model`, `concurrency`, `budget`, `retention`, `config`, `export`, `logs`, `reset`, `update`, `help`. Their settings exist in `config.toml`; the commands are convenience wrappers to come.
