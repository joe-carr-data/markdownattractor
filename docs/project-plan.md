# markdownattractor — Project Plan

> A Claude Code plugin that turns every folder of markdown into a time-aware, searchable knowledge layer Claude reads *before* it reads your files.
>
> Status: v0 design · Date: 2026-09-21 · Owner: Joe · Reviewed: [Codex pre-mortem 2026-09-21](reviews/codex/2026-09-21-pre-mortem.md)

---

## 0. One-paragraph pitch

Claude Code reads files whole. On real projects that means burning tens of thousands of tokens to find one paragraph. **markdownattractor** runs an ultrafast Rust daemon that watches a folder, summarizes every markdown file section-by-section with the user's *existing* Claude Code login (no API key), and builds a local hybrid search index (BM25 + vectors + time). Claude searches the index, reads a 200-token card, and opens only the 40 lines it needs. Every fact in the index carries provenance: which file, which section, which lines, and *when* it was created, changed, and last seen.

---

## 1. Goals and non-goals

### Goals

| # | Goal | Success metric |
|---|---|---|
| G1 | **Speed** — new file searchable in seconds | p50 < 15 s from save to card available (Haiku, ≤ 10K-token doc) |
| G2 | **Zero-config** — `/markdownattractor start` and forget | No API key, no model download prompt blocking first use |
| G3 | **Token economy** — Claude reads less | ≥ 5× fewer source tokens read per answer on the eval set vs. plain `Read`/`Grep` |
| G4 | **Temporal provenance** — every hit is dated | 100% of cards have `created_at`, `updated_at`, `first_seen_at`; sections have their own `updated_at` |
| G5 | **Searchability** — hybrid search that beats grep | Recall@5 ≥ 0.85 on the golden query set; time-filtered queries (“what changed since Monday”) work |
| G6 | **Trust** — never hallucinated metadata | 100% of extracted dates/entities grounded in source text (evidence check) |

### Non-goals (v1)

- Not a note-taking app, wiki editor, or Obsidian replacement.
- No cloud sync, no hosted service, no telemetry.
- No non-markdown formats (PDF/docx) — the chunker is markdown-native. Plugins for other formats later.
- Does not write into the user's source files. Ever.

---

## 2. Landscape (what exists, what we do differently)

All of these attack the same root problem — the "discovery tax" Claude pays when it greps and reads raw files — but none combines a live watcher, LLM section cards, hybrid search and a temporal model for **prose**.

### 2.1 Knowledge-graph tools (the serious competition)

| Project | What it does | Where it stops |
|---|---|---|
| **graphify** (safishamsi, Python skill) | `/graphify .` on any folder — code, PDFs, markdown, images — builds a knowledge graph with Claude; claims ~71× fewer tokens per query; every edge tagged **EXTRACTED / INFERRED (confidence) / AMBIGUOUS**; Leiden communities; *no vectors* — LLM-extracted `semantically_similar_to` edges are the similarity signal. Derivative setups add a PreToolUse hook (“consult the graph before raw files”) and rebuild on git commit / daily | Batch rebuilds, not live; no time model; provider API keys (Gemini→Kimi→Claude→…→Ollama); Python; a full LLM pass over the corpus on every rebuild |
| **CodeGraph** (colbymchenry, npm) | Deterministic tree-sitter symbol graph in SQLite + FTS5; two MCP tools (`codegraph_context`, `codegraph_explore`); file watcher auto-sync; rigorous headless `claude -p` A/B benchmarks (~35% cheaper, 59% fewer tokens, 70% fewer tool calls across 7 repos) | Code only — AST has nothing to say about prose; no summaries; gains narrow on small repos (their own caveat) |
| **Understand Anything** (Lum1104, plugin) | Multi-agent pipeline extracts functions/classes, maps layers, builds guided tours; React dashboard with graph canvas, fuzzy + semantic search, chat; `/understand-diff` for uncommitted changes | Code onboarding tool; one-shot `/understand` run, 3 concurrent agents; not a background index |

### 2.2 Markdown / memory search tools

| Project | What it does | Gap we fill |
|---|---|---|
| **qmd** (tobi) | BM25 + vector + LLM rerank over markdown, local GGUF models, MCP server | Raw-chunk retrieval only, no LLM summaries, no background watcher, no temporal model, Node/Bun runtime |
| **claudix** | Rust binary; indexes a *code* repo, embeds, grep interception | Code-focused, no summarization layer, no time-awareness |
| **claude-vault** | Knowledge graph of notes; FTS5 + nomic embeddings + RRF; hook-enforced capture | Its own vault format; a minute-long blocking embed build on first search; Python |
| **claude-context-local** | Local code search MCP with EmbeddingGemma | Code, not docs; heavy model download (~1.2 GB) |
| **llm-wiki-plugin** | Karpathy “LLM wiki” pattern as a plugin | Agent-driven wiki maintenance, not a fast index over *existing* docs |

### 2.3 Head-to-head

| | graphify | CodeGraph | Understand Anything | markdownattractor |
|---|---|---|---|---|
| Target | code + docs + media | code only | code only | markdown only |
| Freshness | rebuild on commit / daily | live watcher | manual re-run | live watcher, **section-level incremental** |
| Time model | none | none | none | **4 clocks, section-level `updated_at`, tombstones** |
| Retrieval | graph traversal, no vectors | FTS5 symbol lookup | fuzzy + semantic | hybrid BM25 + vectors + recency |
| Summaries | LLM concept extraction | none (deterministic) | LLM file summaries | LLM section cards with `questions_answered` |
| Read path | “consult graph first” hook | 2–4 MCP calls, zero reads | dashboard / chat | L0 → L1 → L2 → **exact line range** |
| Runtime | Python | Node | Node/React | single Rust binary |
| Auth | provider API keys | none | Claude Code session | Claude Code login, no key |

### 2.4 What we take from them

1. **Nobody does time.** None can answer “what changed since Monday”. Temporal provenance is the uncontested lane — lead the README with it. **Claim discipline:** v1 promises only what the stored data supports — *when* something was created, changed, or deleted, at section granularity. “Is this ADR still current” needs `superseded_by` and validity inference (Phase 3); do not put it in the README until it ships.
2. **graphify's “no vectors” bet is a real design fork.** Their argument: LLM-extracted edges + community detection beat embeddings for structural questions. Ours: a watcher-driven index needs incremental, millisecond-cheap similarity that doesn't require an LLM pass over the corpus on every change; card embeddings give exactly that. We don't pick a side — hybrid search stays the retrieval backbone, and we **adopt their EXTRACTED / INFERRED / AMBIGUOUS confidence tags** for phase-2 `relates_to` edges (explicit links = EXTRACTED, embedding neighbours = INFERRED with score, weak matches = AMBIGUOUS).
3. **CodeGraph sets the benchmark bar.** Headless `claude -p` A/B, same question with and without the index, 4 runs, medians, 7 corpora. §11 copies that methodology and publishes comparable numbers (tokens read, tool calls, wall-clock, cost), choosing corpora large enough that the gains show.
4. **Agents ignore MCP tools they aren't reminded of.** graphify's derivative setups use a PreToolUse hook; CodeGraph's guide has a whole “verify it's actually being used” section. We ship a **default-on `PreToolUse` nudge**: on `Grep`/`Read` of `*.md`, inject one line — “index available: try `mda_search` first”. `/mda nudge off` disables it. Cheap, and the single most effective adoption lever — shipping it opt-in would mean the plugin gets installed but barely used. `/mda status` reports the **index hit rate**: share of markdown lookups in the session that went through `mda_search` rather than raw `Grep`/`Read`.

### 2.5 Positioning

