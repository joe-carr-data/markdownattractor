# Handoff — 2026-09-21/22 session (project bootstrap through Phase 1 engine)

Written by Claude (Fable 5.1) at the end of the first build session, for the next session after compaction. **Read this whole file first, then the files in §2 in the order given.** Everything below was verified in-session unless marked otherwise.

Claude Code session: https://claude.ai/code/session_016dTZj7CmBSJeFLDCwUCxEF

## 0. One-paragraph state

markdownattractor is a Rust Claude Code plugin that indexes a folder of markdown into heading-delimited sections, makes them raw-text searchable in milliseconds (SQLite FTS5), summarizes each section once into a structured "card" through a model backend, validates the card with evidence grounding, and serves hybrid search (`mda search`) with exact line ranges (`mda open`). **Phase 0 (spike) is done. Phase 1 (engine + CLI) is done except the daemon/watcher.** All three summarization backends work live: the Claude Messages API with the user's own key (default), a local llama.cpp model, and an opt-in `claude -p` spawn. Two Codex reviews have been done and fully triaged. 216 tests, clippy pedantic clean, CI configured, repo public-ready but currently **private** at github.com/joe-carr-data/markdownattractor, branch `main`, HEAD `442fe04`. No PRs, no issues, no other branches.

## 1. Repo, branches, tickets

| Item | State |
|---|---|
| Repo | https://github.com/joe-carr-data/markdownattractor (private; `gh repo edit --visibility public` when ready) |
| Local clone | `/Users/jcarr/markdownattractor`, remote `origin` over HTTPS (SSH fails from the sandbox) |
| Branch | `main` only; everything is committed and pushed as of `442fe04`; working tree clean |
| CI | `.github/workflows/ci.yml` (fmt, clippy `-D warnings`, nextest on 3 OSes, MSRV 1.88, 70% coverage gate, cargo-deny, rustdoc, shellcheck) and `audit.yml` (weekly). **Never run on GitHub yet** — first push happened before the workflow existed and later pushes have not been checked. Verify at https://github.com/joe-carr-data/markdownattractor/actions early next session. |
| Tickets | No issue tracker used. Work items live in `docs/STATUS.md` (Next, max 3), `docs/plans/*.md` (checkboxes), `docs/backlog.md` (B-0001 document ingestion, B-0002 in-session backend). |
| Codex threads | Pre-mortem review: thread `01a0c558-3d54-7fb2-920a-cdc40379dcf6` (turn `01a0c558-4040-7963-a6f1-68074fff381c`), model `gpt-6-astra`. Crates review: thread `01a0c5bd-d175-7150-9385-4e49b1598d2e` (turn `01a0c5bd-d504-7241-80ca-7c91b141c315`), pinned to `b0814de`. Codex CLI 0.155.1, invoked via the `codex:codex-rescue` subagent with `--fresh --model gpt-6-astra`; `~/.codex/config.toml` defaults to `gpt-6-astra`. |

## 2. Files to re-read, in order (T0 → T2)

