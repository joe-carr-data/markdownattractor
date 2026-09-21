# Contributing

Thanks for looking. Read [`CLAUDE.md`](CLAUDE.md) first: the golden rules there apply to humans too.

## Setup

```
rustup toolchain install 1.98.1   # or whatever rust-toolchain.toml says
cargo install cargo-binstall && cargo binstall -y cargo-nextest cargo-deny cargo-llvm-cov cargo-insta
make check
```

`make check` runs rustfmt, clippy with `-D warnings`, the test suite under nextest, cargo-deny, and a warning-free rustdoc build. It is exactly what CI runs. If it passes locally, CI passes.

## Layout

| Path | What |
|---|---|
| `crates/mda-core` | The library: parser, section diff, planner, worker pool, validator, store, index, search. No I/O with the user. |
| `crates/mda-cli` | The `mda` binary: daemon, CLI commands, MCP server. Thin; all logic lives in core. |
| `prompts/` | Versioned system prompts and JSON schemas the workers use. Changing one bumps `prompt_version`. |
| `evals/` | Golden docs, queries, expected hits. `mda eval` runs them offline. |
| `docs/` | Design, decisions, status. See [`docs/index.md`](docs/index.md). |
| `scripts/dev/` | Hook scripts for the development workflow. Not shipped in the plugin. |

## Rules of the road

- **Tests are not optional.** Unit tests next to the code, integration tests in `tests/`, snapshot tests with `insta` for anything that renders (cards, CLI output), property tests with `proptest` for the differ and planner. Coverage gate in CI is 70% and only goes up.
- **No `unwrap()` outside tests.** Library errors are `thiserror` enums; `anyhow` only at the binary edge.
- **No new dependency without an ADR.** Run `/adr <title>` or copy the template in `docs/adr/`.
- **Never write into the watched root.** If your change touches the filesystem, the test must prove the source tree is byte-identical afterwards.
- **Commits**: `feat|fix|docs|test|refactor|chore(scope): summary`. Small. One idea each.
- **PRs**: the template has a "Docs updated" checklist. It is not decorative.

## Review

Every PR touching `crates/` gets a second-model review (Codex) besides the human one. Findings are triaged in `docs/reviews/codex/`, never silently dropped. If you disagree with a finding, say so in the triage table; a finding rejected twice gets an ADR.

## Reporting a security issue

See [`SECURITY.md`](SECURITY.md). Please don't open a public issue for vulnerabilities.