Don't compete on scope — graphify will always cover more file types. The pitch is: **“the fastest, always-fresh, time-aware layer for the markdown that surrounds your code — ADRs, runbooks, notes, specs.”** Interoperate rather than fight: a code index (CodeGraph, claudix) covers the code; markdownattractor covers the prose. Both can be installed side by side, and the skill tells Claude which to ask.

**markdownattractor's differentiators**

1. **Summaries, not just chunks.** LLM-written section cards with `questions_answered`, entities, decisions, dates — the things queries actually match.
2. **Progressive disclosure by design.** L0 → L1 → L2 → exact line range. Claude never reads a whole file by accident.
3. **Temporal provenance at section granularity** — filesystem, git, frontmatter, content dates, and change history, fused into one time model.
4. **Always fresh.** Background watcher with section-level incremental updates — no rebuild step, ever.
5. **No API key.** Uses the user's Claude Code login. Optional API-key fast path.
6. **One Rust binary** is watcher, summarizer orchestrator, index, MCP server and CLI. Sub-second startup, tiny footprint.
7. **Great UX** — a `/markdownattractor` command family with live status, cost visibility, and a doctor.

---

## 3. Architecture

```
┌────────────────────────── Claude Code session ──────────────────────────┐
│  /markdownattractor …  (skills)      MCP tools: mda_search, mda_card,   │
│                                      mda_timeline, mda_status …         │
└────────────┬────────────────────────────────┬──────────────────────────┘
             │ CLI (bin/mda)                  │ MCP (stdio)
┌────────────▼────────────────────────────────▼──────────────────────────┐
│                     mda daemon  (single Rust binary)                    │
│                                                                         │
│  Watcher ─► Intake ─► Parser ─► Section-diff ─► Planner ─► Worker pool │
│  (notify)  (debounce) (comrak)  (hash/section)  (chunks)   (claude -p) │
│                                                     │                   │
│                       Validator ◄───────────────────┘                   │
│                          │                                              │
│                       Reducer ─► Writer ─► Indexer ─► Event bus         │
│                                  (atomic)  (FTS5+vec)  (relates_to…)    │
│                                                                         │
│  Store: .markdownattractor/{index.sqlite, docs/*.json, cards/**.md}     │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Components

| Component | Tech | Notes |
|---|---|---|
| Watcher | `notify` crate (FSEvents/inotify) | Debounce 1–2 s + size-stable check; respects `.gitignore` + `.markdownattractorignore`; always ignores `.markdownattractor/` |
| Parser | `comrak` | Heading tree, line/byte ranges, frontmatter, links, code langs, tables, per-section token estimate; regex date pre-pass |
| Section diff | blake3 per section | Only changed sections go to the LLM; gives per-section `updated_at` |
| Planner | Rust | 2–6K-token chunks; merge tiny siblings, split large ones at paragraph boundaries, never inside code fences; docs < 3K tokens = single call |
| Worker pool | spawns `claude -p` | Warm pool, adaptive concurrency (AIMD), priority queue. See §4 |
| Validator | `serde` + evidence check | Schema check; every date/entity must have an `evidence` substring present in the section |
| Reducer | Rust + one LLM call | Doc card from section cards + deterministic metadata (never raw text) |
| Store | SQLite (WAL) | `docs`, `sections`, `jobs`, `events`, FTS5 tables, vector table |
| Indexer | FTS5 + `sqlite-vec` + `fastembed-rs` | See §5 |
| MCP server | `rmcp` (stdio) | Same binary: `mda mcp` |
| Event bus | in-process + `events` table | Hook point for phase 2 (`relates_to`, wiki) |

### 3.2 Layout on disk

```
<root>/.markdownattractor/
├── config.toml            # root, model, ignore rules, budgets, retention
├── index.sqlite           # state + FTS5 + vectors (derived, gitignored by default)
├── docs/<doc_id>.json     # full machine record (source of truth)
├── cards/<mirror/path>.md # human/Claude-readable cards (optionally committed)
├── models/                # embedding model cache (or ~/.cache/mda/models)
├── logs/                  # daemon.log (rotating)
└── daemon.pid / daemon.sock
```

`cards/` is the only thing that makes sense to commit; everything else is derived.

---

## 4. Summarization phase (the core)

### 4.1 Authentication

- **Default backend: `claude-cli`.** The daemon spawns the user's `claude -p`. This reuses the subscription login; summarization consumes the user's plan usage and the README says so explicitly.
- **Not bare mode by default.** `--bare` skips OAuth/keychain, so it only works with an API key.
- **Optional backend: `api`.** If `ANTHROPIC_API_KEY` (or `apiKeyHelper`) is present and the user opts in, use `claude --bare -p` (faster startup) or a direct Messages API client later.
- **Never** read OAuth tokens from the keychain or `~/.claude` directly.
- **In Phase 0 (spike exit criterion, not a launch-day check):** get written confirmation from Anthropic that spawning the user's own CLI from an installed plugin is acceptable under the third-party login policy. In parallel, write the **API-key-only pitch** (README variant, onboarding, cost story) so that if the answer is no, the fallback product is already positioned rather than improvised.

### 4.2 Worker call shape (to be confirmed by the spike)

```bash
claude -p \
  --model "$MDA_MODEL" \                          # default: haiku
  --system-prompt "$(cat prompts/section.txt)" \  # replaces Claude Code's default prompt
  --output-format json --json-schema "$SECTION_SCHEMA" \
  --tools "" --strict-mcp-config --setting-sources "" \
  < chunk.txt
# cwd = empty scratch dir; env MARKDOWNATTRACTOR_WORKER=1
```

- `--json-schema` → conforming result in `structured_output`, no parsing layer.
- Replacing the system prompt is the biggest per-call token and latency saving.
- All plugin hooks must `exit 0` immediately when `MARKDOWNATTRACTOR_WORKER=1` (recursion guard — workers load the user's plugins, including this one).
- Content goes over stdin (10 MB cap, far above chunk size).

### 4.3 Pipeline

```
event → debounce → hash (skip if unchanged) → parse → section diff
      → plan chunks → MAP (parallel) → validate/ground → REDUCE
      → atomic write → index → emit doc_summarized
