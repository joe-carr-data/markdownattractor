# Design — daemon and watcher

As built at the end of Phase 1 (2026-09-22). Decisions and their reasons: ADR-0003. This document describes *what the code does*; if it disagrees with the code, the code wins and this file gets fixed.

## Shape

```
notify ─► Debouncer ─► indexer task (Engine A) ──wake──► summarizer task (Engine B)
                          sync_path / index_root            bounded rounds of summarize_pending,
                          rename detection, hot set         hot paths first, backoff on failure
local socket ─► server task: ping / status / stop / pause / resume / index / rescan / watch
```

One process per watched root (`mda daemon --root <root>`, spawned detached by `mda start`). Everything it owns lives under `<root>/.markdownattractor/`: `daemon.lock` (OS file lock held for the daemon's life), `daemon.pid`, `daemon.json` (pid, version, start time, socket), `daemon.sock` (Unix; a named pipe on Windows), `logs/daemon.<date>.log` (daily rolling, seven kept). Nothing else in the root is ever written.

## Intake

1. **Watcher** (`daemon::watch::Watcher`): `notify` 8 recursive on the canonical root, armed *before* the initial scan so nothing written meanwhile is missed. Every event path becomes a `Hint`; access events (reads, including the daemon's own parses) are dropped at the source, and a lost-events signal becomes a rescan hint.
2. **Classification** (`classify`): anything under `.markdownattractor/` or `.git/` is ignored. Ignore files (`.gitignore`, `.ignore`, `.markdownattractorignore`) are structural. Then the entry decides, not its name: a directory is structural; a present markdown file is markdown; a present non-markdown file is ignored; a vanished markdown name is markdown (tombstone or rename); any other vanished path is structural, because it may have been a directory.
3. **Debounce** (`Debouncer`): a markdown path is released once it has been quiet for the debounce period (1 s) *and* its size has not changed since the last hint; a path still growing is held for another period. Structural hints are debounced the same way into one rescan.
4. **Batch application** (`apply_batch`): present paths first (`Engine::sync_path`: index, or ignore if the walker would not discover a new file), then missing paths, then the rescan if one is due. A vanished document whose content hash equals a document created in the same batch is a **rename**: the new row inherits `created_at`, `created_at_source` and `first_seen_at`, the old one is tombstoned without a delete event, and a `doc_renamed` event names the old path. Matching is one-to-one. Otherwise the missing document is tombstoned. Every changed or created document is added to the **hot set**.
5. **Rescan** (`Engine::index_root`): parses everything the walker finds (unchanged documents short-circuit on their content hash) and tombstones every live document the walker did **not** find, whether deleted or newly excluded by an ignore rule. A discovered file that fails to parse keeps its previous index.

## Summarizer

- Runs **rounds** of `Engine::summarize_pending` bounded to 2 × the pool's maximum concurrency (32 on `api`, 8 on `local`). Each round takes hot documents first (most recently touched first), then everything else smallest first, and starts the pool at the concurrency the previous round ended with.
- Sleeps on a `Notify` woken by the indexer, re-checking the store every 30 s regardless.
- **Backoff**: a round in which nothing succeeded (bad key, dead server, outage) doubles a pause from 5 s to 5 min; a round the daily budget cut pauses 10 min. Pause and cancel are re-checked after every sleep, before any model call.
- **Nothing is marked failed by the environment**: cancelled, stopped and pool-stopping outcomes leave sections pending. Only the model's answer or the validator can fail a section.
- When the configured backend cannot be built (no API key in the environment), the daemon still starts with an `Unavailable` backend: watching and raw indexing work, every round backs off, `mda status` shows the reason, and `mda restart` picks up the fix.

## Control channel

Newline-delimited JSON over an `interprocess` local socket. Requests: `ping`, `status`, `stop`, `pause`, `resume`, `index {path?}`, `rescan`, `watch` (streams events until the client hangs up). Request lines are capped at 64 KiB, 32 clients are served at once, every response write has a 10 s deadline. On Unix the socket and the state files are `0600`. Liveness is "does the socket answer `ping`"; exclusivity is the file lock, never the socket.

`mda index` while the daemon runs delegates over the socket and returns when the raw index is updated; cards follow in the background. `mda status` and `mda doctor` merge the store's counts with the live block. `mda open` from another shell writes through its own connection; the store's 5 s busy timeout and `BEGIN IMMEDIATE` transactions make that safe.

## Shutdown

`mda stop` (or SIGTERM, Ctrl-C in `--foreground`) cancels the token: the summarizer's in-flight calls are cut off and their sections stay pending, the indexer finishes its batch, the socket closes, then `daemon.pid`/`daemon.json` are removed and the lock is released. `stop` waits for the pid file to disappear (15 s). A crash leaves pid/info files and possibly the socket file behind; the next `start` removes them, and the lock cannot be stale because the OS released it.

## Measured (this repo's `docs/`, 18 files, 163 sections, Apple M3, `api` backend with Haiku 4.5)

| What | Number |
|---|---|
| Backfill of 61 new sections at start (two rounds, pool 4→) | 60 s, $0.28 |
| Save → file indexed and raw-searchable | **1.27 s** (1 s debounce + parse) |
| Save → card attached | **4.57 s** |
| Rename of a 3-section file | 0 model calls, `created_at` kept, detected 0.2 s after indexing the new path |
| `mda index` delegated to the daemon (17 files, unchanged) | 11 ms |

## Known limits (v1)

- A directory rename is reconciled by rescan; per-file rename history inside it survives only when the old and new hints land in the same batch.
- No peer authentication on the socket; another user with write access to the project directory can drive the daemon (they can already edit the files).
- Config changes need `mda restart`; the state directory is deliberately not watched.
- Windows builds and passes the unit and CLI tests in CI; it has not been run by hand.
