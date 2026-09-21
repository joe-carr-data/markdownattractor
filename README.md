<p align="center">
  <img src="logo/mda_logo.jpg" alt="markdownattractor" width="320">
</p>

<h3 align="center">Claude reads your markdown <em>before</em> it reads your files.</h3>

<p align="center">
  A Claude Code plugin that turns every folder of markdown into a live, time-aware, searchable knowledge layer.<br>
  One Rust binary. Bring your own API key, or run a local model and pay nothing. Nothing else leaves your machine.
</p>

<p align="center">
  <a href="#status"><img alt="status: design" src="https://img.shields.io/badge/status-v0%20design-7c3aed?style=flat-square"></a>
  <a href="LICENSE"><img alt="license: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square"></a>
  <img alt="rust" src="https://img.shields.io/badge/built%20with-Rust-black?style=flat-square&logo=rust">
  <img alt="claude code plugin" src="https://img.shields.io/badge/Claude%20Code-plugin-d97706?style=flat-square">
</p>

---

## The problem

Claude Code reads files whole. On a real project, answering *"how do we roll back a deploy?"* means grepping, opening three runbooks, and burning 40K tokens to find one paragraph. Worse: it can't tell you whether that paragraph was written last week or two years ago.

## What markdownattractor does

A background daemon watches your folder. Every markdown file is parsed into sections, each section is summarized once into a ~100-token **card** by Haiku through your own API key (or by a local model such as gpt-oss-20b), and everything lands in a local hybrid index. Claude searches the index, reads a card, and opens only the 40 lines it needs.

```
you save deploy.md
        │  < 15 s
        ▼
  ┌─────────────────────────────────────────────────────────┐
  │  L0  search hit      "Rollback · deploy.md §Rollback     │
  │                       lines 212–340 · updated 2d ago"   │
  │  L1  section card     tldr, entities, dates, questions   │
  │  L2  doc card         summary, TOC with line ranges      │
  │  ──  exact lines      mda_open → the 40 lines that matter│
  └─────────────────────────────────────────────────────────┘
```

Every hit carries provenance: **which file, which section, which lines, and when it was created, changed, and last seen.**

### Three things nobody else does

| | |
|---|---|
| **Time is a first-class dimension** | Four clocks, never conflated: filesystem/git time, dates *inside* the text, index time, and validity. Ask *"what changed in the runbooks since Monday?"* and get an answer. |
| **Summaries, not just chunks** | Cards carry `questions_answered`, entities, decisions and grounded dates. Queries match what people ask, not what the file happens to contain. A raw-text index sits underneath as a safety net, so nothing the card omitted is ever lost. |
| **Always fresh, section by section** | A watcher plus per-section hashing means only the paragraph you edited gets re-summarized. No rebuild step, ever. Line ranges are re-checked at read time, so Claude never quotes a stale excerpt. |

## Quick start

> **Not released yet.** The design is done and reviewed; the spike starts next. The commands below are the contract we are building to. Watch the repo or open an issue if you want to be a design partner.

```
/plugin install markdownattractor --marketplace joe-carr-data/markdownattractor
export ANTHROPIC_API_KEY=…        # or: /mda backend local   (see below)
/mda start
```

That's it. `/mda start` confirms the folder, indexes it with sensible defaults, and finishes by running one real query against your first indexed docs so you see a hit with line ranges before you walk away. Target: **install to first useful answer in under ten minutes.**

No Python. No Node. No model download blocking first use. Two ways to pay for summaries, both visible in `/mda cost`:

