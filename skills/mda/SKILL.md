---
name: mda
description: Run a markdownattractor command — start/stop the indexer, check status, search the markdown index, open a section by line range, configure models and budgets, or diagnose problems. Use for any "/mda …" request or when the user asks to index, search, or inspect their markdown knowledge layer.
argument-hint: <command> [args]   e.g. status · search "how do we roll back" · open <section_id> · doctor
allowed-tools: Bash("${CLAUDE_PLUGIN_ROOT}/scripts/mda" *)
---

# /mda — markdownattractor command family

You are running a markdownattractor command for the user. The binary is invoked through the launcher `"${CLAUDE_PLUGIN_ROOT}/scripts/mda"`. Always pass `--json` and render the result for the user; never paste raw JSON unless asked.

Arguments: `$ARGUMENTS`

## How to run

1. Parse the first word of the arguments as the command. If empty, run `help`.
2. Run: `"${CLAUDE_PLUGIN_ROOT}/scripts/mda" --json <command> <rest of arguments>` from the project directory. For `search`, pass the query as one quoted argument.
3. Render:
   - `status` → one line summary (docs indexed / pending / failed, index hit rate, tokens today, model), then a short table if there is anything pending or failed.
   - `search` → a numbered list: `path › heading path · lines A–B · updated <relative time> · <tldr or raw snippet>`, and after the list the exact `mda open <section_id>` the user or you would run next. Mark hits from unsummarized sections as `(pending)`.
   - `open` → the exact source lines in a fenced block with the path and line range as its title. If the result says `stale: true`, say so in one line.
   - `doctor` → the checks with ✓ / ! / ✗ and the fix lines verbatim.
   - `start` → the one-line result; if it reports `backend_warning`, show it and the fix (`mda doctor`, then `mda restart`). On a first run the result carries `example` (a real question from a fresh card and its top hit): show the query, the hit's path, heading and line range, and the `mda open` id; if `example_skipped` is set instead, say why in one line. `start` may take up to a minute on a first run while the first cards land; that is expected.
   - `watch` → with `--json` it prints one event per line and runs until Ctrl-C; pass `--count N` to stop after N events. Render each event as one short line.
   - `cost` → the window totals on one line, then the per-day and per-model tables; say plainly that tokens saved on reads are not measured yet (`tokens_saved` is null).
   - `diagnostics` → pass `--out "${TMPDIR:-/tmp}/mda-diagnostics-$(date +%Y%m%d-%H%M%S).json"` (the file must not exist yet; the binary never overwrites) and tell the user where the redacted bundle is and that it holds no document content or paths; never paste the bundle.
   - `nudge` → one line: effective state, this root, everywhere.
   - `timeline`, `recent`, `stale` → a compact table.
   - anything else → the one-line result the binary printed.
4. If the binary exits non-zero, show the `error` field and, when it mentions `doctor`, offer to run `/mda doctor`. If it says the API key is not set, explain the two options in one line each: `export ANTHROPIC_API_KEY=…` (console.anthropic.com) or `/mda backend local` with a llama.cpp server (link `docs/guides/local-model.md`). Never suggest `claude-cli` unprompted; if the user asks for it, show the policy text the binary prints.
5. If the launcher prints "mda binary not found", tell the user to start a new session (the plugin fetches the binary at session start) or run the `cargo install` line it prints.

## Commands (short reference)

| Command | Does |
|---|---|
| `start [path] [--no-example]` · `stop` · `restart` · `status` · `watch` · `doctor` | lifecycle |
| `index [path] [--no-summarize]` · `pause` · `resume` · `rebuild` · `ignore <pattern>` · `prune` | indexing |
| `search <query> [--since 7d] [--until date] [--status current] [--in path] [--raw] [-k 8]` · `open <section_id>` · `card <id>` · `timeline [--since 30d]` · `recent [n]` · `stale` · `explain <query>` | search |
| `summarization_model <id>` · `escalation_model <id\|off>` · `backend [api\|local\|claude-cli]` · `embeddings <…>` · `concurrency <n\|auto>` · `budget <tokens/day\|off>` · `retention <days\|forever>` · `nudge <on\|off> [--global]` · `config` | configuration |
| `cost [--since 7d]` · `diagnostics [--out file]` · `export` · `logs [--tail 100]` · `reset` · `version` · `update` · `help` | maintenance |

Commands not yet available in the installed build return an "unknown command" error; report it plainly and suggest `/mda help`.

## Rules

- Never read or edit files under `.markdownattractor/` yourself; the binary owns them.
- `reset` deletes the index: ask for confirmation with AskUserQuestion before running it. Nothing else needs confirmation.
- Don't run `start` on a directory that isn't the user's project root without saying which directory you're about to index.