```

Details that matter:

- **Priority queue:** user-created/edited files first, bulk backfill second, small docs before huge ones → something is searchable within seconds of `start`.
- **Warm pool:** N pre-spawned workers waiting on stdin; hides CLI startup.
- **Adaptive concurrency (AIMD):** start at 4, +1 on sustained success, halve on `rate_limit`/`overloaded` (`system/api_retry` events in stream-json).
- **Budgets:** daily token cap, per-doc cap, `pause`/`resume`. Per-doc cost estimates logged from the JSON `usage`/cost fields.
- **Escalation:** Sonnet only when a section fails validation twice (opt-in).
- **Deletion:** tombstone with `deleted_at`; kept forever unless `retention` is set.
- **Rename:** delete + create with same hash within a window → keep `doc_id`, record `moved_from`.
- **Identity:** `doc_id` lives only in SQLite. Source files are never modified.
- **Line ranges are refreshed on every parse, hashes are not.** Inserting text above an unchanged section shifts its lines without changing its `section_hash`. The parser rewrites `line_start`/`line_end` for *all* sections of a changed doc, and only sections whose hash changed go to the LLM.
- **Read-time freshness check.** `mda_open` re-hashes the section from the current source before returning. If the hash differs from the indexed one, it returns the *current* lines plus `stale: true` and enqueues the doc at top priority. Claude never receives an excerpt that doesn't match what is on disk.

### 4.4 Output contract

**Section card** (~80–150 tokens rendered)

```jsonc
{
  "section_id", "doc_id", "heading_path": ["Deploy","Rollback"],
  "line_start", "line_end", "token_estimate", "section_hash",
  "tldr", "summary", "keywords", "keyphrases",
  "entities": { "people","orgs","products","technologies","apis_endpoints","files_paths","commands" },
  "questions_answered",                              // 2–4 synthetic queries
  "mentioned_dates": [{ "raw","iso","precision","type","evidence" }],
  "versions_mentioned", "decisions", "action_items",
  "has_code": ["rust","bash"], "has_tables",
  "section_created_at", "section_updated_at"
}
```

**Doc card** (~200–400 tokens rendered)

```jsonc
{
  "doc_id","path","moved_from","content_hash","size_bytes","token_estimate",
  // system time
  "created_at","created_at_source":"birthtime|git|frontmatter|first_seen",
  "updated_at","first_seen_at","last_summarized_at","deleted_at",
  "git": { "first_commit_at","last_commit_at","commit_count","authors" } | null,
  "frontmatter_dates","change_history":[{ "at","sections_changed" }],
  // content time
  "as_of_date","temporal_coverage":{ "start","end" },
  "reference_date_for_relatives","status":"draft|current|deprecated|superseded|historical",
  "time_sensitivity":"evergreen|time_bound|volatile",
  // semantics
  "title","l0","summary","doc_type","audience","language",
  "keywords","topics","entities","questions_answered","acronyms_glossary",
  "links_internal","links_external",
  "sections":[{ "section_id","heading_path","line_start","line_end","token_estimate","tldr" }],
  // provenance
  "model","prompt_version","schema_version","backend"
}
```

**Rendered card** (`cards/<path>.md`): YAML frontmatter → summary → section TOC with `(lines 212–340, ~1.8K tokens)` → closing instruction: *“Read the source only for the sections you need, using these line ranges.”*

### 4.5 Temporal model (what makes it “time-aware”)

Four independent clocks, all stored, never conflated:

| Clock | Source | Example question it answers |
|---|---|---|
| **System time** | birthtime/mtime, git, `first_seen_at` | “What did we add last week?” |
| **Content time** | dates *inside* the text, `as_of_date`, coverage | “What was planned for Q4?” |
| **Index time** | `last_summarized_at`, `change_history` | “Is this card stale?” |
| **Validity** | `status`, `time_sensitivity`, `deleted_at`, `superseded_by` (phase 2) | “Is this still current?” |

Ranking uses recency decay on *system* time, filters on *content* time, and demotes `deprecated/superseded/deleted` unless the query explicitly asks for history.

---

## 5. Search layer — where semantic search happens

**Answer: entirely inside the Rust daemon, locally, exposed to Claude via MCP.** Nothing is sent anywhere except the summarization calls themselves.

```
query ─► query rewriter (optional, local) ─► BM25 (FTS5)  ─┐
                                        └► vector (sqlite-vec) ─┤─► RRF ─► time-aware rerank ─► results
                                        time/type/path filters ─┘
