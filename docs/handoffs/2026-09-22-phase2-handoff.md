# Handoff — 2026-09-22 session 2 (CI fix, Phase 1 daemon, Phase 2 search layer)

Written by Claude (Fable 5.1) at the end of the second build session, for the next session. **Read this whole file first, then the files in §2 in the order given.** Everything below was verified in-session unless marked otherwise.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF · previous handoff: `docs/handoffs/2026-09-22-session-handoff.md` (still accurate for Phase 0/1 engine internals; this file supersedes its §1, §8 and §9).

## 0. One-paragraph state

Phase 1 is **complete and merged** (PR #4 daemon/watcher, PR #5 Windows hotfix): `mda start` keeps a folder's index live, with save→raw-searchable in 1.3 s and save→card in 4.6 s measured on this repo's docs with Haiku through the `api` backend. Phase 2 is **built, tested and up as PR #6** (`feat/phase2-search`, head `a8d17ed` at the time of writing): local card embeddings (fastembed bge-small, static ONNX Runtime) fused as a third list, `mda explain|timeline|recent|stale|rebuild|embeddings|eval|mcp`, an MCP server with seven tools declared in the plugin's `.mcp.json`, and a golden set where hybrid recall@5 is 0.983 against 0.883 lexical. 256 tests, clippy pedantic clean, cargo-deny clean, CI green on main for all three OSes. The Codex review of Phase 2 and PR #6's CI were in flight when this was written; see §9 for what to check first.

## 1. Repo, branches, PRs

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor (private) |
| `main` | `19b0b0c` = Phase 1 daemon (#4) + Windows hotfix (#5). CI green on ubuntu, macOS, Windows, MSRV 1.89, coverage, deny, rustdoc, shellcheck. |
| `feat/phase2-search` | PR #6, head `a8d17ed` (+ any review-fix commits after this file). Merge when CI is green and the Codex triage is filed. |
| Old branches | `feat/daemon`, `fix/windows-start` merged, not deleted. |
| Codex threads | Daemon review: thread `01a0c77b-6ce4-7712-b2bd-c8baf7221a12` (`docs/reviews/codex/2026-09-22-daemon.md`). Phase 2 review: see `docs/reviews/codex/2026-09-22-phase2.md` if it exists; if it does not, the review had not come back when this was written (§9). |
| MSRV | **1.89** (`File::try_lock`), `Cargo.toml`, `clippy.toml`, CI `msrv` job. |
| Lints | `unsafe_code = "deny"` with exactly one `#[allow]` (Windows `SetHandleInformation` in `mda-cli` `start.rs`, ADR-0003 amendment). |

## 2. Files to re-read, in order

1. `CLAUDE.md`, `docs/STATUS.md`, `docs/aha.md` (15 new lines from this session at the top).
2. `docs/plans/2026-09-phase2-search.md` (contracts, tasks, exit criteria with the one honestly unmet item) and `docs/plans/2026-09-phase1-daemon.md` (done).
3. ADRs: `0003-per-root-daemon-and-ipc.md` (+ amendment), `0004-phase2-vectors-embeddings-mcp.md`.
4. Design: `docs/design/daemon.md`, `docs/design/search.md` (vectors), `docs/design/mcp.md`, `docs/design/summarization.md`.
5. `docs/benchmarks.md`, `docs/reviews/codex/2026-09-22-daemon.md` (16 findings and how they were fixed), `.claude/rules/rust.md` (rules from both reviews).
6. Code, Phase 1 daemon: `crates/mda-core/src/daemon/{mod,watch,hot,ipc,tests}.rs`, `crates/mda-cli/src/commands/{start,stop,daemon,watch,pause}.rs`, `crates/mda-cli/tests/daemon_cli.rs`.
7. Code, Phase 2: `crates/mda-core/src/{embed,mcp,timefmt}.rs`, the vector parts of `search.rs` (`VectorIndex`, `fuse`, `explain`), the additions to `pipeline.rs` (`embed_pending`, `stale`, `recent`, `timeline`) and `store/mod.rs` (`SCHEMA_V3`, embeddings API, `recent_documents`, `documents_with_open_sections`), `crates/mda-cli/src/commands/{explain,timeline,recent,stale,rebuild,embeddings,mcp,eval}.rs`, `crates/mda-cli/tests/mcp_cli.rs`.
8. Plugin: `.mcp.json`, `.claude-plugin/plugin.json` (`mcpServers`), `scripts/mda` (exports `MDA_MODEL_DIR`), `scripts/bootstrap.sh` (starts the daemon for indexed projects, detached), `skills/search-first/SKILL.md` (MCP tools first).
9. `evals/README.md`, `evals/golden/queries.jsonl` (format), `evals/golden/cards.json` (recorded with Haiku, $0.31).

## 3. What was built this session, in order

1. **CI**: first look at GitHub Actions. Only Windows failed, on an `unused_mut` in a unix-gated test; fixed, main green.
2. **Daemon (ADR-0003)**: `notify` watcher armed before the initial scan; own debouncer (quiet period + size-stable); hints are never trusted, the filesystem decides; two Engines (indexer, summarizer) on one WAL database with `BEGIN IMMEDIATE` and a 5 s busy timeout; rename detection by content hash within a batch (history kept, zero model calls); hot paths first, bounded rounds, exponential backoff, pause/resume; local-socket JSON-lines control channel with `Lock` (OS file lock) for exclusivity; `Unavailable` backend so the daemon still indexes without a key. Codex review: 16 findings, all addressed (`docs/reviews/codex/2026-09-22-daemon.md`).
3. **Windows hotfix (#5)**: the detached daemon inherited the parent's stdout pipe (`bInheritHandles = TRUE`); `SetHandleInformation` clears the inherit flag on the three std handles before spawning.
4. **Phase 2 (ADR-0004)**: see §0. Design choices worth knowing: no `sqlite-vec` (would need `unsafe` and an old rusqlite; brute-force scan is fast enough below ~50K sections); `bge-small-en-v1.5-q` only; embeddings keyed by section hash, made after cards, never downloaded inside a query (`Embedder::ready`); MCP as `mda mcp`, results are the CLI's `--json` types; eval reports lexical and hybrid side by side.

## 4. Measured numbers to quote

| What | Number |
|---|---|
| Daemon: save → raw-searchable | 1.27 s (1 s of it is the debounce) |
| Daemon: save → card (Haiku, api) | 4.57 s; 5.06 s after the review fixes |
| Daemon backfill, 61 sections | 60 s, $0.28 |
| Golden set, lexical raw only | recall@5 0.883, MRR 0.747, 0.8 ms/query |
| Golden set, lexical cards + raw | 0.900 / 0.777, 2.2 ms |
| Golden set, hybrid | **0.983 / 0.853**, 60 ms/query in-process (query embedding) |
| `mda search` cold process | lexical 20 ms, hybrid 257 ms (≈ 200 ms model load) |
| Model download + 162 cards embedded | 38 s (33 MB) |
| Recording 117 golden cards with Haiku | 45 s, $0.31 |
| Windows CI test job | 6–15 min (compiles `aws-lc-sys`); everything else < 5 min |

## 5. Recipes that worked

```bash
source "$HOME/.cargo/env"; make check                     # 256 tests, clippy pedantic, deny, rustdoc
cargo nextest run -p mda-core -E 'test(daemon)'          # daemon e2e with the mock backend (~1 s)
cargo nextest run -p mda-cli --test mcp_cli              # rmcp client drives the real binary
export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models   # where the model is on this machine
./target/release/mda eval --golden evals/golden -k 5     # lexical + hybrid runs (cards.json is checked in)
set -a; source ~/.config/markdownattractor/env; set +a   # API key + workspace id, never print
./target/release/mda eval --golden evals/golden --record # re-record cards (spends ~$0.3)
# live daemon on a scratch copy of docs/ (never on the repo's docs/ directly):
cp -R docs "$S/live-docs"; mda start --root "$S/live-docs"; mda --json watch --root "$S/live-docs"; mda stop --root "$S/live-docs"
# Codex review: Agent(subagent_type="codex:codex-rescue") with `--fresh --model gpt-6-astra`, pinned to a SHA, then triage into docs/reviews/codex/
gh pr checks <n>            # read the JOB LIST (gh run view <id>), not the column count: I merged #4 on a misread
```

Measurement scripts must be `bash`, not zsh: zsh does not word-split unquoted variables (`set -- $line` silently breaks).

## 6. Caveats and gotchas (new this session)

- Linux `notify` reports opens; `Access` events are dropped at the source or the daemon feeds itself.
- SQLite deferred transactions that read then write fail with `SQLITE_BUSY_SNAPSHOT` regardless of busy timeout; every store transaction is `BEGIN IMMEDIATE` (`Store::write_tx`).
- A per-call `BufReader` over a socket drops buffered lines; the IPC keeps one framed reader per connection.
- Cancelled, stopped and pool-stopping outcomes **defer** (stay pending), they never mark a section failed (`JobResult::was_stopped`).
- The binary never reads `CLAUDE_PLUGIN_DATA` (in a dev shell it belongs to another plugin); the plugin passes `MDA_MODEL_DIR`.
- fastembed defaults pull `native-tls` (banned); the workspace uses the rustls features. `paste` (via `tokenizers`) is an ignored unmaintained advisory in `deny.toml`.
- `use crate::Result` in the file that carries `#[tool_handler]` breaks the rmcp macro; `mcp.rs` uses `crate::Result<T>` explicitly.
- `.mcp.json` sets `MDA_ROOT=${CLAUDE_PROJECT_DIR}` and `MDA_MODEL_DIR=${CLAUDE_PLUGIN_DATA}/models`; the plugin reference states that MCP stdio servers get the placeholders substituted in `command`, `args` and `env`, and that all three variables are also exported to the server process. Loading the plugin in a real session has still not been watched by hand.
- The hybrid latency exit criterion is **not met** (§4); it is recorded, not hidden.

## 7. Open questions and owner items

- Rotate the API key (passed through the chat transcript on 2026-09-22).
- Watch the plugin load in Claude Code once: `claude plugin validate .` passes; `/reload-plugins` and a tool call through `mcp__plugin_markdownattractor_markdownattractor__mda_search` are the real test.
- Peer authentication on the daemon socket (daemon review F8) deferred.
- Query-embedding latency lever (smaller encoder, ONNX threads, query-vector cache) if 50 ms matters.
- §13: commit `cards/` or not (the only plan question still open).

## 8. Next steps, in order

1. **Finish PR #6**: read the Codex Phase 2 review, triage into `docs/reviews/codex/2026-09-22-phase2.md`, fix accepted findings, make CI green on all OSes (first run with ONNX Runtime on Windows/MSRV/coverage: expect surprises), merge.
2. **First-run UX** (plan §9.5): `mda start` runs one example query when the first cards land; `mda cost`, `mda diagnostics`, `mda nudge on|off` (`${CLAUDE_PLUGIN_DATA}/nudge.off`).
3. **Release pipeline** (Phase 4): cargo-dist, GitHub Releases with `SHA256SUMS` that `scripts/bootstrap.sh` verifies, notarisation; bump `VERSION`, `plugin.json`, `marketplace.json` together.
4. **A/B parity protocol** (plan §11): with/without index through `claude -p`, Sonnet-graded; only then claim token savings.
5. Phase 3 (relationships) stays deferred.

## 9. If this file is older than the PR

Check, in this order: `gh pr view 6` (merged?), `ls docs/reviews/codex/` (Phase 2 review filed?), `gh run list --branch feat/phase2-search` (CI). If the review file is missing, re-run the review with the packet described in `.claude/skills/codex-review/SKILL.md` pinned to the branch head.
