# STATUS (updated 2026-09-22 by Claude)
Phase: 1 — summarization engine (started)        Active plan: docs/plans/2026-09-phase1-engine.md
North star reminder: G6 100% grounded metadata · G1 < 1 s save→raw-searchable, p50 < 20 s save→card

## Done (last 5)
- Phase 0 spike complete: thinking off + protocol prompt → 8–12 s/section, 0/24 structured-output failures at 8 workers, 9/9 dates grounded. Results in `docs/plans/2026-09-phase0-spike.md`; ADR-0001 records the worker decision; plan §4.2/§7/G1 updated to measured numbers.
- Cargo workspace (`mda-core`, `mda-cli`) with parser, card contract, config; 38 tests incl. snapshots and property tests; clippy pedantic clean.
- CI: fmt, clippy, tests on 3 OSes, MSRV, 70% coverage gate, cargo-deny, rustdoc, shellcheck, weekly audit, Dependabot; PR template with docs checklist; design-partner issue template.
- Dev workflow: `.claude/rules`, skills `/handoff /resume /adr /codex-review`, hooks under `scripts/dev/`.
- Codex (gpt-6-astra) pre-mortem triaged; mitigations folded into the plan.

## Next (max 3, in order)
1. Write `docs/plans/2026-09-phase1-engine.md`, then build store (SQLite + FTS5 cards + raw), section diff, planner, `claude-cli` worker with the retry table, validator (evidence normalisation), and `mda index|search|open|status`.
2. Codex review of the scaffold + parser (`/codex-review crates/`), triage, file under `docs/reviews/codex/`.
3. Owner: send the login-policy question to Anthropic; record the date in the spike plan.

## Blockers / open questions
- Login-policy confirmation (spike exit criterion) — owner action, not blocking code.
- §13 open decisions: default embedding model, commit `cards/` or not, single vs. separate MCP binary, per-root vs. global daemon.

## Last Codex review: 2026-09-21 (docs/reviews/codex/2026-09-21-pre-mortem.md) — 0 findings open, 2 rejected with reasons
