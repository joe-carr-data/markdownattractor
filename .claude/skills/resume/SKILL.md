---
name: resume
description: Orient at the start of a session. Reads docs/STATUS.md, the active plan, docs/aha.md and the newest handoff, then states the next task in one line. Use when starting work, after compaction, or when asked "where were we".
---

# /resume

Purpose: recover the exact state from disk instead of reconstructing it.

## Steps

1. Read `docs/STATUS.md` in full.
2. Parse the `Active plan:` path from its second line. If it names a file under `docs/plans/`, read that file. If it says "none yet", the next task is to write the plan it suggests.
3. Read `docs/aha.md` (short; skim all of it).
4. If `docs/handoffs/` exists and is non-empty, read only the newest file (`ls -t docs/handoffs | head -1`). Prefer a narrative handoff over an `-auto.md` one from the same session if both exist.
5. Cross-check: if the handoff's "Next step" and STATUS's "Next 1." disagree, trust the handoff (it is newer) and say so.

## Output

Exactly this shape, then stop and wait:

```
Phase <n> — <name>. Active plan: <path or none>.
Next: <one imperative line>.
```

Optionally one more line for a blocker that prevents the next task.

## Rules

- Ask a question only if the plan is ambiguous about what to do next (two candidate tasks with no order, or exit criteria missing). Otherwise do not ask; state the task.
- Do not start the task inside `/resume`. The user confirms or redirects first.
- Do not read `docs/project-plan.md` here; it is reference tier. Read a section of it only when the task needs it.
