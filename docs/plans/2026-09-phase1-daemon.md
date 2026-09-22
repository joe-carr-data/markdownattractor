# Phase 1 — Daemon and watcher

Status: **done** (on branch `feat/daemon`, PR #4) · 2026-09-22 · plan §8 Phase 1, row 13 of `2026-09-phase1-engine.md` · decisions in ADR-0003

Goal: `mda start` turns a folder into a live index. Saving a markdown file makes it raw-searchable within a second and carded within the p50 budget (< 15 s on the `api` backend), without the user running anything again. `mda stop`, `mda status` and `mda watch` control and observe it. Everything the daemon owns lives under `.markdownattractor/`; nothing else in the root is touched.

## Module contracts

All new code in `crates/mda-core/src/daemon/` except the CLI wiring. Each file has its own tests; the daemon as a whole is tested end to end with the mock backend.

| # | Module | Contract |
|---|---|---|
| 1 | `store` (additions) | `busy_timeout` 5 s on every connection. `document_hash(rel_path) -> Option<String>` for the unchanged short-circuit. `note_rename(old_rel, new_rel, at)`: copy `created_at`/`created_at_source`/`first_seen_at` from the old row to the new, tombstone the old, emit `EventKind::DocRenamed` with `detail = moved_from`. |
| 2 | `pipeline` (additions) | `index_parsed` returns early when the stored content hash equals the parsed one (line ranges cannot have moved). `sync_path(abs) -> SyncOutcome { Indexed(IndexOutcome) \| Tombstoned \| Ignored }`: index if the file exists and is discoverable, tombstone if it is gone, ignore otherwise. `SummarizeOptions.hot_paths: Vec<String>`: sections in those documents are submitted first, in the given order, then the rest smallest first. |
| 3 | `daemon::watch` | `Watcher::start(root) -> (Watcher, Receiver<Hint>)` over `notify` (recursive). `Hint { path, at }`. `Debouncer::new(quiet: Duration)`, `push(hint)`, `due(now) -> Vec<PathBuf>` releases paths quiet for ≥ `quiet` whose size has not changed since the last hint; `next_deadline()` for the select loop. Pure apart from `metadata()`. |
| 4 | `daemon::hot` | `HotSet`: `touch(rel_path, at)`, `snapshot() -> Vec<String>` most recent first, `expire(older_than)`. |
| 5 | `daemon::ipc` | `Request { Status, Stop, Pause, Resume, Index { path: Option<String> }, Rescan, Watch }`, `Response { Ok, Status(LiveStatus), Error(String), Event(DaemonEvent), Indexed(IndexReport) }`. `socket_name(root)`, `pid_path(root)`, `info_path(root)`; `DaemonInfo { pid, version, started_at, socket }` written to `daemon.json`. `Server::bind(root)`, `Client::connect(root)`, `Client::request(&Request) -> Response`, `Client::subscribe() -> Stream<DaemonEvent>`. JSON lines. |
| 6 | `daemon::mod` | `Daemon::run(root, backend, DaemonConfig, cancel) -> Result<()>`: writes pid/info, binds the socket, starts the watcher *before* the initial `index_root`, then runs three tasks: **indexer** (owns Engine A; applies debounced batches via `sync_path`, detects renames within a batch by content hash, reconciles on structural hints, touches the hot set, wakes the summarizer), **summarizer** (owns Engine B; loops bounded rounds of `summarize_pending` with `hot_paths`, sleeps when nothing is pending, exponential backoff after a round with no success, honours pause), **server** (answers requests, streams events). Removes pid/info/socket on exit. `LiveStatus` and `DaemonEvent` are `Serialize`. |
| 7 | CLI | `mda start [path] [--foreground]`, `mda stop`, `mda restart`, `mda watch`, hidden `mda daemon --root`. `mda status` adds the live block when the daemon answers. `mda index` delegates to a running daemon. `mda doctor` gains a `daemon` check. Daemon logs to `.markdownattractor/logs/daemon.log` (daily rolling). |

## Tasks

- [x] ADR-0003 (per-root, notify, interprocess) — written.
- [x] Store: `busy_timeout`, `document_hash`, `note_rename` + `EventKind::DocRenamed`, tests.
- [x] Pipeline: unchanged short-circuit, `sync_path`, `hot_paths` ordering, tests.
- [x] `daemon::watch` with debouncer tests (quiet period, size-stable, structural hint).
- [x] `daemon::hot`, `daemon::ipc` (round-trip test over a real socket).
- [x] `daemon::mod` with an end-to-end test on the mock backend: start → write file → raw-searchable → carded → rename → delete → stop.
- [x] CLI commands + integration tests (`start --foreground` in a child, `status`, `stop`); `doctor` check.
- [x] Docs: `design/daemon.md`, README and skill updated, CHANGELOG, STATUS, aha, index.
- [x] Live verification on `docs/` with the `api` backend: save → card latency measured.
- [x] Codex review of the daemon step, triaged.

## Exit criteria

- [x] `mda start` in `docs/` and editing one section: raw-searchable in 1.27 s (1 s of it is the debounce), card attached in 4.57 s (`design/daemon.md`).
- [x] Renaming a file costs zero model calls and keeps the document's `created_at`; deleting it tombstones; recreating it resurrects with its cards.
- [x] `mda index` and `mda open` from another shell while the daemon runs never fail with `database is locked`.
- [x] `mda stop` returns within 10 s with in-flight jobs recorded and unstarted ones left pending; `mda start` again resumes them.
- [ ] Daemon end-to-end test with the mock backend green on all three CI OSes (PR #4 checks).
- [x] Codex review filed with every finding triaged (`reviews/codex/2026-09-22-daemon.md`).
