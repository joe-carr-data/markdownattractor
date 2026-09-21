---
name: codex-review
description: Run an external Codex review with a fixed reviewer prompt, triage every finding (accept/reject/defer), and write docs/reviews/codex/YYYY-MM-DD-<scope>.md. Argument: scope = pr | weekly | phase-N | security | <path>. Codex only reads; it never writes to the repo.
---

# /codex-review <scope>

`$ARGUMENTS` is the scope: `pr`, `weekly`, `phase-N`, `security`, or a path.

## 1. Assemble the packet

- **North star**: `docs/NORTH-STAR.md` if it exists, else the `## Goals` section of `CLAUDE.md`.
- **Design docs**: the `docs/design/*.md` files relevant to the scope (all of them for `weekly`, `phase-N`, `security`; those touched by the diff for `pr`; those matching the path otherwise). Skip silently if `docs/design/` does not exist yet.
- **Subject**:
  - `pr` → `git diff main...HEAD` (fall back to `git diff HEAD` if there is no `main`).
  - `weekly` / `phase-N` → `git ls-files crates docs/design docs/adr` as a file list plus `git log --oneline -20`.
  - `security` → `git ls-files scripts .claude crates/mda-cli` plus any bootstrap/install script.
  - path → that file or directory listing.
- **Questions**: up to 5, specific to the scope. Defaults: `pr` — correctness, error handling, unsafe/async misuse, missing tests; `weekly` — drift from north star, stale docs, dependency risk; `phase-N` — "what would you have done differently"; `security` — path traversal, worker sandboxing, supply chain, hook injection.
- Pin: `git rev-parse HEAD`.

## 2. Run Codex (read-only)

Use the Agent tool with `subagent_type: "codex:codex-rescue"`. Tell it the packet lives in the repo at the paths above and pass this prompt verbatim after the packet:

> You are the reviewer of record. Read the packet. Do not modify any file. Return findings only, ranked by severity. For each finding: `F<n> — <title> — Critical|High|Medium|Low`, then `file:line` (or section for docs), one-line evidence, one-line fix, and `confidence: high|medium|low`. Answer the numbered questions at the end. No praise, no summary of what the code does.

Record the model and CLI version Codex reports. If Codex is unavailable, stop and say so; do not substitute your own review as a "Codex" review.

## 3. Triage every finding

For each finding, one of:
- **accept** → add a checkbox task to the active plan named in `docs/STATUS.md` (or to `## Next` if no plan exists), referencing `F<n>`.
- **reject** → record the reason in the triage table. A finding rejected twice across reviews gets an ADR.
- **defer** → an ADR if it is a decision, otherwise a `## Follow-ups` line.

No finding is silently dropped.

## 4. Write the review file

`docs/reviews/codex/YYYY-MM-DD-<scope>.md`, same layout as `docs/reviews/codex/2026-09-21-pre-mortem.md`:

1. `# Codex review — <scope>` and a header table: Date, Scope, Reviewer (CLI version, model, effort), Pinned to (commit SHA), Triaged by, Outcome (counts).
2. `## 1. Packet` — the prompt given, the inputs, the questions.
3. `## 2. Raw findings` — Codex output verbatim (only link formatting may be fixed).
4. `## 3. Triage` — table: `# | Severity | Decision | Reason | Where it landed`.
5. `## 4. Patterns → rules` — accepted findings that reveal a pattern become a line for `docs/aha.md` or a rule in `.claude/rules/`.
6. `## 5. Follow-ups`.

## 5. Link it

- Update the `## Last Codex review:` line in `docs/STATUS.md`: date, path, `<n> findings open`.
- Run `scripts/dev/gen-index.sh` (the PostToolUse hook does this on Write).

## Guardrails

- Codex never writes to the repo. If the subagent reports it changed files, revert them with `git checkout --` and note it in the review.
- Reviews are pinned to a SHA; re-run against the same SHA to re-check.
