# Workflow rules

## Planning
- Enter plan mode before any medium or large task (more than one file, any new module, any schema change).
- Write the plan to `docs/plans/<yyyy-mm>-<phase-or-topic>.md` before writing code.
- Every plan has: goal, task list with checkboxes, exit criteria. No exit criteria, no plan.
- `docs/STATUS.md` names the active plan. Update it when the active plan changes.

## Commits
- Conventional commits: `feat|fix|docs|chore|test|refactor(scope): summary`.
- Scope is the crate or area: `core`, `cli`, `docs`, `hooks`, `skills`, `plan`.
- Small commits: one logical change each. Never mix a refactor with a behaviour change.
- Commit only when asked. Never commit to `main` directly; branch first.
- PRs touching `crates/` get a Codex review (`/codex-review pr`) before merge.
- PR body carries a "Docs updated" checklist: STATUS, aha, index, ADR if a decision was made.

## When to ask the user
- Ask before anything destructive: deleting files, rewriting history, dropping tables, force pushes.
- Ask when the plan is ambiguous and the choice changes the design (new crate, new schema, new public API).
- Otherwise do not ask. Make the smaller, reversible choice and record it in `docs/aha.md` or an ADR.

## Session hygiene
- Start with `/resume`. End with `/handoff`.
- Hooks under `scripts/dev/` exit 0 fast; if one breaks, fix it before continuing.
