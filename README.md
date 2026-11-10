<p align="center">
  <img src="logo/mda_logo.jpg" alt="markdownattractor" width="320">
</p>

<h3 align="center">Claude answers from your ADRs and runbooks, and shows you the exact lines it used and when each section last changed.</h3>

<p align="center">
  A Claude Code plugin that keeps a live, searchable index of the markdown in your project.<br>
  One Rust binary. Your own Anthropic API key, or a free local model. Local only, no telemetry, never writes to your files.
</p>

<p align="center">
  <a href="https://github.com/joe-carr-data/markdownattractor/releases"><img alt="release" src="https://img.shields.io/github/v/release/joe-carr-data/markdownattractor?style=flat-square"></a>
  <a href="LICENSE"><img alt="license: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square"></a>
  <img alt="rust" src="https://img.shields.io/badge/built%20with-Rust-black?style=flat-square&logo=rust">
  <img alt="claude code plugin" src="https://img.shields.io/badge/Claude%20Code-plugin-d97706?style=flat-square">
  <a href="docs/benchmarks.md"><img alt="benchmarks: preregistered" src="https://img.shields.io/badge/benchmarks-preregistered-16a34a?style=flat-square"></a>
</p>

---

## The problem

Claude Code reads files whole. On a real project, *"how do we roll back a deploy?"* means grepping, opening several runbooks end to end, and reading hundreds of lines to find one paragraph. And it can't tell you whether that paragraph was written last week or two years ago.

## What markdownattractor does

A background daemon watches your folder. Every markdown file is split into sections. Each section is summarized once into a short **card** by Haiku through your own API key, or by a local model, and everything lands in a local index. Claude searches the index, reads the cards, and opens only the lines it needs.

```
you save deploy.md
        │  raw text searchable in ~2 s, card ready in ~8 s
        ▼
  ┌──────────────────────────────────────────────────────────┐
  │  search hit     runbooks/deploy.md › Rollback  L11–13    │
  │                 updated 2 days ago                       │
  │  section card   summary, entities, dates, questions      │
  │  exact lines    mda_open → only the lines that matter    │
  └──────────────────────────────────────────────────────────┘
```

Every hit carries provenance: **which file, which section, which lines, and when the section last changed.** In a git repository, times come from history: a section's change time is when the last commit touching its lines was authored, for every file that is committed and unmodified. Uncommitted edits, untracked files and folders without git use file times, and each time says which source it came from.

## What a session looks like

A real headless Claude Code session on the fictional demo corpus in [`evals/golden/docs`](evals/golden/docs) (an excerpt: the tool calls are verbatim, the results and the answer lightly trimmed):

```
> How do we roll back a failed deploy, and how long does it take?

⏺ mda_search("roll back a failed deploy, how long does rollback take")
  1. runbooks/deploy.md › Deploy runbook › Rollback            L11–13
  2. runbooks/deploy.md › Deploy runbook › Release             L5–9
  3. adr/0002-blue-green-deploys.md › ADR 0002 › Decision      L10–12

⏺ mda_open("58c690e807a880d0#2")    ⏺ mda_open("58c690e807a880d0#1")

  To roll back, run `deployctl rollback --to <previous-sha>` (runbooks/deploy.md:11-13):
  - Within 30 minutes of the release the previous colour is still warm, so traffic
    flips back within 20 seconds.
  - After that, a rollback is a full release of the old sha: about six minutes.
  When to roll back: watch the error-rate panel for ten minutes; above 1% is a
  rollback (runbooks/deploy.md:9).
```

One search, two sections opened, every claim cited to a line range. Ask *"what changed in the runbooks this week?"* and `mda_timeline` lists it, grouped by day.

## Install

Inside Claude Code:

```
/plugin marketplace add joe-carr-data/markdownattractor
/plugin install markdownattractor@markdownattractor
```

Then give it a way to write cards, and start it in your project:

```
export ANTHROPIC_API_KEY=…      # before launching Claude Code; or use a local model (below)
/mda start
```

The plugin downloads the binary for your platform from the GitHub release on the next session start and checks its checksum. `/mda start` indexes the folder and finishes with one real query against your first cards, so you see a hit with line ranges right away. Raw section text is searchable before any card exists, so search works while the first cards are still being written.

