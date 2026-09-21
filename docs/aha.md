# aha.md — things learned the hard way

Dated one-liners. Newest first. Pruned monthly: entries that became rules graduate to `.claude/rules/`, the rest go to `archive/aha-YYYY-MM.md`. Keep under 60 lines.

- 2026-09-21 — Codex CLI 0.155.1 returned "requires a newer version of Codex" for `gpt-6-astra` once, then accepted it minutes later after a Claude Code restart. Server-side gating, not a client bug; retry before upgrading.
- 2026-09-21 — `codex exec` refuses to run outside a git repo unless `--skip-git-repo-check` is passed. Init the repo before wiring `/codex-review`.
- 2026-09-21 — Pre-mortem framing ("it's 4 months later and it failed, why?") got sharper findings from Codex than a plain review would. Reuse the frame at each phase gate.
- 2026-09-21 — Shipping an adoption lever as opt-in is the same as not shipping it. The nudge is now default-on.