| Backend | What you need | Cost | Speed |
|---|---|---|---|
| **api** (default) | an [Anthropic API key](https://console.anthropic.com/) | Haiku list price, ≈ $0.006 per section, with a daily cap you set | ≈ 1 section/s at 16 workers |
| **local** | `brew install llama.cpp` and one command ([guide](docs/guides/local-model.md)) | $0 | depends on your machine |

Why not your Claude subscription? Anthropic's terms don't allow third-party tools to route requests through Pro or Max plan credentials on a user's behalf, and this project won't ship a pattern the terms name. Details in [ADR-0002](docs/adr/0002-backends-and-login-policy.md).

## What a session looks like

```
> how do we roll back a failed deploy?

⏺ mda_search("rollback failed deploy", status=current)
  1. deploy-runbook.md › Deploy › Rollback         lines 212–340   updated 2 days ago   0.91
  2. adr/0007-blue-green.md › Consequences         lines 40–58     updated 3 months ago 0.74
  3. incidents/2026-08-14.md › Timeline            lines 12–90     updated 5 weeks ago  0.61

⏺ mda_open("deploy-runbook.md#rollback")
  → 128 lines read instead of 2,400

  To roll back, run `deployctl rollback --to <previous-sha>` …
```

Claude read 5% of the file and knows the answer is two days old. Ask *"what changed in the runbooks since Monday?"* and `mda_timeline` lists it, grouped by day, with the sections that moved.

## Commands

Everything is `/markdownattractor …` or the short form `/mda …`. The search tools are also exposed to Claude over MCP.

| | |
|---|---|
| **Lifecycle** | `start` · `stop` · `restart` · `status` · `watch` · `doctor` |
| **Indexing** | `index [path]` · `pause` · `resume` · `rebuild` · `ignore <pattern>` · `prune` |
| **Search** | `search <q> [--since 7d] [--status current] [--in path]` · `open <id>` · `timeline` · `recent` · `stale` · `explain <q>` |
| **Configure** | `summarization_model` · `escalation_model` · `backend` · `embeddings` · `concurrency` · `budget` · `retention` · `nudge` · `config` |
| **Meta** | `cost` · `diagnostics` · `export` · `logs` · `reset` · `version` · `update` · `help` |

The full reference with flags is in [`docs/project-plan.md` §6](docs/project-plan.md#6-command-ux--markdownattractor).

## How it works

```
Watcher ─► Parser ─► Section diff ─► Planner ─► Workers ─► Validator ─► Reducer ─► Index
(notify)   (comrak)  (blake3/sect.)  (chunks)   (claude -p)  (evidence)  (doc card)  (FTS5 + sqlite-vec)
```

- **Watcher**: FSEvents/inotify, debounced, `.gitignore`-aware. Never touches `.markdownattractor/`.
- **Section diff**: a blake3 hash per section. Unchanged sections never go to the LLM.
- **Workers**: a warm pool of `claude -p` processes with a replaced system prompt, no tools, and a JSON schema for the output. Adaptive concurrency backs off on rate limits.
- **Validator**: every extracted date and entity must quote an `evidence` substring that exists in the section, or it is dropped. Metadata is never hallucinated into the index.
- **Index**: SQLite with FTS5 over cards, FTS5 over raw section text, `sqlite-vec` over card embeddings from a small local ONNX model, fused with reciprocal rank fusion and a recency prior. Search over 10K sections in under 30 ms.
- **MCP**: `mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_related`, `mda_status`, served by the same binary.

Full design: [`docs/project-plan.md`](docs/project-plan.md).

## Guarantees

- **Never writes into your source files.** Identity lives in SQLite; your markdown is read-only to us.
- **Local only.** No cloud, no hosted service, no telemetry. The only network calls are the summarization requests to the API with your key (none at all on the local backend), and the one-time download of the embedding model.
- **Grounded metadata.** 100% of dates and entities in the index are backed by text in the source.
- **Fallback that works.** Cards are plain markdown on disk. With MCP off, `Grep` over `cards/` still works.
- **Honest benchmarks.** Token savings are only reported for queries where the with-index answer is at least as correct as the baseline. Corpora are named before results exist, and the small-repo numbers get published even when the gain is nil.

## How it compares

| | graphify | CodeGraph | qmd | **markdownattractor** |
|---|---|---|---|---|
| Target | code + docs + media | code | markdown | markdown |
| Freshness | rebuild on commit | live watcher | manual | live, **section-level** |
| Time model | — | — | — | **4 clocks, section `updated_at`** |
| Summaries | LLM concept graph | none | none | **LLM cards + raw-text safety net** |
| Retrieval | graph traversal | FTS5 symbols | BM25 + vectors + rerank | BM25 + vectors + recency |
| Auth | provider API keys | none | local models | **your API key, or a local model** |
| Runtime | Python | Node | Bun | **one Rust binary** |

We don't compete on scope. A code index covers the code; markdownattractor covers the prose around it. Install both.

## Status

**v0 design, September 2026.** The plan has been through an adversarial pre-mortem by a second model ([findings and triage](docs/reviews/codex/2026-09-21-pre-mortem.md)) and the mitigations are folded in.

| Phase | What | State |
|---|---|---|
| 0 | Spike: cost and latency, Haiku grounding quality, backend policy | done |
| 1 | Summarization engine, raw + card search, core `/mda` commands, API and local backends | engine done, daemon next |
| 2 | Vectors, hybrid ranking, time filters, MCP server, search-first skill, default-on nudge | |
| 3 | Relationships (`relates_to`, `supersedes`) and wiki index pages | |
| 4 | Distribution, benchmarks page, design-partner program, launch | |

Progress lives in [`docs/STATUS.md`](docs/STATUS.md). Lessons learned in [`docs/aha.md`](docs/aha.md).

## Contributing

Design partners wanted: if you run Claude Code on a repo with a lot of markdown (ADRs, runbooks, specs, notes), open an issue titled `design partner` and say roughly how many files. You get early builds and a direct line; we get a real corpus for the benchmarks.

Read [`CLAUDE.md`](CLAUDE.md) for the golden rules before opening a PR. Every PR touching `crates/` gets a second-model review; findings are triaged in `docs/reviews/`, never silently dropped.

## License

Apache-2.0.
