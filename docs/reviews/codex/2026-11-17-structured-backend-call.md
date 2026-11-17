# Codex review — the schema-constrained backend call (2026-11-17, PR #90)

Reviewer: Codex (`gpt-6-astra`, fresh thread `01a1eb3f-5535-7563-8c2d-c549dee86d55`, read-only, static review, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: the three commits of PR #90 (two refactors of the `claude -p` backend, then `Backend::structured`).

**Verdict:** "Changes requested." On the refactors: "preserve the inspected early-return classifications, guardrail precedence, error strings, logging fields, empty-input rejection, and timeout/kill behavior."

## Findings and triage (5: 4 P2, 1 P3; all accepted and fixed in the PR)

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | P2 | A response with no accounting fields got default zeros and `usage: Some(…)`: an unaccounted attempt looked free. | **Fixed.** `usage` is `Some` only when the body has a `usage` object (CLI: a `usage` object or a cost); `from_text` and `truncated` take `Option<Usage>`. Tests: missing, explicit zero and populated usage. |
| 2 | P2 | `local`: a refusal (`message.refusal`) was malformed output; `finish_reason: "content_filter"` with object-shaped content passed as `Ok`. | **Fixed.** Both are decided before the content is read and are `Fatal` with usage kept. Tests for both, and for `length` and empty output. Whether a given local server emits them is not established. |
| 3 | P2 | `claude-cli` silently ignored `max_output_tokens`. | **Fixed by enforcing after the fact, and documented.** The CLI has no output-token flag; an answer whose reported output is above the cap is rejected as malformed with its usage kept (`enforce_output_cap`), so a caller never receives more than it asked for. The spend itself is bounded by the per-call dollar budget, not by the cap; `StructuredRequest::max_output_tokens` says so. Test added. |
| 4 | P2 | Non-200 responses discarded any usage in their body. | **Fixed.** `api` and `local` read an optional `usage` object from a parseable error body before mapping the status. Test added. That servers do this is a hypothesis; the loss for such a body was certain. |
| 5 | P3 | The new shell fixtures interpolated temporary paths unquoted. | **Fixed.** Paths are single-quoted. |

## Other points from the review

- `wall_ms` on the CLI now starts after argv and user-message construction (microseconds earlier work excluded). **Accepted as is**: not a behaviour any caller depends on.
- Required method on a public trait: breaks external implementors. **Accepted as is**: the crate is not published and the workspace holds every implementation.
- CLI argv assertions by substring; the reused `dead_url` helper can race on port reuse. **Deferred**: the argv test already checks the order of the isolation flags line by line; the `dead_url` race predates this change and applies to the card tests as well. Backlog: exact argv comparison; a `dead_url` that keeps the port reserved.
- Mock and unavailable report a known zero usage when no call is made. **Intended.**
