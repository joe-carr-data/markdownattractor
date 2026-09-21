# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- **Backends (ADR-0002).** The Claude Messages API with your own key is the default; an OpenAI-compatible local server (llama.cpp + gpt-oss-20b documented) is the second option; spawning `claude -p` is opt-in only and requires acknowledging Anthropic's third-party login policy. `mda backend`, per-backend `mda doctor` checks, `docs/guides/local-model.md`.

### Added
- `mda index | search | open | card | status`: parse markdown into heading-delimited sections with exact line ranges, make them raw-searchable immediately (FTS5), summarize new sections through the user's own `claude -p` (Haiku, thinking off, structured output, Sonnet escalation), validate every card with evidence grounding, and search with reciprocal rank fusion over cards and raw text plus a recency prior and `--since/--until/--in` filters.
- Read-time staleness check: `mda open` re-hashes the section and returns the current lines with `stale: true` if the file changed after indexing.
- Deterministic cards for heading-only sections; `--retry-failed`, `--limit`, and daily token budget enforcement for large corpora.
- Plugin skeleton: manifest, repo-as-marketplace catalog, `/mda` and `search-first` skills, bootstrap hook, default-on nudge.
- Cargo workspace (`mda-core`, `mda-cli`), CI (fmt, clippy, tests on three OSes, MSRV, coverage gate, cargo-deny, rustdoc, shellcheck), weekly security audit, Dependabot.
- Development workflow hooks and skills under `.claude/` and `scripts/dev/`.
- Project plan, Codex pre-mortem review, charter.