```

| Piece | Choice | Why |
|---|---|---|
| Lexical (cards) | SQLite **FTS5** (BM25) over `title`, `tldr`, `summary`, `keywords`, `questions_answered`, `entities`, `heading_path` | Zero dependencies, instant, works while embeddings build |
| Lexical (raw) | Second FTS5 table over the **raw section text**, populated at parse time (no LLM needed) | A config value, error string, or qualifier the card omitted is still findable. Cards drive ranking; raw text is the safety net. Also the only searchable layer for docs still waiting in the summarization queue |
| Vectors | **sqlite-vec** extension | Same file as everything else; no second DB |
| Embeddings (default) | **fastembed-rs** (ONNX) with a small model (`bge-small-en-v1.5` or `nomic-embed-text-v1.5`, ~30–130 MB) | Local, no key, no GPU; embeds a card in ms |
| Embeddings (optional) | Voyage or other API via `embedding_provider` config | Better quality for users who want it; Anthropic has no embeddings API |
| Fusion | Reciprocal Rank Fusion | Robust, tunable, standard |
| Time-awareness | recency decay + filters (`since`, `until`, `as_of`, `status`) + section-level `updated_at` | The differentiator |
| Reranker (later) | optional local cross-encoder or Haiku rerank of top-20 | Only if the eval shows a gain |

**What gets embedded:** `embed_text = title + heading_path + tldr + summary + keywords + questions_answered` — per section, plus one per doc. Cards, not raw chunks (raw-chunk embedding is a future opt-in for very technical corpora). Embeddings are derived state: rebuilt on model change, keyed by `section_hash`, never committed.

**How Claude uses it (MCP tools):**

| Tool | Purpose |
|---|---|
| `mda_search(query, since?, until?, status?, path_glob?, k=8)` | Hybrid search → L0/L1 results with `doc_id`, `section_id`, line ranges, dates, score breakdown. Each hit says whether it matched on card fields, raw text, or both; docs not yet summarized appear as raw-text hits marked `pending` |
| `mda_card(doc_id \| section_id)` | Full card (L1/L2) |
| `mda_open(section_id)` | Returns the *exact source lines* for a section — the sanctioned way to read source. Re-hashes at read time; on mismatch returns current lines with `stale: true` (see §4.3) |
| `mda_timeline(since, until, path_glob?)` | What was created/changed/deleted in a window |
| `mda_related(doc_id)` | Phase 2: explicit links now, `relates_to` later |
| `mda_status()` | Queue, coverage, staleness, cost |

A bundled **skill** tells Claude: *search first, read cards, open sections by line range, and only `Read` the whole file when the user explicitly asks or the card says the file is small.* It also states the **recovery rule**: if the top hits don't contain the answer, retry with `mda_search(..., raw=true)` (raw-text lexical only), and if that fails too, fall back to `Grep` — say so to the user, so “the answer was in the file” never becomes a silent miss.

Fallback with MCP off: cards on disk are plain markdown, so `Grep` over `cards/` still works.

---

## 6. Command UX — `/markdownattractor`

Alias: `/mda` (all commands accept both). Every command prints a one-line result and, where relevant, a link to the log. Interactive prompts use `AskUserQuestion` only for destructive actions.

### Lifecycle

| Command | Does |
|---|---|
| `/mda start [path]` | Starts the daemon on cwd (or path); first run offers a quick setup (model, ignore rules, budget) |
| `/mda stop` | Graceful stop; unfinished jobs resume on next start |
| `/mda restart` | Restart daemon (after config changes) |
| `/mda root <path>` | Change the watched root (confirms before re-indexing) |
| `/mda status` | Live dashboard: docs indexed / pending / failed, queue depth, workers, tokens used today, model, staleness |
| `/mda watch` | Stream progress live in the session (uses stream-json events) |
| `/mda doctor` | Checks: `claude` on PATH & logged in, disk, model cache, extension load, permissions, orphaned PID; prints fixes |

### Indexing

| Command | Does |
|---|---|
| `/mda index [path\|glob]` | Force (re)index now; `--all` for full rebuild |
| `/mda pause` / `/mda resume` | Pause summarization without stopping the watcher |
| `/mda rebuild [--cards\|--embeddings\|--all]` | Regenerate derived artifacts |
| `/mda ignore <pattern>` / `/mda unignore <pattern>` | Edit `.markdownattractorignore` |
| `/mda prune [--expired]` | Remove tombstones per retention policy |

### Search & explore (also available as MCP tools)

| Command | Does |
|---|---|
| `/mda search <query> [--since 7d] [--until date] [--status current] [--in path]` | Hybrid search; results show title, tldr, lines, dates, why-matched |
| `/mda open <doc\|section id>` | Prints the card, or the exact source lines for a section |
| `/mda timeline [--since 30d] [--in path]` | Created / changed / deleted, grouped by day |
| `/mda recent [n]` | Most recently updated docs with what changed |
| `/mda stale` | Docs whose source changed after their last summary (should be empty) |
| `/mda explain <query>` | Shows score breakdown (BM25 vs vector vs recency) — great for tuning and demos |

### Configuration

| Command | Does |
|---|---|
| `/mda summarization_model <haiku\|sonnet\|opus\|model-id>` | Set summarizer model |
| `/mda escalation_model <model\|off>` | Model for sections that fail validation |
| `/mda backend <claude-cli\|api>` | Auth backend |
| `/mda embeddings <local-small\|local-base\|voyage\|off>` | Embedding provider |
| `/mda concurrency <n\|auto>` | Worker pool size |
| `/mda budget <tokens/day\|off>` | Daily cap |
| `/mda retention <days\|forever>` | Tombstone expiry |
| `/mda nudge <on\|off>` | The `PreToolUse` reminder on `Grep`/`Read` of `*.md` (default: on) |
| `/mda config` | Show / edit `config.toml` |

### Maintenance & meta

| Command | Does |
|---|---|
| `/mda cost [--since 7d]` | Tokens spent indexing (per day / per doc) **next to** tokens saved on reads (estimated from `mda_open` line ranges vs. full-file size), so the net is visible at a glance |
| `/mda diagnostics` | Writes a redacted, voluntary diagnostics bundle (versions, config, queue/coverage stats, index hit rate, last errors — no document content) the user can attach to an issue |
| `/mda export [--format jsonl\|md]` | Dump cards for other tools |
| `/mda logs [--tail 100]` | Daemon logs |
| `/mda reset` | Delete `.markdownattractor/` (double confirm) |
| `/mda version` / `/mda update` | Version info, self-update |
| `/mda help` | Command reference |

**Delight details:** progress bar with ETA on first index; `status` shows “Claude will read ~X% fewer tokens with this index”; `search` results are copy-pasteable `Read(path, offset, limit)` calls; colorized, but respects `NO_COLOR`.

---

## 7. Speed budget

| Stage | Target |
|---|---|
| Watcher → job enqueued | < 50 ms after debounce |
| Parse + section diff (100 KB file) | < 20 ms |
| LLM section call (Haiku, 4K-token chunk) | 3–6 s (warm worker) |
| Reduce | 2–4 s |
| Embed a card (local small model) | < 20 ms |
| Search (10K sections) | < 30 ms end-to-end |
| Daemon RSS | < 150 MB with model loaded |
| Cold start of `mda status` | < 50 ms |

Levers, in order of impact: hashing (never re-summarize unchanged content) → warm pool → replaced system prompt + tool-less worker → compact schemas with string caps → adaptive concurrency → Haiku w/o thinking → priority queue.

---

## 8. Roadmap

### Phase 0 — Spike (1–2 days)
- Measure `claude -p` cold start, tokens/call with `--system-prompt`, warm-pool effect, rate limits at 4/8/16 workers (Pro vs Max).
- Haiku quality on 20 real docs: date grounding pass-rate, usefulness of `questions_answered`.
- Confirm flags exist and combine in current Claude Code.
- Request the login-policy confirmation from Anthropic (§4.1) and draft the API-key-only README variant.
- Output: `docs/aha.md` + go/no-go on defaults. **Exit criteria:** policy answer received (or a dated follow-up and the fallback pitch ready); Haiku grounding pass-rate ≥ 95% on the 20 docs.

### Phase 1 — Summarization engine (2–3 weeks)
- Rust crate `mda-core`: watcher, parser, section diff, planner, worker pool, validator, reducer, store, atomic writer.
- CLI `mda` + plugin skeleton (`.claude-plugin/plugin.json`, skills, hooks with worker guard).
- Cards on disk; FTS5 search; `/mda start|stop|status|index|search|open|doctor|summarization_model`.
- Golden set + eval harness (`mda eval`).

### Phase 2 — Search layer (2 weeks)
- sqlite-vec + fastembed; hybrid RRF over card FTS + raw-text FTS + vectors; time-aware ranking; MCP server with the tools in §5.
- Bundled skill teaching Claude the progressive-disclosure workflow and the recovery rule.
- `/mda timeline|recent|stale|explain`, filters.
- Default-on `PreToolUse` nudge hook (`/mda nudge on|off`) and the index hit rate in `/mda status`.

### Phase 3 — Relationships & wiki (later, separate design)
- `relates_to` from explicit links + embedding neighbors + shared entities, each edge tagged EXTRACTED / INFERRED (score) / AMBIGUOUS; `supersedes` from status/date signals.
- Wiki index pages (by topic, by time). Design doc TBD.

### Phase 4 — Polish & launch
- Distribution per §9: self-hosted marketplace, bootstrap hook, GitHub Releases with checksums, Homebrew tap, `cargo install`; submit to `claude-plugins-community`.
- Benchmarks page with reproducible numbers; 60-second demo GIF; `plugin eval` cases.
- Launch post; Show HN; README leads with the one-command install.
- **Adoption loop (no telemetry, by design):**
  - *Activation* = first useful answer within 10 minutes of install. *Retention* = daemon still running and index hit rate > 0 after 14 days.
  - 5–8 design partners with real markdown-heavy repos, recruited before launch, checked in with every two weeks for the first two months; their corpora become the private half of the benchmark set.
  - `/mda diagnostics` bundle linked from the issue template; every abandonment report gets a one-line entry in `docs/aha.md` (what broke, what we changed).

---

## 9. Distribution & installation

### 9.1 What the ecosystem does

| Project | Distribution | User experience |
|---|---|---|
| graphify | `pip install graphifyy` + `graphify install` writes a skill | Two steps, needs Python; PyPI name squatting issues |
| claude-vault, llm-wiki-plugin, most community plugins | Repo doubles as its own marketplace (`.claude-plugin/marketplace.json` + plugin in same repo) | `/plugin marketplace add owner/repo` → `/plugin install name@marketplace` |
| CodeGraph | `npm install -g` + `codegraph install` writes MCP config | Two steps, needs Node 22/24; native-vs-WASM binding gotcha |
| Understand Anything | Marketplace add + install (`Lum1104/Understand-Anything`) | Standard two-step |
| claudix | Rust binary; plugin bootstraps the binary at session start | Closest to what we need |

The standard, lowest-friction path for a Claude Code plugin is **the repo is the marketplace**: one GitHub repo containing both `.claude-plugin/marketplace.json` (catalog) and the plugin itself. Users need nothing but Claude Code and git. We do that, plus the two upgrades that make it one command and zero prerequisites.

### 9.2 Install paths, in order of preference

**1. One command (Claude Code ≥ 2.1.275):**

```
/plugin install markdownattractor --marketplace joe-carr-data/markdownattractor
```

Claude Code shows the resolved source, asks to confirm, adds the marketplace, opens the plugin details, and installs at the chosen scope. This is the line at the top of the README.

**2. Classic two-step (any recent Claude Code):**

```
/plugin marketplace add joe-carr-data/markdownattractor
/plugin install markdownattractor@markdownattractor
```

**3. Community marketplace (after launch):** submit to `anthropics/claude-plugins-community`. It runs Anthropic's automated validation and safety screening and pins each plugin to a commit SHA — a trust signal worth having. Users then run `/plugin install markdownattractor@claude-community`. The official marketplace is at Anthropic's discretion; we apply once adoption numbers justify it.

**4. Teams:** one `.claude/settings.json` snippet (`extraKnownMarketplaces` + `enabledPlugins`) so every collaborator who trusts the repo gets it. Document it; it's how orgs standardise.

**5. Power users / CI:** `cargo install mda-cli`, `brew install joe-carr-data/tap/mda`, and prebuilt archives on GitHub Releases. `claude plugin install markdownattractor@markdownattractor --scope user` for scripted installs. Also `--plugin-dir ./` for contributors.

### 9.3 The hard part: shipping a Rust binary inside a plugin

Plugins are copied into `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/` at install; the Rust binary cannot live in git (size, per-platform builds) and `${CLAUDE_PLUGIN_ROOT}` changes on every update. Solution: **a bootstrap hook that fetches the right prebuilt binary into `${CLAUDE_PLUGIN_DATA}`**, which persists across updates.

```
SessionStart hook → scripts/bootstrap.sh
  1. read wanted version from ${CLAUDE_PLUGIN_ROOT}/VERSION
  2. if ${CLAUDE_PLUGIN_DATA}/bin/mda --version == wanted → exit 0 (fast path, <20 ms)
  3. detect os/arch (darwin-arm64, darwin-x64, linux-x64, linux-arm64, windows-x64)
  4. download https://github.com/joe-carr-data/markdownattractor/releases/download/v<ver>/mda-<target>.tar.gz
     + SHA256SUMS; verify checksum; extract to ${CLAUDE_PLUGIN_DATA}/bin/mda
  5. on failure: print one line telling the user to run `/mda doctor` — never block the session
