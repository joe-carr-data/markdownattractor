# Documentation rules

Before ending any task, in this order:

1. Update `docs/STATUS.md` (T0, one screen): phase, active plan, Done (last 5), Next (max 3), Blockers, Last Codex review.
   If it grows past one screen, move the excess to `docs/plans/` or `docs/archive/`. Never expand T0.
2. If something was learned the hard way, add one dated line to the top of `docs/aha.md` (keep it under 60 lines).
3. If a decision was made (crate, storage, schema, auth path, public API), write an ADR with `/adr <title>`.
   ADRs live in `docs/adr/NNNN-kebab-title.md`, are never edited after Accepted; supersede instead.
4. Regenerate `docs/index.md` with `scripts/dev/gen-index.sh`. Never hand-edit the generated table;
   the "Not yet written" section at the bottom is preserved verbatim and may be edited by hand.
5. If user-visible behaviour changed, update the relevant `docs/design/*.md` and `CHANGELOG.md`.

Other rules:
- `docs/handoffs/` is written by hooks (`pre-compact.sh`, `session-end.sh`) and by `/handoff`. Do not write there by hand.
- `docs/reviews/codex/` holds every Codex review with its triage. No finding is silently dropped.
- `docs/project-plan.md` is reference tier (T2): read sections, never the whole file, and do not restate it elsewhere.
- Prefer editing an existing doc over creating a new one. New docs must appear in `docs/index.md` (the hook does this).
- Dates are ISO `YYYY-MM-DD`. Newest first in every list.
