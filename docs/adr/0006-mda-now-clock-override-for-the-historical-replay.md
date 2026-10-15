# ADR 0006: MDA_NOW clock override for the historical replay

Date: 2026-10-15
Status: Accepted

## Context
Benchmark table T5 (execution plan §4 T5, plan `docs/plans/2026-09-m8-t5-replay.md`) replays a repository's history into mda's store: the harness checks a fixed commit range out step by step, sets file mtimes from git author dates and indexes each step, so that the stored `first_seen_at`, `updated_at`, tombstone and event times are the history's dates and can be validated against `git log`. Section `updated_at` already follows the file's mtime, but `first_seen_at`, events, tombstones and `summarized_at` were taken from `Timestamp::now()` at seventeen call sites across `pipeline`, `store`, `search`, `daemon` and `timefmt`, and the CLI had four more. Without one seam a replay would stamp every historical step with the real date and the time model could not be measured. Goal G6 (never hallucinated metadata) rules out patching dates into the store after the fact.

## Decision
We route every "now" in `mda-core` and `mda-cli` through one function, `mda_core::clock::now()`, which returns the real clock unless the environment variable `MDA_NOW` holds an RFC 3339 instant or a `YYYY-MM-DD` date, read once per process. The override is a test and benchmark facility, not a user feature: a malformed value is refused by `clock::check()`, which `Engine::open` calls before anything is indexed or served (fail closed), and the daemon refuses to run with the variable set, so the override only ever applies to inline runs such as `MDA_NOW=<author date> mda index`. Relative forms (`7d`) are rejected because they would be relative to the clock being overridden.

## Consequences
- Positive: the T5 replay produces a store whose dates are git's dates and can be validated exactly; `timeline`, `recent` and relative searches evaluated under the same override are consistent with it; the one seam is unit-tested (parse, real-clock fallback) and integration-tested (fixed timestamps at index time, refusal of a malformed value, the daemon's refusal).
- Negative: a process-wide environment read is global state; a user who sets `MDA_NOW` by accident gets an index dated in the past (the variable name is deliberately specific and the behaviour is documented only in this ADR, the runbook and the module docs, never in user-facing help).
- Follow-ups: the replay harness (`scripts/eval/t5.sh`) validates stored times against `git log` and voids a replay on any mismatch; `mda status --json` may expose the active override for the harness's preflight; `created_at_source` stays `first_seen` under a replay because birth times cannot be set from git, which the T5 page discloses.

## Alternatives considered
- A `--now` CLI flag on `index` — would need threading through `Engine`, the store and the daemon's indexer, and would invite use as a feature; the environment seam reaches every call site without changing any signature.
- Rewriting timestamps in the store after each replay step — contradicts G6 and golden rule 3 (metadata must come from evidence); the validation against git would be testing the rewrite, not the product.
- Faking the system clock (`libfaketime`) — platform-specific, invisible to the harness's provenance, and it would also move the summariser's wall-clock and timeouts.
- A trait-object clock injected into `Engine` — the right shape for a library with many clocks, but every caller including the CLI would need it and the daemon refusal would still be an environment check; deferred until a second consumer appears.