```

- Hook and MCP config reference `${CLAUDE_PLUGIN_DATA}/bin/mda`, so the path is stable across plugin updates.
- Checksums are published per release and generated in CI (`cargo-dist` or a release workflow with cross builds via `cross`); binaries are signed on macOS (notarised) so Gatekeeper doesn't block them.
- Air-gapped or download-blocked machines: `/mda doctor` prints the manual install steps (drop the binary in `${CLAUDE_PLUGIN_DATA}/bin/`, or `cargo install`).
- No top-level `bin/` directory in the plugin: claude.ai organisation distribution rejects plugins that have one. Everything lives under `scripts/` and `${CLAUDE_PLUGIN_DATA}`.
- Worker guard: `bootstrap.sh` exits immediately when `MARKDOWNATTRACTOR_WORKER=1`.

Alternative considered — **npm `optionalDependencies` per platform** (the esbuild/biome pattern): Claude Code runs `npm ci --ignore-scripts` on plugins that ship a `package-lock.json`, so platform binaries would arrive without any download script. Rejected for v1: 60-second install timeout, npm dependency, and no control over failure messaging. Revisit if the bootstrap hook proves flaky.

### 9.4 Versioning & updates

- `plugin.json.version` is the update signal — bump it on every release (users keep the cached copy otherwise). Semver; `VERSION` file in the plugin root mirrors it so `bootstrap.sh` and the release workflow read one source.
- Release = git tag `vX.Y.Z` → CI builds binaries, publishes GitHub Release + checksums, bumps `marketplace.json`, and publishes to crates.io / Homebrew tap.
- Users update with `/plugin update markdownattractor@markdownattractor` or via marketplace auto-update (off by default for third-party marketplaces — the README tells users how to turn it on).
- `renames` map in `marketplace.json` is append-only; never rename `markdownattractor` itself.
- Release channels: a `stable` branch/tag for the marketplace default; `latest` available via `/plugin marketplace add joe-carr-data/markdownattractor@main` for early adopters.

### 9.5 First-run experience

1. Install (one command). 2. The SessionStart hook fetches the binary in the background. 3. The user types `/mda start`: **one** confirmation (“Index `<root>`? N markdown files found”) — model, ignore rules and budget use sensible defaults (Haiku, `.gitignore` + common junk, budget off) and are changeable later with the config commands. Raw-text search is available the moment parsing finishes, before any LLM call. 4. When the first ~10 docs are summarized, `start` runs **one example query** against them (picked from the first doc's `questions_answered`) and prints the hit with its line range, so the user sees a real result before walking away. 5. `/mda status` shows coverage and the index hit rate. **Target: install → first useful answer < 10 minutes.** Nothing else to configure, no API key, no Python, no Node.

---

## 10. Repository layout

```
markdownattractor/
├── .claude-plugin/
│   ├── plugin.json
│   └── marketplace.json               # the repo is its own marketplace
├── VERSION                            # single source for plugin + binary version
├── skills/
│   ├── markdownattractor/SKILL.md      # command family (/mda …)
│   └── search-first/SKILL.md          # teaches Claude the L0→L2→lines workflow
├── hooks/hooks.json                   # SessionStart: ensure daemon; worker guard; opt-in PreToolUse nudge
├── .mcp.json                          # mda mcp (stdio)
├── scripts/
│   ├── bootstrap.sh                   # SessionStart: fetch verified binary into ${CLAUDE_PLUGIN_DATA}/bin
│   └── mda                            # launcher → ${CLAUDE_PLUGIN_DATA}/bin/mda (no top-level bin/)
├── crates/
│   ├── mda-core/                      # library: pipeline, store, search
│   ├── mda-cli/                       # `mda` binary (daemon + commands)
│   └── mda-mcp/                       # MCP server (or feature in cli)
├── prompts/                           # versioned system prompts + JSON schemas
├── evals/                             # golden docs, queries, expected hits; plugin eval cases
├── docs/
│   ├── index.md                       # doc index (kept current)
│   ├── design/summarization.md
│   ├── design/search.md
│   ├── design/temporal-model.md
│   ├── design/commands.md
│   ├── adr/                           # decisions (backend, storage, embeddings…)
│   ├── benchmarks.md
│   ├── aha.md
│   └── archive/
├── CHANGELOG.md · LICENSE (Apache-2.0) · README.md · CONTRIBUTING.md · SECURITY.md
```

Engineering rules: production-ready, documented code; frequent descriptive commits; documented PRs; plan mode before medium/hard tasks; `docs/index.md` always updated; findings into `docs/aha.md`.

---

## 11. Evaluation & benchmarks (public from day one)

- **Golden set:** 30 real technical docs (mixed sizes, ADRs, runbooks, changelogs, meeting notes) with hand-labelled dates, entities, status, and 60 queries (incl. temporal ones).
- **Metrics:** recall@5 / MRR; date precision & recall; grounding pass-rate; tokens Claude reads per answered query (vs. baseline `Grep`+`Read`); p50/p95 save→searchable; cost per 1K docs.
- **Harness:** `mda eval` runs everything offline against recorded worker outputs; CI publishes `docs/benchmarks.md`.
- **A/B protocol (CodeGraph-style):** same question, headless `claude -p`, with and without the index, 4 runs, medians, on ≥ 5 corpora of different sizes; report tokens, tool calls, wall-clock, cost.
- **Answer-quality parity gate:** every A/B query has a reference answer; a grader (Sonnet, rubric-based) scores both runs for correctness and completeness. Token savings are only reported for queries where the with-index answer scores ≥ the baseline. Queries that fail parity are listed separately — they are the bug list, not noise.
- **Corpora are fixed before results are seen:** the 5 corpora (sizes from ~50 to ~5K docs, including at least two design-partner repos) are named in `docs/benchmarks.md` at Phase 1 start, and never swapped to make the numbers look better. Report the small-corpus results even if the gain is nil — that's the honest “when not to install this” section.
- **Model matrix:** Haiku vs Sonnet, local-small vs local-base vs Voyage embeddings.

---

## 12. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Policy on reusing Claude Code login | Written confirmation from Anthropic requested in Phase 0; API-key backend as alternative with its own pitch pre-written; clear README disclosure |
| Claude ignores the index and keeps grepping raw files | Default-on `PreToolUse` nudge; index hit rate visible in `/mda status`; skill states the search → card → lines workflow and the recovery rule |
| Card omitted the detail the user needed (“the answer was in the file”) | Raw-text FTS5 table as lexical safety net; `raw=true` retry; explicit Grep fallback in the skill |
| Stale card or wrong line range during active editing | Line ranges refreshed on every parse; `mda_open` re-hashes at read time and flags `stale`; pending docs searchable via raw text |
| Benchmarks look good but don't reproduce for users | Parity gate on answer quality; corpora fixed up front; small-corpus results published |
| `claude -p` startup overhead makes “ultrafast” a stretch | Warm pool, section-level incremental updates, priority queue; measure in spike; `--bare` path for API users |
| Opaque subscription rate limits | AIMD concurrency, budgets, `pause`, visible cost |
| Recursion (workers loading this plugin) | Worker env guard in every hook; scratch cwd; `--setting-sources ""` |
| Hallucinated metadata poisoning search | Evidence grounding on every extracted date/entity; schema caps |
| Embedding model download friction | Small default model, lazy download with progress, BM25 works immediately |
| Binary bootstrap fails (offline, corporate proxy, Gatekeeper) | Checksummed downloads, notarised macOS builds, never block the session, `/mda doctor` with manual steps, `cargo install` fallback |
| Flag drift across Claude Code versions | `mda doctor` checks flags; version-gated code paths; CI against latest CLI |
| Large corpora (100K+ files) | Backfill throttling, sampling mode, `ignore` defaults, SQLite WAL, batch embedding |

---

## 13. Open decisions

1. Default local embedding model: `bge-small` (fast, English) vs `nomic-embed-text-v1.5` (multilingual, larger).
2. Commit `cards/` by default or leave it to the user? (Leaning: prompt once at `start`.)
3. Single binary with MCP as a subcommand vs separate `mda-mcp` binary.
4. Should the daemon be per-root (simple) or one global daemon with multiple roots (fewer processes)?

---

## 14. Immediate next steps

1. Run the Phase 0 spike; record results in `docs/aha.md`.
2. Write `docs/design/summarization.md` and `docs/design/search.md` from this plan (with ADRs for backend, storage, embeddings).
3. Scaffold the repo (`cargo workspace` + plugin skeleton) and ship `/mda start|status|search` end-to-end on a 20-file corpus.
4. Publish the benchmark page skeleton so numbers are public from the first release.

---

## 15. References for Claude

Primary sources to read before implementing. Prefer the `.md` versions of Claude Code docs (append `.md` to any `code.claude.com/docs/en/...` URL); the full index is at `https://code.claude.com/docs/llms.txt`.

