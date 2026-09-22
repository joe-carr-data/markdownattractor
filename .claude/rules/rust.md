# Rust rules

## Workspace
- Two crates: `crates/mda-core` (library: parser, differ, index, cards, MCP) and `crates/mda-cli` (binary: CLI, daemon entry, hooks).
- All logic lives in `mda-core`; `mda-cli` is argument parsing, wiring and output formatting only.

## Errors and diagnostics
- Library errors: `thiserror` enums per module. No `Box<dyn Error>` in public signatures.
- `anyhow` only at the binary edge (`mda-cli` `main` and command handlers).
- No `unwrap()` or `expect()` outside `#[cfg(test)]`. Use `?`, `ok_or`, or a typed error.
- Diagnostics go through `tracing` (`debug!`, `info!`, `warn!`, `error!`), never `println!`/`eprintln!`.
  The only `println!` is the CLI's final one-line output (and `--json`).

## Quality gates (all must pass before a commit is proposed)
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` (or `cargo nextest run`)
- Every public item (`pub fn`, `pub struct`, `pub enum`, `pub mod`) has a doc comment. `#![warn(missing_docs)]` in `mda-core`.

## Dependencies
- No new dependency without an ADR in `docs/adr/`. Reference the ADR number in the `Cargo.toml` comment.
- Prefer crates already listed in `docs/project-plan.md` §16.

## Tests
- Unit tests next to the code in `mod tests`.
- Integration tests in `crates/<crate>/tests/`.
- Snapshot tests with `insta` for parser output and card rendering; review snapshots with `cargo insta review`, never accept blind.
- Property tests with `proptest` for the section differ (idempotence, hash stability, line-range refresh).
- Test names say what they prove: `parses_nested_headings_into_sections`, not `test1`.

## From the 2026-09-22 Codex review
- Every path that reaches the filesystem goes through `Engine::rel_path` / `Engine::safe_join`. The walker's exclusions are not a security boundary.
- Spend is a ledger, not a property of success: anything that calls a model records its usage whatever the outcome.
- "Not seen this round" is not "deleted". Tombstone only on confirmed absence.
- Child processes: write stdin, drain stdout and stderr, and wait — concurrently, under a timeout.

## From the 2026-09-22 Codex daemon review
- A watcher event is a hint, never a fact, and reads are not events: filter `Access` at the source, then look at the filesystem.
- Read-then-write store transactions begin `IMMEDIATE` (`Store::write_tx`); the busy timeout cannot rescue a deferred upgrade.
- A stopped run is not a failed section: cancel, stop and pool-stopping outcomes defer; only the model's answer or the validator fails a section.
- Exclusivity is an OS file lock, never "does the socket answer".
- Every file under `.markdownattractor/` is created through `config::write_private` / `state_dir`, which refuse symlinks.
