# ADR-0003 — One daemon per watched root, `notify` for the watcher, `interprocess` for the CLI↔daemon socket

Status: **Accepted** · 2026-09-22 · Plan §3.1, §13 question 4 · Evidence: `docs/plans/2026-09-phase1-daemon.md`

## Context

Phase 1 row 13 adds the process that keeps the index live: a filesystem watcher, a debounced intake, the summarizer loop, and a control channel for `mda status|stop|watch`. Three decisions were open in plan §13 and §16:

1. **Per-root daemon or one global daemon serving many roots?**
2. **Which watcher crate?** The plan named `notify` 8.x plus `notify-debouncer-full` (unverified).
3. **Which IPC?** The plan named `interprocess` 2 (unverified) for a Unix socket / named pipe.

Constraints that matter: nothing is ever written into the watched root except under `.markdownattractor/` (golden rule 1); the store is SQLite in WAL mode and the Codex review deferred job leases on the promise that *the daemon is the single summarizer* (F3); `mda open` and `mda index` must keep working from any shell while the daemon runs; CI builds on Linux, macOS and Windows.

## Decision

1. **One daemon per root.** `mda start` in a root spawns `mda daemon --root <root>`, which writes `daemon.pid`, a JSON `daemon.json` (pid, socket name, started_at, version) and listens on a local socket. Everything the daemon owns lives under `<root>/.markdownattractor/`; there is no global registry, no shared state between roots, and `mda stop` in one root cannot affect another. A second `mda start` in the same root connects to the existing socket and reports it, rather than starting a second process.
2. **Watcher: `notify` 8.2 (stable), our own debouncer.** `notify` gives FSEvents/inotify/ReadDirectoryChangesW behind one API and is the de-facto crate. `notify-debouncer-full`'s stable line lags `notify` and cannot express the size-stable check the plan asks for, so the debouncer is ~100 lines of our own: per-path quiet period (1 s default) plus "size unchanged since the last event" before a path is released. Events are treated as *hints*: the intake never trusts an event kind, it stats the path and either indexes it (exists) or tombstones it (missing). A hint about anything that is not a markdown file (a directory, an ignore file, `config.toml`) triggers a full reconcile (`index_root`), which is cheap because unchanged documents short-circuit on their content hash.
3. **IPC: `interprocess` 2.4 local sockets (tokio), newline-delimited JSON.** Unix domain socket on Linux/macOS at `<root>/.markdownattractor/daemon.sock`, falling back to `$TMPDIR/mda-<hash>.sock` when the path would exceed the 104-byte `sun_path` limit on macOS; a named pipe on Windows, named from the same hash. One request per line, one JSON response per line; `watch` keeps the connection open and streams events. Liveness is decided by connecting, never by signals, so no `libc`/`nix` dependency is needed.
4. **Two store connections inside the daemon, one writer role each.** The *indexer* task owns an `Engine` and is the only thing that upserts documents from watcher hints; the *summarizer* task owns a second `Engine` on the same database and is the only thing that attaches cards. SQLite serialises their short transactions; `busy_timeout = 5 s` on every connection absorbs the overlap, including `mda open` re-indexing a stale file from another process. `mda index` while the daemon runs delegates to the daemon over the socket instead of writing itself, so the F3 promise ("the daemon is the single summarizer") holds.
5. **Priority: user edits before backfill, smallest first within a tier.** The indexer records the paths it touched from watcher hints in a *hot set*; every summarization round takes hot-path sections first (most recently touched first), then everything else by size. Rounds are bounded (2 × pool max) so a fresh edit never waits behind a long backfill.

## Consequences

- Two new crates: `notify = "8"` (CC0-1.0, already allowed by `deny.toml`) and `interprocess = "2"` (0BSD OR Apache-2.0), plus `tracing-appender` from the workspace list for the daemon log. Cargo comments reference this ADR.
- Users with many roots run many daemons. Each is one small process that idles on FSEvents/inotify; a global daemon can be added later without changing the per-root layout because every root already carries its own socket name.
- `mda status` now merges two sources: the store (counts, coverage, spend) read directly, and the live daemon (queue, in-flight, watcher health) over the socket when one answers. Without a daemon the second part prints "not running".
- Windows is a first-class build target but only tested by CI's unit and integration tests; detached spawning uses `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`, the socket is a named pipe. The socket path fallback and the pipe naming are covered by tests on their platforms.
- A stuck daemon that no longer answers its socket is reported with its pid and a `kill` hint; `mda stop` never sends signals itself.

## Alternatives considered

- **Global daemon with a root registry under `~/.config`** — fewer processes, but a cross-root registry, a second state directory, and per-root permission questions for a v1 that has zero users. Deferred; the per-root socket naming keeps the door open.
- **`notify-debouncer-full`** — stable 0.7 pins `notify` 8 and its debounce cannot see file size; the rc line tracks `notify` 9 rc. Not worth a pre-release dependency for a check we can write.
- **tokio `UnixListener` + `named_pipe` by hand** — two code paths and a hand-rolled Windows client; `interprocess` is the same amount of code with one path.
- **Signals for `stop` (SIGTERM)** — needs `libc`/`nix`, has no Windows story, and a socket round-trip already gives a graceful stop plus an acknowledgement.

## Amendments

- **2026-09-22 — one `unsafe` block for Windows.** The first Windows CI run of the daemon hung in `mda start`: Rust's `Command` spawns Windows children with `bInheritHandles = TRUE`, so the detached daemon inherited the stdout pipe that the test harness (or any shell capturing output) had given `mda start`, and the capture never saw EOF. The fix is the documented one, `SetHandleInformation(…, HANDLE_FLAG_INHERIT, 0)` on the three standard handles before spawning, which needs `windows-sys` and one `unsafe` block. The workspace lint moved from `forbid` to `deny` so that this single, commented, `#[cfg(windows)]` site can `allow` it; every other `unsafe` still fails the build.