### Claude Code — plugins & distribution
- Plugins reference (manifest schema, hooks, MCP servers, monitors, `${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_DATA}`, caching, Node dependency install, `bin/` rules): https://code.claude.com/docs/en/plugins-reference.md
- Create and distribute a marketplace (`marketplace.json` schema, sources, versioning, `renames`, release channels, team config): https://code.claude.com/docs/en/plugin-marketplaces.md
- Discover and install plugins (one-command `--marketplace` install, scopes, auto-update, community marketplace): https://code.claude.com/docs/en/discover-plugins.md
- Create plugins + community marketplace submission: https://code.claude.com/docs/en/plugins.md
- Plugin evals (`claude plugin eval`): https://code.claude.com/docs/en/plugin-evals.md
- Hooks reference (events, `PreToolUse` matchers, exec vs shell form): https://code.claude.com/docs/en/hooks.md
- Skills: https://code.claude.com/docs/en/skills.md
- Sub-agents: https://code.claude.com/docs/en/sub-agents.md
- MCP in Claude Code (plugin-provided servers, `roots/list`, tool naming `mcp__plugin_<plugin>_<server>__<tool>`): https://code.claude.com/docs/en/mcp.md
- Settings reference (`enabledPlugins`, `extraKnownMarketplaces`, `pluginConfigs`, permission rule syntax): https://code.claude.com/docs/en/settings-reference.md
- Environment variables: https://code.claude.com/docs/en/env-vars.md

### Claude Code — headless workers (the summarizer backend)
- Run Claude Code programmatically (`-p`, `--bare`, `--output-format json`, `--json-schema`, `stream-json`, `system/api_retry`, stdin cap, exit codes): https://code.claude.com/docs/en/headless.md
- CLI reference (all flags: `--model`, `--system-prompt`, `--append-system-prompt`, `--tools`, `--setting-sources`, `--strict-mcp-config`, `--permission-mode`): https://code.claude.com/docs/en/cli-reference.md
- Permission modes: https://code.claude.com/docs/en/permission-modes.md
- Agent SDK overview (why we use the CLI/Client path, not the agent loop; third-party login policy note): https://code.claude.com/docs/en/agent-sdk/overview.md
- Agent SDK Python / TypeScript (if we ever move workers off the CLI): https://code.claude.com/docs/en/agent-sdk/python.md · https://code.claude.com/docs/en/agent-sdk/typescript.md
- Cost tracking: https://code.claude.com/docs/en/agent-sdk/cost-tracking.md

### Claude API & models
- Models overview (IDs, context windows, latency tiers, pricing): https://platform.claude.com/docs/en/about-claude/models/all-models
- Docs site map: https://docs.claude.com/en/docs_site_map.md
- Structured outputs / JSON schema: https://platform.claude.com/docs/en/build-with-claude/structured-outputs
- Prompt caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- Rate limits: https://platform.claude.com/docs/en/api/rate-limits
- Commercial Terms (governs SDK/plugin use): https://www.anthropic.com/legal/commercial-terms
- Usage policy: https://www.anthropic.com/legal/aup

### Search & storage building blocks
- SQLite FTS5: https://www.sqlite.org/fts5.html
- sqlite-vec: https://github.com/asg017/sqlite-vec
- fastembed-rs: https://github.com/Anush008/fastembed-rs
- rusqlite: https://github.com/rusqlite/rusqlite
- Reciprocal Rank Fusion (Cormack et al.): https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf
- Model Context Protocol spec: https://modelcontextprotocol.io/specification
- Rust MCP SDK (`rmcp`): https://github.com/modelcontextprotocol/rust-sdk

### Rust engine crates
- notify (filesystem watcher): https://github.com/notify-rs/notify
- comrak (CommonMark/GFM parser): https://github.com/kivikakk/comrak
- pulldown-cmark (alternative parser): https://github.com/pulldown-cmark/pulldown-cmark
- gix / gitoxide (git metadata): https://github.com/GitoxideLabs/gitoxide
- blake3: https://github.com/BLAKE3-team/BLAKE3
- ignore (gitignore-aware walking, from ripgrep): https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore
- tiktoken-rs (token estimates): https://github.com/zurawiki/tiktoken-rs
- cargo-dist (release builds & installers): https://github.com/axodotdev/cargo-dist
- cross (cross-compilation): https://github.com/cross-rs/cross

### Competitors & prior art (studied in §2)
- graphify: https://github.com/safishamsi/graphify
- CodeGraph: https://github.com/colbymchenry/codegraph
- Understand Anything: https://github.com/Lum1104/Understand-Anything
- qmd (tobi): https://github.com/tobi/qmd · Rust port: https://github.com/qntx-labs/qmd
- claudix: https://docs.rs/crate/claudix/latest
- claude-vault: https://github.com/DenisKhay/claude-vault
- claude-context-local: https://github.com/FarhanAliRaza/claude-context-local
- llm-wiki-plugin: https://github.com/praneybehl/llm-wiki-plugin
- Community marketplace (submission target): https://github.com/anthropics/claude-plugins-community
- Demo plugins (reference implementations): https://github.com/anthropics/claude-code/tree/main/plugins

---

## 16. Rust stack — current state (checked September 2026)

The ecosystem moved a lot in 2025–26. What follows is what I verified against crates.io / GitHub this month; anything marked *(unverified)* is a well-known crate I did not re-check. Re-run this survey before Phase 1 starts.

### 16.1 Things that changed and affect the plan

