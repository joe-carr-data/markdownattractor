---
name: handoff
description: Write a session handoff note to docs/handoffs/, refresh docs/STATUS.md and regenerate docs/index.md so the next session (or post-compaction context) starts grounded. Use at the end of a task, before compaction, or when asked to "hand off".
---

# /handoff

Purpose: nothing about this session survives in memory. Put it on disk.

## Steps

1. Gather state (read-only):
   - `git status --short` and `git diff --stat` for files touched and uncommitted work.
   - The task you were doing, the last thing that worked, the next concrete step.
   - Any open question the user has not answered.

2. Write `docs/handoffs/YYYY-MM-DD-HHMM.md` (local time, `date +%Y-%m-%d-%H%M`). At most 30 lines:

   ```markdown
   # Handoff YYYY-MM-DD HH:MM
   ## Doing
   one or two lines: the task and the plan it belongs to
   ## Files touched
   - path — what changed (one line each)
   ## Uncommitted intent
   what the uncommitted diff is trying to achieve; "none" if clean
   ## Next step
   one imperative line
   ## Open questions
   - ... or "none"
   ```

   If `docs/handoffs/` already has an `-auto.md` file from this session (written by a hook), keep it; yours is the narrative one.

3. Refresh `docs/STATUS.md`, keeping its fixed template and one-screen budget:
   - Header line: `# STATUS (updated YYYY-MM-DD by Claude)`.
   - `Phase:` and `Active plan:` if they changed.
   - `## Done (last 5)`: prepend what was finished; drop the oldest beyond 5.
   - `## Next (max 3, in order)`: rewrite from the active plan's unchecked tasks.
   - `## Blockers / open questions`: add or remove.
   - `## Last Codex review:` leave unless a review ran this session.

4. If something was learned the hard way, add one dated line at the top of `docs/aha.md`.

5. Run `scripts/dev/gen-index.sh` to regenerate `docs/index.md`.

6. Finish by suggesting, not running: `git commit -m "docs: handoff"`.

Do not touch `crates/` or any source file during a handoff.