1. `CLAUDE.md` — charter, six goals, ten golden rules, orientation order.
2. `docs/STATUS.md` — phase, active plan, done/next/blockers. Keep it under one screen; update at the end of every task.
3. `docs/plans/2026-09-phase1-engine.md` — module contracts (rows 1–13), exit criteria (all ticked except the daemon row), decisions.
4. `docs/aha.md` — ~25 dated lessons; the last 15 are from this session and explain most non-obvious code.
5. `docs/adr/0001-worker-backend-claude-cli.md` (+ amendments) and `docs/adr/0002-backends-and-login-policy.md` — why the backends are what they are.
6. `docs/design/summarization.md`, `docs/design/search.md` — the engine as built, with measured numbers.
7. `docs/reviews/codex/2026-09-21-pre-mortem.md`, `docs/reviews/codex/2026-09-22-crates.md` — findings, triage, resulting rules.
8. `docs/project-plan.md` — the full plan (T2; read sections, not the whole 60 KB). Most-updated sections: §1 G1, §2.4, §4.1, §4.2 (worker call shape + guardrail table), §7, §8, §9.5, §11, §12.
9. `docs/guides/local-model.md`, `docs/backlog.md`, `docs/plans/2026-09-phase0-spike.md`.
10. Code, in pipeline order: `crates/mda-core/src/lib.rs` (module map) → `markdown/mod.rs` → `card.rs` → `config.rs` → `walk.rs` → `store/mod.rs` → `diff.rs` → `planner.rs` → `worker/{mod,parse,claude_cli,api,local,http,pool,mock}.rs` → `validate.rs` → `pipeline.rs` → `search.rs` → `crates/mda-cli/src/{main,output}.rs` + `commands/*.rs`.
11. Plugin surface: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`, `skills/mda/SKILL.md`, `skills/search-first/SKILL.md`, `hooks/hooks.json`, `scripts/{bootstrap.sh,mda,nudge.sh}`, `prompts/section.v2.txt`, `prompts/section.schema.v1.json`.
12. Dev workflow: `.claude/settings.json` (hooks), `.claude/rules/*.md`, `.claude/skills/{handoff,resume,adr,codex-review}/SKILL.md`, `scripts/dev/*.sh`.

## 3. Architecture as built

```
walk::discover ─► markdown::parse_str ─► store.upsert_document ─► sections_raw_fts  (searchable in ms)
                                                 │ new section hashes (summaries table, state=pending)
                                                 ▼
   store.pending_hashes ─► pipeline (heading-only → deterministic card)
                        ─► planner::chunk_text (≤ 6K tokens, cut at paragraph outside fences)
                        ─► worker::Pool (AIMD concurrency, retries, escalation, backoff, admission)
                              └► Backend: api | local | claude-cli   (worker::backend_for(&Config))
                        ─► validate::validate (caps, dedupe, entity + date grounding, iso normalisation)
                        ─► store.attach_summary (by hash; cards_fts) + store.record_usage (ledger)
search::search: fts_escape → cards_fts + sections_raw_fts (bm25) → filters → RRF → recency → top-k; OR fallback
pipeline::Engine::open_section: re-parse file, compare doc hash, re-index if changed, resolve section by hash→heading→index, return current lines + stale flag + current id
```

Key invariants (also in `CLAUDE.md` and `.claude/rules/rust.md`):
- Summaries are keyed by **section hash** (blake3 of normalised text), not by position; moved/duplicated sections never cost a call. `doc_id` = first 16 hex of blake3(rel_path); `section_id` = `{doc_id}#{index}`.
- Nothing writes into the watched root; state lives in `<root>/.markdownattractor/` (`config.toml`, `index.sqlite`). `.markdownattractor/` is gitignored at any depth.
- Every filesystem path goes through `Engine::rel_path` / `Engine::safe_join` (symlink-safe root containment).
- Every model attempt is logged to `usage_log` regardless of outcome; the daily budget reads that ledger.
- "Not seen this round" ≠ deleted: tombstone only when the file is actually missing.
- Worker child processes: write stdin, drain stdout+stderr, wait — concurrently, under a timeout.
- The user message to any backend is `<section-<nonce> path=… heading=…>…</section-<nonce>>` (`worker::user_message`), nonce = keyed blake3 with a per-process secret; the prompt says everything inside is data.

Store schema v2 (`store/mod.rs` `SCHEMA_V1`, `SCHEMA_V2`): `meta`, `docs`, `summaries` (hash PK, state pending|summarized|failed, summary JSON, provenance JSON, usage cols), `sections` (FK docs, FK summaries), `events`, `usage_log`, FTS5 `sections_raw_fts`, `cards_fts`. Timestamps are RFC 3339 UTC text with fixed 9-digit fractions (`fmt_ts`). Migrations: add an entry to `MIGRATIONS`.

## 4. Backends (ADR-0002) — the most important decision of the session

- **Policy fact** (Claude Code *Legal and compliance* page, verbatim): "Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their users." The Agent SDK overview says the same. The original plan (spawn `claude -p` by default) was withdrawn on that basis. Contact route for approval, if ever wanted: https://www.anthropic.com/contact-sales (no email exists).
- **`api` (default)**: `POST {api_base_url}/v1/messages`, headers `x-api-key`, `anthropic-version: 2023-06-01`, and `anthropic-workspace-id` when configured (`api_workspace_id` in config or `$ANTHROPIC_WORKSPACE_ID`; required for keys not scoped to a workspace). Body: `system` with `cache_control: ephemeral`, one user message, `output_config.format = {type: json_schema, schema}` → single-turn structured output. Haiku `claude-haiku-4-5`, escalation `claude-sonnet-5` with `output_config.effort: low`. Cost from `PRICES_PER_MTOK` in `worker/api.rs` (cache write 1.25×, cache read 0.1×). **Live-verified**: 10/10 cards, 1 turn each, 15 s wall, $0.043 on `docs/plans`. Schema caveat: schemars emits unit enums as `oneOf`; `card::simplify_enums` rewrites them to `enum` — the API rejects `oneOf`.
- **`local`**: OpenAI-compatible `POST {local_base_url}/chat/completions` with `response_format.json_schema.strict`, `chat_template_kwargs.reasoning_effort` (gpt-oss), cost 0. Pool bounds are forced to start 2 / cap 4 (`Config::pool_bounds`), timeout ≥ 300 s (`Config::effective_worker_timeout`). **Live-verified** with llama.cpp 0.4.1: gpt-oss-20b (`-hf ggml-org/gpt-oss-20b-GGUF`) 9/9 good cards when memory-resident (166 tok/s decode), Gemma 3 4B (`-hf ggml-org/gemma-3-4b-it-GGUF`) usable. On this machine both were slow (6–9 tok/s) because the Mac was swapping (Docker VM + Chrome); gpt-oss with `--ctx-size 0 -np 4` threw Metal errors under that pressure. The user wants the guide to keep full context (`--ctx-size 0`) as the general setting; the guide also explains that 16K/slot covers everything the planner sends.
- **`claude-cli`**: kept, opt-in only, requires `claude_cli_policy_ack = true` (config validation errors with `CLAUDE_CLI_POLICY` text otherwise); `mda backend claude-cli --i-accept-the-policy`. Call shape: `MAX_THINKING_TOKENS=0 MARKDOWNATTRACTOR_WORKER=1 claude -p --model … --system-prompt … --output-format json --json-schema … --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence --max-budget-usd …`, section over stdin, stdin closed immediately. Spike facts: thinking off is 10× on latency; `subtype` in the result JSON is unreliable; structured output is a tool call and needs the protocol line; a section that *looks* like instructions confuses Haiku unless delimited.
- **Secrets on this machine**: `~/.config/markdownattractor/env` (mode 600) holds `ANTHROPIC_API_KEY` and `ANTHROPIC_WORKSPACE_ID=wrkspc_<redacted>` (the other id the user gave, `wrkspc_<redacted>`, returns "workspace not found" for this key). Load with `set -a; source ~/.config/markdownattractor/env; set +a`. **Never print or commit the value.** The key passed through the chat transcript once; the user should rotate it. Memory note: `~/.claude/projects/-Users-jcarr-markdownattractor/memory/anthropic-api-key-location.md`.
- The auto-mode permission classifier blocks reading `.env` files (credential exploration/materialisation) — don't try; ask the user.

## 5. Recipes that worked

```bash
# toolchain
source "$HOME/.cargo/env"            # rustup stable 1.98.1; cargo-nextest, cargo-deny, cargo-llvm-cov, cargo-insta, cargo-binstall installed
make check                            # fmt + clippy(-D warnings, pedantic) + nextest + deny + rustdoc(-D warnings)
cargo insta test --accept --workspace --all-features    # first run after changing a snapshot
cargo llvm-cov nextest --workspace --all-features --summary-only   # coverage (was 84%)
cargo run -q -- schema section > prompts/section.schema.v1.json     # regenerate the checked-in schema (a test enforces sync)

# live runs (release binary at target/release/mda, ~8 MB)
set -a; source ~/.config/markdownattractor/env; set +a
MDA_LIVE_API=1   cargo nextest run -p mda-core -E 'test(live_api)'   --run-ignored ignored-only
MDA_LIVE_LOCAL=1 cargo nextest run -p mda-core -E 'test(live_local)' --run-ignored ignored-only
MDA_LIVE_TESTS=1 cargo nextest run -p mda-core -E 'test(live_claude)' --run-ignored ignored-only   # claude-cli
cd docs/plans && ../../target/release/mda backend api --root . && ../../target/release/mda index --root . --retry-failed
../../target/release/mda --json status --root . ; mda search "…" -k 3 ; mda open <section_id> ; mda card <section_id> ; mda doctor
sqlite3 .markdownattractor/index.sqlite "select model, outcome, count(*), sum(input_tokens), sum(output_tokens), round(sum(cost_usd),4) from usage_log group by 1,2;"

# local model server
brew install llama.cpp
llama-server -hf ggml-org/gpt-oss-20b-GGUF --ctx-size 0 --jinja -fa on -b 2048 -ub 2048 -np 4 --port 8080   # model cached under ~/.cache/huggingface/hub/
curl -s http://127.0.0.1:8080/health

# Codex review (process in .claude/skills/codex-review/SKILL.md and docs/project-plan.md §17.5)
Agent(subagent_type="codex:codex-rescue", prompt="--fresh --model gpt-6-astra <review prompt, pinned to a commit SHA>")
# then triage every finding accept/reject/defer into docs/reviews/codex/YYYY-MM-DD-<scope>.md and fix accepted ones

# plugin
claude plugin validate .            # validates .claude-plugin/marketplace.json
scripts/dev/gen-index.sh            # regenerate docs/index.md (also runs from the PostToolUse hook)
shellcheck -S warning scripts/*.sh scripts/dev/*.sh scripts/mda
```

Working style that worked: write the phase plan with module contracts first; fan out independent modules to `general-purpose` subagents with the exact API in the prompt and "touch only these files"; keep integration modules (pipeline, search, CLI) for the main session; run the full gate after every merge; commit in small conventional commits with the attribution footer; run Codex on the result and fix everything accepted the same day.

## 6. Caveats and gotchas

- `claude -p` waits 3 s for stdin when not a TTY — always write and close. Content starting with `-` breaks argument delivery (use stdin).
- Haiku 4.5 via the CLI often writes JSON as text then needs a "reminder" turn (2× input tokens). The API's `output_config.format` avoids this entirely.
- `Outcome::stops_pool()` decides pool shutdown by reason **prefix** (`FATAL_NOT_LOGGED_IN`, `FATAL_BAD_MODEL`, `FATAL_NO_API_KEY`, `FATAL_LOCAL_DOWN`, `FATAL_ACCOUNT`). Keep prefixes stable.
- Jobs reported with `attempts == 0` (pool stopped, cancelled) are **deferred**, not failed; `mda index` picks them up next run. Failed ones need `--retry-failed`.
- `mda open` returns the section's **current** id (`section_id`) plus `requested_id`; staleness is decided on the document hash, and opening a changed file re-indexes it as a side effect.
- Grounding drops entities/dates the model didn't quote verbatim (after quote/dash/whitespace folding); `iso` must match `precision` (day = real calendar date; month = YYYY-MM; year/quarter = YYYY) and the year written in `raw`. Timestamp-style isos are normalised, not rejected.
- `Config` uses `deny_unknown_fields`; add new fields with defaults. `Config::load` errors if `backend = claude-cli` without the ack (the `mda backend` command loads leniently to allow switching away).
- Store writes are conditional: `attach_summary` never overwrites a `summarized` row; `mark_failed` only touches `pending`. There is no job lease; the daemon is intended to be the single writer.
- Search filters (`--since/--until/--in`) apply before candidate cuts and before the OR-fallback decision; with filters the per-index fetch depth is ≥ 200.
- `deny.toml` allows `CDLA-Permissive-2.0` (webpki root certs pulled by reqwest/rustls).
- Integration test crates need `#![allow(clippy::expect_used, clippy::unwrap_used)]` at the top (clippy.toml's test allowances don't reach them).
- `std::env::set_var` is unsafe in edition 2024 and `unsafe_code = "forbid"`: tests inject keys via `ApiBackend::with_key`, never env mutation.
- The nudge hook (`scripts/nudge.sh`, PreToolUse on Read|Grep|Glob) only fires for markdown targets when `<project>/.markdownattractor/index.sqlite` exists; off switch is `${CLAUDE_PLUGIN_DATA}/nudge.off`.
- `.claude/settings.json` hooks in this repo are for *developing* markdownattractor (STATUS/handoff/index upkeep); the plugin's own hooks are in `hooks/hooks.json`.
- Machine notes: Apple M3, 24 GB, macOS 15; the machine was heavily swapped during local-model tests (Docker VM + Chrome). llama-server may still be running on :8080 (Gemma 3 4B) — `pkill -f "llama-server -hf"` if needed.

## 7. Measured numbers to quote

| What | Number |
|---|---|
| Parse + raw index, full `docs/` (10 files, 119 sections) | 28 ms |
| API backend, `docs/plans` (10 sections) | 15 s wall, 1 turn/call, 24K in / 3.9K out tokens, $0.043 |
| claude-cli backend, same sections | 32 s wall, 2 turns/call |
| claude-cli backend, full `docs/` (113 calls) | 1 min 53 s, AIMD 4→16, 0 failures, $0.63 list |
| Local gpt-oss-20b, resident | ≈ 160 tok/s decode; ≈ 40 s/section at 2–4 slots on a swapping machine |
| Coverage | 84% lines (before the last few commits) |

## 8. Next steps, in order

1. **Check CI on GitHub** (Actions tab). Fix whatever the three-OS matrix, coverage gate, or shellcheck job reports; Windows has never been tried (`interprocess`, path handling, `/bin/sh`-based tests are `#[cfg(unix)]`).
2. **Daemon + watcher** (plan row 13, last Phase 1 item): `notify` 8.x watcher → debounce 1–2 s + size-stable check → `Engine::index_file`; priority queue (user-edited files before backfill, smallest first); `mda start|stop|restart|watch`; Unix socket (`interprocess` 2) for `status`; rename detection (delete+create with same content hash → keep `doc_id`, `moved_from`); PID/socket files under `.markdownattractor/`. Write `docs/plans/2026-09-phase1-daemon.md` first. Decide §13 open question "per-root vs global daemon" (lean: per-root).
3. **Phase 2**: `sqlite-vec` 0.1.x (pins rusqlite ^0.31 — conflict with our 0.40; may need vendoring, see plan §16.3) + `fastembed` 7 (`BGESmallENV15Q`) as a third RRF list; MCP server (`rmcp` 3.x, stdio, `mda mcp`) with `mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_status`; `.mcp.json` in the plugin; `mda timeline|recent|stale|explain`; eval harness (`mda eval`) with the 30-doc golden set and the answer-quality parity gate (plan §11). Check the grounding strictness question (162 entities / 38 dates dropped on the full docs run).
4. **Prompt/cost experiment** for the claude-cli backend only (the API is already single-turn): get Haiku to call the tool on turn one.
5. **First-run UX** (plan §9.5): `mda start` with one confirmation, example query at the end; `mda cost`, `mda diagnostics`; `mda nudge on|off` writing `${CLAUDE_PLUGIN_DATA}/nudge.off`.
6. **Release pipeline** (Phase 4): `dist` (cargo-dist) with GitHub Releases + `SHA256SUMS` that `scripts/bootstrap.sh` expects (`mda-<os>-<arch>.tar.gz` containing `mda`); notarisation; bump `VERSION` + `plugin.json` + `marketplace.json` together.
7. Owner items: rotate the API key; decide when to make the repo public; design partners (issue template exists).

## 9. Status of every plan item

- Phase 0 spike: **done**, all exit criteria closed (`docs/plans/2026-09-phase0-spike.md`).
- Phase 1 engine: rows 1–12 **done** and live-verified; row 13 (daemon) **open**. Exit criteria: all ticked except none pending; Codex review filed and fixed.
- Codex pre-mortem (11 findings): 9 accepted, 1 partial, 2 rejected — all folded into the plan.
- Codex crates review (14 findings): 14 accepted, all fixed (`docs/reviews/codex/2026-09-22-crates.md`), 5 suggested tests added.
- Backlog: B-0001 document ingestion (anydoc/anytomd/kreuzberg; exit criterion: Phase 2 green + a design partner needing it), B-0002 in-session backend.
- Open decisions (plan §13): default embedding model, commit `cards/` or not, MCP as subcommand vs separate binary, per-root vs global daemon.
