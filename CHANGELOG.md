# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Benchmark M2** (execution plan §5): the competitor arms. `scripts/eval/arms/qmd.sh` (qmd 2.8.3: one index per project with the MDX mask, MCP `query` in full, no-rerank and lex-only modes through the `mcp_time` client), `scripts/eval/arms/graphify.sh` (graphify 0.9.66 built headlessly on a checkout copy with its own skill and hooks, `graphify` and `graphify-haiku` configurations, completion decided from the process, partial graphs quarantined, per-model usage recorded, graphs archived), `scripts/eval/bm25-files.sh` (the whole-page FTS5 control), `crates/mda-cli/examples/mcp_time.rs` + `scripts/eval/mcp-time.sh` (one MCP client for every arm's latency and results). Probes for five arms; the preflight requires them for every tool-driven frozen arm, checks artifact identity and regenerates every external-arm result. Development rows for every arm on the four DocsQA projects on `docs/benchmarks.md`; runbook §4c.
- **Benchmark M1** (execution plan §5): `mda eval --dataset docsqa --export-cards <file>` writes a corpus's cards as `{"<section_hash>": <SectionSummary>}` plus a provenance sidecar, and `--arm-output <jsonl>` scores an external arm's ranked paths with the same page rule and metrics as the store's rows (`missing` and `unknown` ids listed, no latency column). The report carries `card_coverage` (every section carded or not). `scripts/eval/freeze.sh` writes and checks `FROZEN.md`; `scripts/eval/preflight.sh` runs the machine-checked preflight (frozen inputs, binary, model files, store completeness, exact regeneration, reconstruction from the committed cards, coverage per arm, three activation probes per arm through `scripts/eval/probe.sh`); `scripts/eval/table.sh` renders the DocsQA table from the raw results. The four DocsQA corpora's cards are committed under `evals/results/docsqa/` with `model.sha`. After the Codex review (`docs/reviews/codex/2026-09-23-bench-m1.md`): `results.json` archives every question's first ten distinct pages (`pages`, was `top` with five) so the metrics regenerate from the file; the preflight's `passed` needs a completed run and every required check; `model.sha` lists the snapshot links the loader opens; a probe counts only a successful tool result.

### Changed
- **Leaner `mda_search` payload over MCP** (benchmark plan B0b). Default `k` is 5 (was 8); each hit keeps the CLI's field names but drops the ranking diagnostics (`score`, `vector`, `vector_score`, `title`), sends `snippet` only when the section has no card, `pending` only when true, `updated_at` to the second, and the OR-fallback flag once as `partial`. The CLI `--json` shape is unchanged. Numbers in `docs/benchmarks.md`.
- `scripts/eval/grade.sh` grades through `claude -p` with a JSON schema instead of the Messages API (no API key in `evals/`); an answer with no grade is reported as ungraded, never as zero; medians are conventional.

### Added
- **`mda eval --dataset docsqa`** (benchmark plan B0/B2): the DocsQA-Repo adapter. Loads a project from a `docsqa-data` checkout, maps its page labels to repository paths, reports ingestion coverage (labels indexed, questions excluded for image-derived evidence or missing pages), assigns a seeded dev/test/holdout split, and scores success@5, MRR@5 and nDCG@10 at page granularity for the raw, carded and hybrid configurations; `--out` writes `coverage.json`, `split.json`, `results.json`. Metrics, coverage and the split live in `mda_core::eval`.

### Added
- **`.mdx` ingestion** (`docs/design/ingestion.md`). The walker accepts `.mdx`; the parser excludes a leading block of `import`/`export` statements from sections the way it excludes front matter, keeps a heading that sits inside a JSX or HTML block without a blank line before it (CommonMark would swallow it), and falls back for the document title to front matter `title:` (YAML or TOML) and then to `export const title = "…"`. JSX, expressions and template tags stay as text. Search snippets skip markup-only lines. Checked on the four DocsQA-Repo corpora at their pinned commits: every heading kept, every page titled except partials. The search-first skill and the nudge hook cover `.mdx` too.

## [0.1.1] — 2026-09-22

### Fixed
- The plugin failed to load when installed from the marketplace: `plugin.json` pointed at `hooks/hooks.json`, `skills/` and `.mcp.json`, which Claude Code loads automatically from their standard locations and then rejected as duplicates ("Duplicate hooks file detected"). The three keys are gone; a `--plugin-dir` load had masked it.

## [0.1.0] — 2026-09-22

First tagged release: five prebuilt archives and `SHA256SUMS` on GitHub Releases, fetched by the SessionStart hook.

