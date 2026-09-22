# Handoff — 2026-09-22 session 2 (CI fix, Phase 1 daemon, Windows hotfix, Phase 2 search layer)

Written by Claude (Fable 5.1) at the end of the second build session, for the next session after compaction. **Read this whole file first, then every file in §3 in the order given; nothing here replaces reading the code and the docs it points to.** Everything below was verified in-session unless marked otherwise.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF · previous handoff (Phase 0/1 engine internals, still accurate for the engine, superseded for repo state and next steps): `docs/handoffs/2026-09-22-session-handoff.md`.

## 0. One-paragraph state

markdownattractor is a Rust Claude Code plugin that turns a folder of markdown into a live, time-aware, searchable knowledge layer: heading-delimited sections, raw-text FTS in milliseconds, one structured "card" per section from a model backend with evidence grounding, local card embeddings, hybrid search fused from three lists, exact line ranges re-checked at read time, a per-root daemon that keeps all of it live, and an MCP server Claude talks to. **Phases 0, 1 and 2 are complete and merged on `main` (`42a8471`)**: engine and CLI (PR-less, session 1), daemon and watcher (PR #4), Windows hotfix (PR #5), search layer with embeddings, MCP, time commands and the eval harness (PR #6). 261 tests, clippy pedantic clean, cargo-deny clean, CI green on ubuntu, macOS and Windows. Three Codex reviews of the code have been filed and fully triaged (14 + 16 + 17 findings). The owner has decided the API key does **not** need rotating. Next up: watch the plugin's MCP server load in a real Claude Code session, then first-run UX (plan §9.5), then the release pipeline (Phase 4).

## 1. Repo, branches, PRs, tickets, threads

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor, **private**. Owner Joe Carr (joe.carr.data@gmail.com). Make public with `gh repo edit --visibility public` when the owner says so. |
| Local clone | `/Users/jcarr/markdownattractor`, remote `origin` over HTTPS (SSH is blocked from the sandbox). Working tree clean at the time of writing. |
| `main` | `42a8471`. History: 44 commits. Merges: #4 `feat/daemon` (`b7888e1`), #5 `fix/windows-start` (`19b0b0c`), #6 `feat/phase2-search` (`28fafa6`). |
| Branches on origin | `feat/daemon`, `fix/windows-start`, `feat/phase2-search` — all merged, not deleted (harmless; delete if you like). |
| PR workflow used | Branch → small conventional commits with the attribution footer → `make check` green → push → `gh pr create` → CI on all OSes → Codex review pinned to the branch head → triage file → fixes → merge with `gh pr merge <n> --merge`. CI-only fixes went straight to `main` once (the Windows `unused_mut`). |
| CI | `.github/workflows/ci.yml`: rustfmt, clippy `-D warnings` (pedantic), nextest on ubuntu/macOS/windows with profile `ci`, MSRV **1.89**, coverage gate 70% (last measured 84% before Phase 2), cargo-deny, rustdoc `-D warnings`, shellcheck. `audit.yml`: weekly `cargo audit` via `rustsec/audit-check@v2`, now with `checks: write` and `ignore: RUSTSEC-2024-0436`. |
| Tickets | None. Work items live in `docs/STATUS.md` (Next, max 3), `docs/plans/*.md` (checkboxes), `docs/backlog.md` (B-0001 document ingestion, B-0002 in-session backend). |
| Codex threads | Pre-mortem: `01a0c558-3d54-7fb2-920a-cdc40379dcf6`. Crates review: `01a0c5bd-d175-7150-9385-4e49b1598d2e` (pinned `b0814de`). Daemon review: `01a0c77b-6ce4-7712-b2bd-c8baf7221a12` (turn `01a0c77b-70f6-70c3-8523-6d936bf2cc9b`, pinned `f7c1d77`). Phase 2 review: `01a0c7cc-43c4-7452-b6c4-69f819526fd3` (pinned `a8d17ed`). All via the `codex:codex-rescue` subagent with `--fresh --model gpt-6-astra`; Codex CLI 0.155.1. |
| Secrets | `~/.config/markdownattractor/env` (mode 600) holds `ANTHROPIC_API_KEY` and `ANTHROPIC_WORKSPACE_ID=wrkspc_<redacted>`. Load with `set -a; source ~/.config/markdownattractor/env; set +a`. Never print or commit. **Owner decision 2026-09-22: no rotation needed.** The permission classifier blocks reading `.env`-style files; ask the owner rather than searching. |
| Embedding model on this machine | `~/.cache/markdownattractor/models/models--Qdrant--bge-small-en-v1.5-onnx-Q/…` (33 MB). Export `MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models` in dev shells; the plugin passes it itself. |
| Versions | `VERSION` 0.1.0 = `plugin.json` = `marketplace.json` = workspace version. Toolchain rustup stable 1.98.1 (`rust-toolchain.toml`), MSRV 1.89, edition 2024. |

## 2. Charter, goals, golden rules (from `CLAUDE.md`, ranked)

G6 Trust (no hallucinated metadata; every date/entity has evidence in the source) › G1 Speed (p50 < 15 s save→card) › G3 Token economy (≥ 5× fewer source tokens per answer at parity) › G5 Searchability (recall@5 ≥ 0.85 on the golden set) › G4 Temporal provenance › G2 Zero-config. Ten golden rules; the ones that shaped this session: never write into the watched root (state lives under `.markdownattractor/`), hash before summarize, raw text is FTS-indexed before any model call, `mda open` re-hashes at read time, every hook exits 0 under `MARKDOWNATTRACTOR_WORKER=1`, no new crate without an ADR, every command has `--json`.

## 3. Files to re-read, in order (T0 → T2)

1. `CLAUDE.md` — charter. Then `.claude/rules/{rust,workflow,docs}.md` (the rust rules now carry the lessons of all three Codex reviews).
2. `docs/STATUS.md` — phase, active plan, next three, blockers, last Codex review.
3. `docs/aha.md` — ~40 dated lessons; the 20 at the top are from this session and explain most non-obvious code.
4. Plans: `docs/plans/2026-09-phase2-search.md` (contracts, tasks, exit criteria with the one honestly unmet item), `2026-09-phase1-daemon.md`, `2026-09-phase1-engine.md`, `2026-09-phase0-spike.md` (all done).
5. ADRs: `0001-worker-backend-claude-cli.md` (+ amendments), `0002-backends-and-login-policy.md`, `0003-per-root-daemon-and-ipc.md` (+ Windows `unsafe` amendment), `0004-phase2-vectors-embeddings-mcp.md`.
6. Design (what the code does): `docs/design/summarization.md`, `search.md` (vectors), `daemon.md`, `mcp.md`. Numbers: `docs/benchmarks.md`.
7. Reviews and triage: `docs/reviews/codex/2026-09-21-pre-mortem.md`, `2026-09-22-crates.md`, `2026-09-22-daemon.md`, `2026-09-22-phase2.md`.
8. `docs/project-plan.md` — the full design, T2, read sections not the whole 60 KB. §13 open decisions are now all closed except "commit `cards/` or not". §16 crate notes are partly stale (sqlite-vec was not used; see ADR-0004).
9. Code, in pipeline order (all under `crates/mda-core/src/`): `lib.rs` (module map) → `markdown/mod.rs` → `card.rs` → `config.rs` → `walk.rs` → `store/mod.rs` (+ `store/tests.rs`) → `diff.rs` → `planner.rs` → `worker/{mod,parse,claude_cli,api,local,http,pool,mock,unavailable}.rs` → `validate.rs` → `pipeline.rs` (+ `pipeline/tests.rs`) → `embed.rs` → `search.rs` → `timefmt.rs` → `daemon/{mod,watch,hot,ipc,tests}.rs` → `mcp.rs`.
10. CLI (`crates/mda-cli/src/`): `main.rs`, `output.rs`, `commands/mod.rs` (root resolution, `block_on`, `live_status`, `embedder_for`), then `commands/{index,search,explain,open,card,status,timeline,recent,stale,rebuild,embeddings,backend,doctor,parse,schema,start,stop,pause,watch,daemon,mcp,eval}.rs`. Tests: `tests/{cli,engine_cli,daemon_cli,mcp_cli}.rs`.
11. Plugin surface: `.claude-plugin/plugin.json` (skills, hooks, `mcpServers: ./.mcp.json`), `.claude-plugin/marketplace.json`, `.mcp.json`, `hooks/hooks.json`, `scripts/{bootstrap.sh,mda,nudge.sh}`, `skills/mda/SKILL.md`, `skills/search-first/SKILL.md`, `prompts/section.v2.txt`, `prompts/section.schema.v1.json`.
12. Evals: `evals/README.md`, `evals/golden/docs/**` (32 fictional platform-team docs), `evals/golden/queries.jsonl` (60 queries), `evals/golden/cards.json` (117 cards recorded with Haiku, checked in). `evals/spike/` is Phase 0.
13. Dev workflow: `.claude/settings.json` (SessionStart/PreCompact/SessionEnd/Stop/PostToolUse hooks that keep STATUS, handoffs and `docs/index.md` honest), `.claude/skills/{handoff,resume,adr,codex-review}/SKILL.md`, `scripts/dev/*.sh`, `Makefile`, `.cargo/config.toml` (aliases `lint`, `t`, `docs`, `cov`), `.config/nextest.toml`, `deny.toml`, `clippy.toml`.

## 4. Architecture as built (all three phases)

```
walk::discover ─► markdown::parse_str ─► store.upsert_document ─► sections_raw_fts   (raw-searchable in ms)
                                                 │ new section hashes (summaries table, state = pending)
                                                 ▼
   store.pending_hashes ─► pipeline (heading-only → deterministic card)
                        ─► planner::chunk_text (≤ 6K tokens, cut at paragraph outside fences)
                        ─► worker::Pool (AIMD concurrency, retries, escalation, backoff, admission)
                              └► Backend: api (default) | local | claude-cli (opt-in) | unavailable
                        ─► validate::validate (caps, dedupe, entity + date grounding, iso normalisation)
                        ─► store.attach_summary (by hash; cards_fts) + store.record_usage (ledger)
                        ─► pipeline::embed_pending: embed_text(title+heading_path+card) ─► embeddings table (schema v3)
search::search_with: fts_escape → cards_fts + sections_raw_fts (bm25) ┐
                     embed(query) → VectorIndex::top_k (dot product)  ┤→ RRF (k=60) → recency → filters → top-k; OR fallback on eligible-empty
pipeline::Engine::open_section: re-parse, compare doc hash, re-index if changed, resolve by hash→heading→index, return current lines + stale + current id
daemon::run: notify watcher → Debouncer → indexer task (Engine A: sync_path / index_root, rename by content hash, hot set)
             ──wake──► summarizer task (Engine B: bounded summarize_pending rounds, hot paths first, backoff, embed_pass)
             local socket (interprocess) → server task: ping/status/stop/pause/resume/index/rescan/watch
mcp::serve_stdio: rmcp 3 over stdio, tools mda_search/card/open/timeline/recent/stale/status; engine work on spawn_blocking
```

Key invariants (also in `CLAUDE.md` and `.claude/rules/rust.md`):
- Summaries **and vectors** are keyed by section hash (blake3 of normalised text), never by position; moved or duplicated sections cost nothing. `doc_id` = first 16 hex of blake3(rel_path); `section_id` = `{doc_id}#{index}`.
- Nothing writes into the watched root; state is `<root>/.markdownattractor/{config.toml,index.sqlite,daemon.lock,daemon.pid,daemon.json,daemon.sock,logs/}`. Every state file is created through `config::state_dir` / `config::write_private`, which refuse symlinks and use mode 0600.
- Every filesystem path goes through `Engine::rel_path` / `Engine::safe_join` (canonical containment), including diagnostics like `stale()`.
- Every store transaction is `BEGIN IMMEDIATE` (`Store::write_tx`) with a 5 s busy timeout; the daemon's two engines and any `mda` command in another shell share the file safely.
- Every model attempt is logged to `usage_log` regardless of outcome; the daily budget reads that ledger. A cancelled, stopped or pool-stopping outcome **defers** a section (stays pending); only the model's answer or the validator can fail one.
- Watcher events are hints, never facts; `Access` events are dropped at the source; the filesystem decides.
- A query never downloads a model: `Embedder::ready` (every artifact present) gates the vector list; a missing or broken model leaves search lexical with one warning.
- The binary never reads `CLAUDE_PLUGIN_DATA`; the plugin passes `MDA_MODEL_DIR` and `MDA_ROOT` explicitly (`.mcp.json`, `scripts/mda`, `scripts/bootstrap.sh`).

Store schema v3 (`store/mod.rs` `SCHEMA_V1..V3`, `MIGRATIONS`): `meta`, `docs`, `summaries` (hash PK, state pending|summarized|failed), `sections` (FK docs, FK summaries), `events` (kinds doc_created/changed/deleted/**renamed**/section_summarized/failed), `usage_log`, `embeddings` (hash+model PK, dim, f32 LE blob, L2-normalised), FTS5 `sections_raw_fts`, `cards_fts`. Timestamps are RFC 3339 UTC text (`fmt_ts`). To add a version, append to `MIGRATIONS`.

Backends (ADR-0002): `api` default (Messages API, `output_config.format` structured output, single turn, cached system prompt, `anthropic-workspace-id` header, price table); `local` (OpenAI-compatible, pool 2→4, ≥ 300 s timeout, `docs/guides/local-model.md`); `claude-cli` opt-in behind `claude_cli_policy_ack`; `unavailable` (daemon runs without a key: indexing and raw search work, every round backs off, `mda status` shows the reason).

Daemon (ADR-0003, `docs/design/daemon.md`): per-root process spawned detached by `mda start` (own process group on Unix; on Windows `DETACHED_PROCESS` after `SetHandleInformation` clears the inherit flag on the three std handles — the one `unsafe` site in the workspace, lint is `deny` not `forbid`). Exclusivity is `daemon.lock` held with `File::try_lock` (MSRV 1.89). Debounce 1 s + size-stable; structural hints (directories, ignore files, lost events) trigger `index_root`, which also tombstones documents the walker no longer discovers (ignore rules take effect). Renames: a vanished doc whose content hash equals a doc created in the same batch keeps `created_at`, one-to-one. Rounds are 2 × pool max (32 on `api`); the previous round's final concurrency seeds the next; backoff 5 s → 5 min doubling when nothing succeeded, 10 min on budget exhaustion; pause and cancel re-checked after every sleep. IPC: newline JSON over `interprocess` local sockets (Unix socket 0600 under the state dir, temp-dir fallback over 100 bytes, named pipe on Windows), 64 KiB request cap, 32 connections, 10 s write timeout. Logs roll daily to `.markdownattractor/logs/daemon.<date>.log` (7 kept). The SessionStart hook starts the daemon (detached) for projects that already have a `.markdownattractor/` directory.

Search (ADR-0004, `docs/design/search.md`): `bge-small-en-v1.5` quantised through fastembed (384-d, ~33 MB, CPU, static ONNX Runtime, pure-rustls features) behind the default-on cargo feature `embeddings` (`--no-default-features` gives a lexical-only binary, needed on Intel macOS where `ort-sys` ships no binaries). Model cache: config `embedding_cache_dir` → `$MDA_MODEL_DIR` → `~/.cache/markdownattractor/models`. Vectors are made after cards (`Engine::embed_pending`, batches of 32, keyset pagination by hash), by `mda index`, by the daemon (stepped, pause/stop aware) and by `mda rebuild --embeddings`. `VectorIndex` loads all vectors of the model into one contiguous buffer and scans by dot product; no `sqlite-vec` (would need `unsafe` + an old rusqlite; fine below ~50K sections). `Matched::Vector`, `Hit.vector`, `Hit.vector_score`. `mda explain` shows the three lists at the search's own candidate depth and says when the OR form was used.

MCP (`docs/design/mcp.md`): `mda mcp` on rmcp 3.4 (`server`, `transport-io`, `macros`, `schemars`), stdio; root from `--root`, `$MDA_ROOT`, else nearest indexed ancestor of cwd. Tools return `structured_content` built from the CLI's `--json` types. `initialize` carries the search-first instructions. Tested end to end by `crates/mda-cli/tests/mcp_cli.rs` through an rmcp client that spawns the real binary. **Not yet watched by hand inside a Claude Code session** (`claude plugin validate .` passes; the plugin reference confirms placeholder substitution in `command`/`args`/`env` for stdio servers).

Evals: `mda eval --golden evals/golden -k 5` copies the corpus (symlinks skipped) into a temp root, indexes it, runs the lexical-only pass, attaches `cards.json` if present, embeds if the model is ready, and reports **success@k** (any expected section in the top k; expectations are alternatives) and **MRR@k**, plus misses. `--record` re-summarizes with the configured backend and rewrites `cards.json` (~$0.31 on Haiku). The plan calls the metric recall@5; the harness names it precisely.

## 5. Measured numbers to quote (Apple M3, 24 GB, macOS 15)

| What | Number |
|---|---|
| Parse + raw index, full `docs/` (119 sections) | 28 ms |
| Daemon: save → raw-searchable | 1.27 s (1 s of it is the debounce) |
| Daemon: save → card attached (Haiku via `api`) | 4.57 s; 5.06 s after the review fixes |
| Daemon backfill of 61 sections at start | 60 s, $0.28, two rounds |
| Rename of a 3-section file | 0 model calls, `created_at` kept, detected 0.2 s after the new path was indexed |
| API backend, 10 sections | 15 s wall, 1 turn each, $0.043 |
| Golden set, lexical raw only | success@5 0.883, MRR@5 0.747, 0.8 ms/query |
| Golden set, lexical cards + raw | 0.900 / 0.777, 2.2 ms |
| Golden set, hybrid | **0.983 / 0.853**, 25–60 ms/query in-process (the query embedding) |
| `mda search` as a cold process | lexical 20 ms; hybrid 257 ms (≈ 200 ms model load) |
| Model download + 162 cards embedded | 38 s |
| Recording 117 golden cards with Haiku | 45 s, $0.31 |
| Windows CI test job | 6–24 min (ONNX Runtime / aws-lc-sys compile); everything else under 10 min |
| Session spend on the API | ≈ $1.30 |

The plan's exit criterion "hybrid p50 < 30 ms" is **not met** (lexical is); recorded in `docs/benchmarks.md` and the Phase 2 plan, not hidden.

## 6. Recipes that worked

```bash
# toolchain and gate
source "$HOME/.cargo/env"; make check            # fmt, clippy pedantic -D warnings, nextest (261), cargo-deny, rustdoc
cargo nextest run -p mda-core -E 'test(daemon)'  # daemon e2e with the mock backend, ~1 s, real watcher + socket
cargo nextest run -p mda-cli --test mcp_cli      # rmcp client drives the real binary
cargo check --workspace --no-default-features    # lexical-only build (no fastembed/ort)
cargo build --release -p mda-cli                 # ~8 min cold (ONNX Runtime download + LTO); target/release/mda ≈ 9 MB + ORT
cargo insta test --accept --workspace --all-features   # after changing a snapshot
cargo run -q -- schema section > prompts/section.schema.v1.json   # a test keeps it in sync

# live runs (always on a scratch COPY of docs/, never the repo's docs/ directly)
set -a; source ~/.config/markdownattractor/env; set +a
export MDA_MODEL_DIR=$HOME/.cache/markdownattractor/models
S=/private/tmp/…/scratchpad; cp -R docs "$S/live-docs"
mda backend api --root "$S/live-docs"; mda start --root "$S/live-docs"
mda --json watch --root "$S/live-docs" | while IFS= read -r l; do printf '%s %s\n' "$(python3 -c 'import time;print(f"{time.time():.3f}")')" "$l"; done > watch.log
mda status --root … ; mda search "…" -k 3 --root … ; mda explain "…" --root … ; mda stale --root … ; mda stop --root …
mda rebuild --embeddings --root …                # downloads the model once, embeds every card
mda eval --golden evals/golden -k 5              # lexical + hybrid runs; cards.json is checked in
mda eval --golden evals/golden --record          # re-record cards (spends money)
sqlite3 "$S/live-docs/.markdownattractor/index.sqlite" "select model, outcome, count(*), round(sum(cost_usd),4) from usage_log group by 1,2;"

# GitHub
gh pr create … ; gh pr checks 6 ; gh run view <run-id>        # read the JOB LIST, not the column count
gh api repos/joe-carr-data/markdownattractor/actions/jobs/<job-id>/logs | sed -E 's/\x1b\[[0-9;]*m//g' | grep -nE "FAIL|panicked|TIMEOUT"
gh run watch <run-id> --exit-status                           # in the background; Windows takes 6–24 min
gh pr merge <n> --merge --delete-branch=false

# Codex review (process: .claude/skills/codex-review/SKILL.md)
Agent(subagent_type="codex:codex-rescue", prompt="--fresh --model gpt-6-astra <packet pinned to a SHA, reviewer prompt verbatim>")
# then docs/reviews/codex/YYYY-MM-DD-<scope>.md with the triage table; fix accepted findings the same day

# plugin
claude plugin validate .            # validates .claude-plugin/marketplace.json
scripts/dev/gen-index.sh            # regenerate docs/index.md (the PostToolUse hook runs it on Write/Edit under docs/)
shellcheck -S warning scripts/*.sh scripts/dev/*.sh scripts/mda
```

Working style that worked: write the phase plan with module contracts and exit criteria first; ADR before any new crate; implement core → CLI → tests → docs; `make check` before every commit; small conventional commits with the footer; Codex review pinned to the branch head, triage every finding (accept / accept in part / reject with reason / defer), fix the same day; update STATUS, aha, index, design docs and CHANGELOG before ending a task.

Editing files with heredoc/Python patches from Bash: **always run `cargo fmt` first and patch against the formatted text**; a patch script whose anchor mismatches must report which block failed rather than abort the rest (see `scratchpad/patch.py` pattern: per-block writes, failure list). Three multi-block scripts silently half-applied this session because of that.

## 7. Caveats and gotchas (all sessions, deduplicated)

- `claude -p` waits 3 s for stdin when not a TTY: always write and close. Content starting with `-` breaks argument delivery (use stdin). `subtype` in its result JSON is unreliable.
- Haiku via the CLI often needs a reminder turn (2× input tokens); the API's `output_config.format` is single-turn. schemars emits unit enums as `oneOf`, which the API rejects: `card::simplify_enums`.
- `Outcome::stops_pool()` decides by reason **prefix** (`FATAL_NOT_LOGGED_IN`, `FATAL_BAD_MODEL`, `FATAL_NO_API_KEY`, `FATAL_LOCAL_DOWN`, `FATAL_ACCOUNT`, `FATAL_BACKEND_UNAVAILABLE`). Keep prefixes stable. `JobResult::was_stopped()` is what the pipeline defers on.
- Grounding drops entities/dates the model did not quote verbatim (after quote/dash/whitespace folding); `iso` must match `precision` and the year in `raw`.
- `Config` uses `deny_unknown_fields`; add fields with defaults. New fields this session: `embeddings` (`local-small`|`off`), `embedding_cache_dir`.
- Linux `notify` reports opens; the watcher drops `EventKind::Access` or the daemon feeds itself. macOS FSEvents never showed it.
- SQLite deferred transactions that read then write fail with `SQLITE_BUSY_SNAPSHOT` regardless of the busy timeout; every transaction is `BEGIN IMMEDIATE`.
- A per-call `BufReader` over a socket drops buffered lines; the IPC keeps one framed reader per connection.
- Building a fresh worker pool per daemon round resets AIMD; the round's final concurrency is carried (`SummarizeOptions.initial_concurrency`).
- `classify()` looks at the entry, not the name (a directory called `notes.md` is a directory); a vanished non-markdown path is a rescan.
- Renames inside a moved directory are only matched when old and new hints land in the same debounce batch; otherwise delete + create (cards still reused; `created_at` continuity lost).
- Windows: `Command` spawns with `bInheritHandles = TRUE`; the daemon spawn clears the inherit flag first (PR #5). No `timeout` binary on macOS: the harness's own tool timeout is the only bound (`timeout 600 …` silently prints nothing).
- zsh does not word-split unquoted variables; measurement scripts are `bash`.
- `use crate::Result` in the file that carries `#[tool_handler]` breaks the rmcp macro; `mcp.rs` writes `crate::Result<T>`. rustdoc `-D warnings` rejects doc links to private items.
- fastembed's default features pull `native-tls` (banned by `deny.toml`); the workspace uses `ort-download-binaries-rustls-tls` + `hf-hub-rustls-tls`. `paste` (via `tokenizers`) is an ignored unmaintained advisory in both `deny.toml` and `audit.yml`.
- `is_cached` requires `model_optimized.onnx`, `tokenizer.json`, `config.json`, `tokenizer_config.json` under `snapshots/*`; an interrupted download is not "ready".
- `mda embeddings off` only saves config; a running daemon keeps its embedder until `mda restart` (the command says so and reports `needs_restart`).
- `mda index` while a daemon runs delegates over the socket; `--limit` is ignored then; `--retry-failed` still runs locally first.
- `mda stale` exits 1 when anything is stale (scriptable); `unreadable` means "could not check", never "fresh".
- Vectors and cards share the section-hash key, so a hash shared by several documents carries one document's title/heading context (Codex F9, rejected with reason; documented in `design/search.md`).
- `interprocess` local sockets need `use interprocess::local_socket::tokio::prelude::*` for the traits; the socket file must be chmod'ed after `create_tokio`.
- Integration test crates need `#![allow(clippy::expect_used, clippy::unwrap_used)]` at the top; `clippy::too_many_lines` is allowed once for the daemon CLI scenario test.
- Machine: Apple M3, 24 GB; `llama-server` may still be installed (`brew`) but was not running at the end of this session.

## 8. Next steps, in order

1. **Watch the plugin load in Claude Code** once: open a session in a project with a `.markdownattractor/` index (or run `mda start` first), `/reload-plugins`, confirm the SessionStart hook started the daemon (`mda status`) and call `mcp__plugin_markdownattractor_markdownattractor__mda_search`. Fix whatever that shows (root resolution via `MDA_ROOT`, model dir via `MDA_MODEL_DIR`, stdout hygiene).
2. **First-run UX** (plan §9.5, `docs/project-plan.md` lines ~506–511): `mda start` on a fresh root runs one example query when the first ~10 cards land (pick from a card's `questions_answered`) and prints the hit with its line range; `mda cost [--since]` from `usage_log`; `mda diagnostics`; `mda nudge on|off` writing `${CLAUDE_PLUGIN_DATA}/nudge.off` (the nudge hook already honours it). Write `docs/plans/2026-09-phase4-launch.md` first (goal, tasks, exit criteria).
3. **Release pipeline** (Phase 4): `cargo-dist` (ADR needed), GitHub Releases with `mda-<os>-<arch>.tar.gz` and `SHA256SUMS` exactly as `scripts/bootstrap.sh` expects, macOS notarisation, a lexical-only build for Intel macOS (`--no-default-features`), bump `VERSION` + `plugin.json` + `marketplace.json` together, then decide on making the repo public.
4. **A/B parity protocol** (plan §11): same questions with and without the index through `claude -p`, Sonnet-graded; only then claim token savings in the README.
5. Follow-ups carried in the review files: peer authentication on the daemon socket; live config reload in daemon/MCP; time-filter cases with fixed timestamps in the golden set; mismatched-dimension vector rows repaired on rebuild; hybrid latency lever (query encoder, ONNX threads, query-vector cache) if 50 ms matters; a Codex review of the review fixes themselves has not been run.
6. Phase 3 (relationships, `relates_to`, wiki) stays deferred per the plan.

## 9. Status of every plan item

- Phase 0 spike: done (`docs/plans/2026-09-phase0-spike.md`).
- Phase 1 engine: rows 1–13 done; exit criteria all ticked; Codex crates review 14/14 fixed.
- Phase 1 daemon: all tasks and exit criteria ticked; live numbers in `design/daemon.md`; Codex daemon review 16 findings: 15 fixed, 1 in part (socket peer auth deferred).
- Phase 2 search: all tasks ticked; exit criteria ticked except hybrid latency (recorded as not met); Codex Phase 2 review 17 findings: 14 fixed, 2 in part (F8 restart note, F10 mtime gate), 1 rejected with reason (F9), 1 deferred (F14).
- Plan §13 open decisions: default embedding model → bge-small (ADR-0004); `cards/` commit → open; MCP subcommand vs binary → subcommand (ADR-0004); per-root vs global daemon → per-root (ADR-0003).
- Backlog: B-0001 document-to-markdown ingestion (anydoc/anytomd/kreuzberg), B-0002 in-session backend.
- Owner items: API key rotation — **declined, no action**; repo visibility — private until the owner says otherwise.
