# Security

## Reporting

Email joe.carr.data@gmail.com with the subject `markdownattractor security`. You'll get an acknowledgement within 72 hours and a fix or a plan within 14 days for anything confirmed. Please don't open a public issue for a vulnerability.

## What we consider in scope

- Anything that makes the daemon or a hook read files outside the watched root, or write into it.
- Anything that lets document content reach a place other than the summarization call (there is no telemetry; if you find a network call that isn't `claude -p` or the embedding-model download, that's a bug).
- Path traversal in `mda_open`, `mda_card`, or the bootstrap script.
- Supply chain: the release pipeline, checksums, the bootstrap download.
- Prompt injection from document content that escapes the worker (the worker runs with `--tools ""` and a scratch cwd precisely so that a hostile markdown file cannot do anything, but if you find a way, we want to know).

## Design notes for reviewers

- Workers are spawned `claude -p` with no tools, no MCP servers, no user settings, and a `MARKDOWNATTRACTOR_WORKER=1` env that makes every hook in this plugin exit immediately. A document is data to the worker, never instructions.
- OAuth tokens are never read; the daemon relies entirely on the user's installed CLI.
- The daemon binds a Unix socket (or named pipe on Windows) with owner-only permissions. No TCP.
- The bootstrap hook verifies SHA-256 checksums published with each release before extracting a binary.