### Added
- **Release pipeline (ADR-0005).** `.github/workflows/release.yml` builds `mda-{darwin,linux,windows}-{arm64,x64}.tar.gz` and `SHA256SUMS` on every `v*` tag (dry run through `workflow_dispatch`), installs the linux-x64 archive through the real `scripts/bootstrap.sh` as its gate, signs and notarises macOS binaries when the Apple secrets exist, and publishes a GitHub Release. Intel macOS gets a lexical-only build. `scripts/dev/check-version.sh` (also in CI) keeps `VERSION`, `plugin.json`, `marketplace.json` and `Cargo.toml` in step; `scripts/dev/bump-version.sh` changes them together. `docs/design/distribution.md`.
- **First-run experience (plan §9.5).** `mda start` on a fresh root waits for the first cards (bounded, 60 s) and prints one real example query with its hit and line range; `--no-example` skips it and the SessionStart hook passes it. `mda cost [--since]` reads the usage ledger per day and per model/outcome and says that tokens saved on reads are not measured yet. `mda diagnostics [--out]` writes a redacted bundle (home directory, workspace id and hot paths redacted; no document content) for issue reports. `mda nudge on|off [--global]` switches the search-first reminder per root (`config.toml`) or everywhere (`${CLAUDE_PLUGIN_DATA}/nudge.off` through `MDA_NUDGE_FILE`); the hook honours both.
- `mda index <directory>` treats the directory as the root instead of failing with "is a directory".
- The plugin's MCP server was watched loading in a real Claude Code session (`claude -p --plugin-dir`): tools appear as `mcp__plugin_markdownattractor_markdownattractor__mda_*` and answer; recorded in `docs/design/mcp.md`.
- **Search layer (ADR-0004).** Card embeddings with a local `bge-small-en-v1.5` model (fastembed, static ONNX Runtime, downloaded once, never inside a query) as a third fused list; `mda explain` shows cards, raw and vector lists; `mda embeddings local-small|off`, `mda rebuild --embeddings`. Golden set: hybrid success@5 0.983 vs 0.883 lexical.
- **MCP server.** `mda mcp` (stdio, rmcp 3) with `mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_recent`, `mda_stale`, `mda_status`; declared by the plugin in `.mcp.json`.
- `mda timeline`, `mda recent`, `mda stale`, `mda eval --golden` (success@k, MRR@k, `--record`); `evals/golden` with 32 documents and 60 queries; `docs/benchmarks.md`.
- **Daemon (ADR-0003).** `mda start` watches a folder and keeps its index live: `notify` watcher, debounced intake with a size-stable check, rename detection by content hash (history kept, zero model calls), ignore-rule reconciliation, hot-path-first bounded summarization rounds with exponential backoff, and a newline-JSON control socket. `mda stop|restart|watch|pause|resume`; `status` and `doctor` show the live daemon; `mda index` delegates to a running daemon; the SessionStart hook restarts the daemon for previously indexed projects. Measured on this repo's docs: raw-searchable 1.3 s after save, card 4.6 s after save.
- Codex review of the daemon step (16 findings, all addressed): symlink-safe state files, OS file lock for one-daemon-per-root, `BEGIN IMMEDIATE` store transactions, environment failures defer instead of failing sections, bounded IPC.

### Changed
- MSRV is 1.89 (`File::try_lock`).
- **Backends (ADR-0002).** The Claude Messages API with your own key is the default; an OpenAI-compatible local server (llama.cpp + gpt-oss-20b documented) is the second option; spawning `claude -p` is opt-in only and requires acknowledging Anthropic's third-party login policy. `mda backend`, per-backend `mda doctor` checks, `docs/guides/local-model.md`.

### Added
- `mda index | search | open | card | status`: parse markdown into heading-delimited sections with exact line ranges, make them raw-searchable immediately (FTS5), summarize new sections through the user's own `claude -p` (Haiku, thinking off, structured output, Sonnet escalation), validate every card with evidence grounding, and search with reciprocal rank fusion over cards and raw text plus a recency prior and `--since/--until/--in` filters.
- Read-time staleness check: `mda open` re-hashes the section and returns the current lines with `stale: true` if the file changed after indexing.
- Deterministic cards for heading-only sections; `--retry-failed`, `--limit`, and daily token budget enforcement for large corpora.
- Plugin skeleton: manifest, repo-as-marketplace catalog, `/mda` and `search-first` skills, bootstrap hook, default-on nudge.
- Cargo workspace (`mda-core`, `mda-cli`), CI (fmt, clippy, tests on three OSes, MSRV, coverage gate, cargo-deny, rustdoc, shellcheck), weekly security audit, Dependabot.
- Development workflow hooks and skills under `.claude/` and `scripts/dev/`.
- Project plan, Codex pre-mortem review, charter.