| Area | What happened | Decision |
|---|---|---|
| **MCP SDK** | Official `rmcp` is now **v2.x** (2.1.0 in July 2026), implements the stable **MCP 2026-07-28** spec (stateless Streamable HTTP, long-running tasks, server discovery) and stays compatible with 2025-11-25. `rust-mcp-sdk` also hit 2.x with 100% conformance-suite pass. rmcp v2 deprecated the old roots/sampling/logging types — read the migration guide (#926). | **`rmcp = "2"`** with `server` + `transport-io` features (stdio). Not v0.x as in earlier notes. |
| **Embeddings** | `fastembed` is at **v7.0.0** (Sept 2026; v6 in July). Now ships **EmbeddingGemma-300m** incl. Q4/int8 builds, **Qwen3-Embedding-0.6B** and **Nomic v2 MoE** (behind `qwen3` / `nomic-v2-moe` features, candle backend), plus cross-encoder rerankers (`bge-reranker-v2-m3`, `jina-reranker-v2`). Cache dir moved to `.fastembed_cache` / `FASTEMBED_CACHE_DIR`. | **`fastembed = "7"`**. Default model: `BGESmallENV15Q` (fast, ~33 MB). Offer `EmbeddingGemma300MQ4` (multilingual, ~200 MB) as `local-base`. Reranker is now cheap to add — put it behind `/mda rerank on`. Set `FASTEMBED_CACHE_DIR` to `${CLAUDE_PLUGIN_DATA}/models`. |
| **Vector store** | `sqlite-vec` crate is at **0.1.9** (March 2026; 0.1.10 alphas since) and, importantly, the upstream extension **shipped its first ANN index (“rescore”, PR #276, March 2026)** — bit/int8 quantized coarse search + full-precision re-score, `INDEXED BY rescore(...)` syntax. Still pre-1.0 and the crate pins `rusqlite ^0.31`. | Keep **sqlite-vec** (same file as FTS5, tiny). Brute-force is fine < 50K sections; enable `rescore` above that. Watch the rusqlite version pin — it may force our rusqlite version. |
| **SQLite engine** | **Turso Database** (the Rust rewrite, ex-Limbo) now runs in production at some orgs, has native vector search and, since v0.5 (Jan 2026), **experimental native FTS built on tantivy**. libSQL remains the battle-tested fork. Turso's own FAQ still says “not ready for production use” in the crate docs. | **`rusqlite` (bundled, FTS5)** for v1. Turso is the obvious v2 candidate (one engine for FTS + vectors + async), so keep the store behind a trait and re-evaluate when Turso hits 1.0. |
| **Release tooling** | `cargo-dist` (now just **`dist`**) is alive: 0.31–0.33 in 2026, GitHub artifact attestations, cross-compilation to Linux (cargo-zigbuild) and Windows (cargo-xwin) built in. The astral-sh fork was merged back. | Use **dist** for the release pipeline; it produces the checksummed archives our bootstrap hook needs. |
| **Time** | `jiff` (BurntSushi) at 0.2.x, 1.0 slipped to “spring/summer 2026” — check whether it landed. Built-in IANA tz, RFC 9557 lossless zoned serialisation, Temporal-style API. | **`jiff`** for all timestamps (our temporal model wants correct, lossless ISO 8601 with zones). Fall back to `chrono` only if a dependency forces it. |

### 16.2 Recommended dependency list (Cargo.toml, workspace)

```toml
[workspace.dependencies]
# runtime
tokio        = { version = "1", features = ["full"] }
tokio-util   = "0.7"          # CancellationToken, task tracking
clap         = { version = "4", features = ["derive"] }
tracing      = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender   = "0.2"
anyhow / thiserror              # errors

# storage & search
rusqlite     = { version = "0.31", features = ["bundled", "fts5"] }   # pinned by sqlite-vec — verify
sqlite-vec   = "0.1"
fastembed    = "7"             # add features = ["qwen3"] only if we offer Qwen3-Embedding

# markdown & files
comrak       = "0.x"           # GFM, frontmatter, sourcepos on every node (unverified: check latest)
gray_matter  = "0.x"           # frontmatter parsing (unverified)
notify       = "8"             # + notify-debouncer-full (unverified: check 8.x API)
ignore       = "0.4"           # gitignore-aware walking (ripgrep)
gix          = "0.x"           # git metadata; heavier compile but pure Rust (unverified: latest)
blake3       = "1"
tiktoken-rs  = "0.x"           # token estimates; or a chars/4 heuristic to avoid the BPE dependency

# LLM output contract
serde / serde_json
schemars     = "1"             # derive JSON Schema from Rust structs → --json-schema for claude -p (unverified: 1.0 status)
jsonschema   = "0.x"           # validate structured_output before trusting it

# MCP
rmcp         = { version = "2", features = ["server", "transport-io", "macros"] }

# time
jiff         = { version = "0.2", features = ["serde"] }   # or "1" if released

# plumbing
governor     = "0.x"           # optional token-bucket for API-key backend; AIMD stays hand-rolled
interprocess = "2"             # unix socket / named pipe for daemon ↔ CLI (unverified)
indicatif    = "0.17"          # progress in CLI
```

### 16.3 Notes & gotchas

- **`schemars` + `rmcp` + `claude -p --json-schema` share one schema source.** Derive `JsonSchema` on the card structs once; use it for the worker's `--json-schema`, for validation, and for MCP tool `outputSchema` (rmcp v2 allows any JSON Schema type per SEP-2106).
- **ONNX Runtime binary size.** fastembed pulls `ort`; expect +30–60 MB in the binary or a dynamic `libonnxruntime`. Decide: static link (simpler bootstrap) vs. `ort` `download-binaries` feature. Apple Silicon: CoreML EP exists in `ort` but CPU is already fast for 384-dim small models; don't chase it in v1.
- **Candle-backed models (EmbeddingGemma? no — that one is ONNX; Qwen3/Nomic-MoE are candle)** add a second inference stack. Keep them behind a feature flag so the default binary stays lean.
- **sqlite-vec pins rusqlite.** If we need a newer rusqlite (e.g., for a security fix), we may have to vendor the extension via `cc` and `sqlite3_auto_extension` ourselves — it's a single C file.
- **rmcp v2 stateless HTTP** is irrelevant for us (stdio only), but the same server struct can be exposed over HTTP later for a web UI or other clients at no cost.
- **Turso** is the one bet worth tracking quarterly: if it stabilises with FTS + vectors + async, it collapses three dependencies into one and removes the C toolchain from the build.
- **dist + notarisation:** dist builds and signs Windows; macOS notarisation still needs `rcodesign` (apple-codesign crate) or Apple's `notarytool` in CI — budget a day for it.

---

## 17. SDLC documentation — keeping Claude oriented across sessions and compactions

The failure mode to design against: a long build where every session (or every compaction inside a session) starts with Claude re-deriving what the project is, where it stands, and what it decided last week. The cure is a small set of documents with fixed roles, a strict size budget for the ones loaded every session, and hooks that refresh them automatically. Nothing below relies on Claude "remembering" — everything is on disk, and the loading order is deterministic.

### 17.1 Three tiers of memory

| Tier | Loaded | Budget | Contents |
|---|---|---|---|
| **T0 — always on** | Every session and every compaction | ≤ 1,500 tokens total | `CLAUDE.md` (project charter + rules pointer), `docs/STATUS.md` (where we are), `.claude/rules/*.md` |
| **T1 — on demand, cheap** | When the task touches the area | ≤ 3K tokens each | `docs/index.md`, `docs/aha.md`, the one relevant `docs/design/*.md`, the current `docs/plans/*.md`, ADRs |
| **T2 — reference** | Only when explicitly needed | unbounded | This plan, full specs, benchmark logs, review transcripts, archive |

Rule: T0 must fit in one screen. If `STATUS.md` grows past its budget, the excess moves into `docs/plans/` or `docs/archive/` — never expand T0.

### 17.2 The documents

```
markdownattractor/
├── CLAUDE.md                        # T0. Charter: what, why, non-goals, 10 golden rules, how to navigate docs/
├── .claude/
│   ├── rules/
│   │   ├── workflow.md              # plan mode, commit cadence, PR template, when to ask
│   │   ├── docs.md                  # "update STATUS/aha/index before ending a task"
│   │   └── rust.md                  # crate choices, error handling, no new deps without ADR
│   ├── skills/                      # dev-only skills: /handoff, /resume, /codex-review, /adr
│   └── settings.json                # hooks below, enabled for the team
├── docs/
│   ├── index.md                     # T1. Map of every doc, newest first, one line each
│   ├── STATUS.md                    # T0. Current phase, active plan, last 5 done, next 3, open blockers, last Codex review
│   ├── NORTH-STAR.md                # T1. Objectives G1–G6 + success metrics; the thing we optimise for
│   ├── aha.md                       # T1. Short, dated findings; pruned monthly (≤ 60 lines)
│   ├── plans/                       # T1. One file per phase/sprint: goals, task list w/ checkboxes, exit criteria
│   │   └── 2026-09-phase0-spike.md
│   ├── design/                      # T1. summarization.md, search.md, temporal-model.md, commands.md, distribution.md
│   ├── adr/                         # T1. NNNN-title.md — context / decision / consequences / status
│   ├── reviews/
│   │   ├── codex/                   # T2. YYYY-MM-DD-<scope>.md — external review + our triage
│   │   └── retro/                   # T2. milestone retrospectives
│   ├── handoffs/                    # T2. auto-written by PreCompact/SessionEnd, one per session, pruned weekly
│   ├── benchmarks.md
│   ├── project-plan.md              # T2. this document
│   └── archive/                     # anything superseded, with a one-line pointer left behind
```

**`CLAUDE.md` (charter) — the only prose Claude reads unconditionally.** Contents, in order: one-paragraph pitch; the six goals by name (G1 speed … G6 trust); non-goals; the ten golden rules (never write to source files, hash before summarise, no new crate without ADR, `--json` on every CLI command, worker env guard, etc.); "How to orient yourself": *read `docs/STATUS.md`, then the plan it points to, then `docs/aha.md`; consult `docs/index.md` to find anything else*. No status, no history — those change and belong in `STATUS.md`.

**`docs/STATUS.md` — the single mutable source of truth.** Fixed template:

```markdown
# STATUS (updated 2026-09-21 by Claude, session 0f3a)
Phase: 0 — spike            Active plan: docs/plans/2026-09-phase0-spike.md
North star reminder: G1 p50 < 15 s save→searchable · G6 100% grounded metadata
## Done (last 5)
- …
## Next (max 3, in order)
1. …
## Blockers / open questions
- …
## Last Codex review: 2026-09-19 (docs/reviews/codex/2026-09-19-worker-pool.md) — 2 findings open
```

Claude updates it at the end of every task. Because it is T0, the next session (or the post-compaction context) starts with the exact state, not a reconstruction.

**`docs/aha.md`** stays as already defined: dated one-liners of things learned the hard way, checked on every compaction. Monthly, entries that became rules graduate to `.claude/rules/`; the rest go to `archive/aha-YYYY-MM.md`.

**ADRs** are the memory of *why*. Every crate choice, storage decision, auth path, and schema change gets one. `/adr <title>` scaffolds it. `docs/index.md` lists them newest-first so "why did we pick sqlite-vec" is one lookup.

### 17.3 Hooks that keep it honest

| Hook | Action |
|---|---|
| `SessionStart` | Print `docs/STATUS.md` into context (it is small) and the first 20 lines of the active plan. |
| `PreCompact` | Run `/handoff`: write `docs/handoffs/<date>-<session>.md` (what was being done, files touched, uncommitted intent, next step), then refresh `STATUS.md`. Compaction then keeps only the charter + STATUS, and nothing is lost. |
| `PostCompact` | Re-inject `STATUS.md` and the latest handoff so the resumed context is grounded. |
| `SessionEnd` | Same as `PreCompact`, plus a reminder if `STATUS.md` was not touched this session. |
| `Stop` (soft) | If files under `crates/` changed but nothing under `docs/` did, nudge: "update aha/STATUS/index?" |
| `PostToolUse` on `Write|Edit` of `docs/**` | Regenerate `docs/index.md` (script), so the index never goes stale. |

All hooks are scripts under `scripts/dev/` and are cheap (<100 ms). They ship in the repo's `.claude/settings.json`, not in the plugin.

### 17.4 Working rhythm

1. **Start**: `/resume` → reads STATUS, active plan, aha; states the next task in one line; asks only if the plan is ambiguous.
2. **Plan mode** for anything medium or larger; the plan is written to `docs/plans/` *before* code, with exit criteria.
3. **Build** in small commits (`feat|fix|docs|chore(scope): …`); PRs carry a "Docs updated" checklist (STATUS, aha, index, ADR if a decision was made).
4. **Close**: `/handoff` (manual or via hook) → STATUS, aha, index updated → commit `docs: handoff`.
5. **Weekly**: prune handoffs, graduate aha entries, check T0 budget, run the Codex review (below).

### 17.5 Periodic Codex reviews

A second model catches what the author model normalises. Codex (OpenAI) is the reviewer of record; the workflow is model-agnostic so Gemini or a second Claude can substitute.

**Cadence**

| When | Scope | Output |
|---|---|---|
| Every PR touching `crates/` | Diff review: correctness, error handling, unsafe, async misuse, tests | Inline findings, PR comment |
| Weekly (Friday) | Architecture drift vs `NORTH-STAR.md` + `design/*`; stale docs; dependency risk | `docs/reviews/codex/YYYY-MM-DD-weekly.md` |
| Milestone (end of each phase) | Full re-read of design docs, ADRs, benchmarks; "what would you have done differently" | `docs/reviews/codex/YYYY-MM-DD-phase-N.md` + retro |
| Before public release | Security-focused: bootstrap script, hooks, worker sandboxing, path traversal, supply chain | Findings must be closed or explicitly accepted in an ADR |

**How it runs — `/codex-review <scope>`**

1. Assemble a review packet: `NORTH-STAR.md`, the relevant design doc(s), the diff or file list, and the *questions we want answered* (max 5).
2. Run Codex non-interactively (`codex exec --json` with the packet on stdin; or via the `pal` MCP `clink`/`codereview` tools if the session has them) with a fixed reviewer prompt: severity-tagged findings, each with file:line, a one-line fix, and confidence.
3. Claude triages: for each finding → *accept* (creates a task in the active plan), *reject* (reason recorded), or *defer* (ADR or backlog). No finding is silently dropped.
4. Write the review file with packet, raw findings, triage table; link it from `STATUS.md` ("Last Codex review") and `index.md`.
5. Accepted findings that reveal a pattern become a line in `aha.md` or a rule in `.claude/rules/`.

**Guardrails**: Codex never writes to the repo; it reviews. Reviews are pinned to a commit SHA so they can be re-checked. A finding closed twice as "won't fix" gets an ADR so the disagreement is documented, not re-litigated every week.

### 17.6 Definition of done for any task

- Code + tests merged; `cargo clippy -D warnings` clean.
- `docs/STATUS.md` updated; `docs/index.md` regenerated.
- If a decision was made → ADR. If a lesson was learned → `aha.md`.
- If it changed behaviour a user sees → the relevant `docs/design/*.md` and `CHANGELOG.md`.
- Handoff written if the session is ending or compaction is imminent.