| Backend | What you need | Cost | Speed |
|---|---|---|---|
| **api** (default) | an [Anthropic API key](https://console.anthropic.com/) | Haiku list price; the 117-section demo corpus cost $0.31 | about 1 section/s at 16 workers |
| **local** | `brew install llama.cpp` and one command ([guide](docs/guides/local-model.md)) | $0 | depends on your machine |

Why not your Claude subscription? Anthropic's terms don't allow third-party tools to route requests through Pro or Max plan credentials on a user's behalf, so this project doesn't ship that as a supported path. Details in [ADR-0002](docs/adr/0002-backends-and-login-policy.md).

## What it does differently

| | |
|---|---|
| **Time is a first-class dimension** | Git history or file times, dates written *inside* the text, and index time are kept apart, per section. Search with `--since 7d`, or ask for a timeline of what the index saw created, changed, renamed or deleted. |
| **Summaries, not just chunks** | Cards carry a summary, the questions a section answers, entities and grounded dates, so queries match what people ask. Raw section text is indexed underneath, so nothing a card omitted is lost. |
| **Fresh, section by section** | A watcher plus a hash per section means only the section you edited is re-summarized. No rebuild step. Line ranges are re-checked when Claude opens them, and a changed section is flagged as stale instead of quoted wrong. |

## Commands

Everything is `/mda …` inside Claude Code, or `mda …` in a terminal. Every command supports `--json`. The search tools are also exposed to Claude over MCP. Other MCP clients (Codex CLI, Gemini CLI, Claude Desktop, Cursor, opencode) can use the same server: [setup guide](docs/guides/mcp-clients.md).

| | |
|---|---|
| **Lifecycle** | `start` · `stop` · `restart` · `status` · `watch` · `pause` · `resume` · `doctor` |
| **Search** | `search <q> [--since 7d] [--until …] [--in path] [--raw]` · `open <id>` · `card <id>` · `timeline` · `recent` · `stale` · `explain <q>` |
| **Indexing** | `index [path]` · `rebuild --embeddings` · `parse <file>` |
| **Configure** | `backend` · `embeddings` · `nudge` |
| **Meta** | `cost` · `diagnostics` · `schema` · `eval` |

## How it works

```
Watcher ─► Parser ─► Section diff ─► Workers ─► Validator ─► Index
(notify)   (comrak)  (blake3 per     (Haiku or   (evidence   (SQLite: FTS5 over cards and raw
                      section)        local)      check)      text, local embeddings, time)
```

- **Daemon** (`mda start`, one per folder): FSEvents/inotify watcher, debounced, `.gitignore`-aware. Renames keep their history without a model call. Never writes anything outside `.markdownattractor/`.
- **Section diff**: a blake3 hash per section. Unchanged sections never go to the model.
- **Workers**: an adaptive pool against the Messages API or a local OpenAI-compatible server, with structured output and a JSON schema. Concurrency ramps up on success and halves on rate limits.
- **Validator**: every extracted date and entity must quote an evidence substring that exists in the section, or it is dropped. On the demo corpus it dropped 16 dates and 49 entities.
- **Index**: SQLite with FTS5 over cards and over raw section text, plus card embeddings from a small local ONNX model (`bge-small-en-v1.5`, 33 MB, downloaded once), fused with reciprocal rank fusion. An optional recency prior is off by default.
- **MCP**: `mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_recent`, `mda_stale`, `mda_status`, served by the same binary (`mda mcp`).

Full design: [`docs/project-plan.md`](docs/project-plan.md).

## Guarantees

- **Never writes into your source files.** Identity lives in SQLite; your markdown is read-only to us.
- **Local only.** No cloud, no hosted service, no telemetry. The only network calls are the summarization requests with your key (none on the local backend) and the one-time download of the embedding model.
- **Grounded metadata.** Every stored date and entity quotes text that exists in its section; anything without evidence is dropped.
- **Honest benchmarks.** Corpora, comparators and gates are named before results exist. Every published number is rendered by a script from committed artifacts, preceded by a freeze, and a claim is made only where its preregistered gate passed. The headline claims, as measured ([benchmarks](docs/benchmarks.md); generated by `scripts/eval/t6.sh readme-row`, checked by `make check`):

<!-- benchmark-row:start -->
| Headline claim | Status, as measured (gates passed only) |
|---|---|
| Fewer source tokens at answer parity (T2, DocsQA test split; T6, our corpora) | No comparator pair passed all three gates; no savings claimed. |
| Fresh within seconds (T4) | Prisma, 20 edits, one machine: raw-searchable p50 1.7 s; card-ready p50 8.4 s (nearest-rank p50, as disclosed on the page). |
| Time questions correct with less effort at equal evidence (T5) | No pair met the correctness-and-effort criterion. |
| Retrieval: match qmd full (T1) | Hybrid point estimates met or exceeded qmd full on all 4 projects; all paired intervals included zero. |
<!-- benchmark-row:end -->

## How it compares

| | graphify | qmd | **markdownattractor** |
|---|---|---|---|
| Target | code, docs and media | markdown and docs | markdown |
| Freshness | `--update` rebuild | `update` command | live watcher, **per section** |
| Time model | — | — | **per-section change time from git, dates in the text, timeline** |
| Summaries | LLM concept graph | none | **LLM cards plus raw text** |
| Retrieval | graph traversal | BM25, vectors, rerank | BM25, vectors |
| Runtime | Python | Bun | **one Rust binary** |

All three are measured side by side, with grep, on the [benchmarks page](docs/benchmarks.md). We don't compete on scope: a code index covers the code, markdownattractor covers the prose around it.

## Status

**Released** (the badge above shows the current version; [changelog](CHANGELOG.md)). The engine, daemon, hybrid search, git-aware times, MCP server, search-first skill and release pipeline are shipped. Six benchmark tables are published (T1–T6). Relationships between documents (`supersedes`, `relates_to`) are not built yet.

Progress lives in [`docs/STATUS.md`](docs/STATUS.md). Lessons learned in [`docs/aha.md`](docs/aha.md).

## Contributing

Early users wanted: if you run Claude Code on a repo with a lot of markdown (ADRs, runbooks, specs, notes), open an issue titled `early user` and say roughly how many files. You get a direct line; we get the feedback that shapes the next release.

Read [`CLAUDE.md`](CLAUDE.md) for the golden rules before opening a PR. Every PR touching `crates/` gets a second-model review; findings are triaged in `docs/reviews/`, never silently dropped.

## License

Apache-2.0.
