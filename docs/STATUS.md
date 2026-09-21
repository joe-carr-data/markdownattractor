# STATUS (updated 2026-09-21 by Claude)
Phase: 0 — spike (not started)        Active plan: none yet (write docs/plans/2026-09-phase0-spike.md)
North star reminder: G6 100% grounded metadata · G1 p50 < 15 s save→searchable

## Done (last 5)
- Project plan written (`docs/project-plan.md`).
- Codex (gpt-6-astra) pre-mortem run on the plan; 11 findings triaged (9 accepted, 1 partial, 2 rejected), mitigations folded into §2.4, §4, §5, §6, §8, §9.5, §11, §12.
- Repo initialised; `CLAUDE.md` charter, `README.md`, `docs/index.md`, `docs/aha.md` written.

## Next (max 3, in order)
1. Write `docs/plans/2026-09-phase0-spike.md` from §8 Phase 0 (exit criteria included).
2. Run the spike: `claude -p` cold start, `--system-prompt` token cost, warm pool, rate limits at 4/8/16 workers.
3. Request the login-policy confirmation from Anthropic; draft the API-key-only README variant.

## Blockers / open questions
- §13 open decisions: default embedding model, commit `cards/` or not, single vs. separate MCP binary, per-root vs. global daemon.

## Last Codex review: 2026-09-21 (docs/reviews/codex/2026-09-21-pre-mortem.md) — 0 findings open, 2 rejected with reasons
