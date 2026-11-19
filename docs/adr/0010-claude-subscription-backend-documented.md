# ADR-0010 — The Claude subscription backend is documented, still opt-in

Status: **Accepted** · 2026-11-19 · Supersedes decision 3 of ADR-0002 · Owner decision

## Context

ADR-0002 (2026-09-22) made the API key the default and kept the `claude-cli` backend (the user's own `claude -p`, signed in with their Claude plan) in the code, opt-in and unadvertised. The README said Anthropic's terms do not allow third-party tools to route requests through Pro or Max credentials, and the backend's refusal message said the same.

The owner asked on 2026-11-19 whether that is still true. Anthropic's *Legal and compliance* page for Claude Code (https://code.claude.com/docs/en/legal-and-compliance), read the same day, still says that OAuth authentication "is designed to support ordinary use of Claude Code and other native Anthropic applications", that developers building products should use API keys, and that Anthropic does not permit third-party developers "to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users". It now also says:

- the restriction does not "prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription";
- developers "may not collect, store, or intermediate Claude.ai credentials or session tokens" and sign-in must complete through Anthropic's own flow;
- "Advertised usage limits for Pro, Max, and Ultra plans assume ordinary, individual usage of Claude Code and the Agent SDK";
- Anthropic may enforce these restrictions without prior notice.

`claude-cli` runs the user's own, unmodified `claude` binary, signed in by the user through Anthropic's flow; mda never sees, stores or passes on the login (golden rule 7), so it meets the credential conditions. The page does not say whether automated background calls made by a third-party tool through that binary count as the permitted sign-in or as the restricted routing; that question stays open, together with volume: a daemon summarising many sections is arguably more than ordinary individual use, and the plan's limits and Anthropic's enforcement fall on the user. Documenting the backend is this project's decision on that reading, not a statement that Anthropic approves it.

## Decision

We document the `claude-cli` backend in the README, next to `api` and `local`, as "use your Claude subscription", with the command (`mda backend claude-cli --i-accept-the-policy`), what it does (runs your own signed-in `claude`; mda never touches the login) and what it costs (it counts against the plan's usage limits, which Anthropic sets for ordinary individual use and may enforce without notice). It stays opt-in: `api` remains the default, and selecting `claude-cli` still requires the acknowledgement flag, whose message now states the current terms instead of saying they forbid it.

## Consequences

- Positive: users with a Claude plan and no API key can get cards without a second bill; the README no longer states a reading of the terms that the page no longer supports.
- Negative: users who choose it may hit their plan's limits or Anthropic's enforcement; the README says so plainly. If `ANTHROPIC_API_KEY` is set, the spawned `claude` bills the key, not the plan; the README says to unset it. Launch readers may still raise the terms; the answer is this ADR and the quoted page.
- Also (Codex review of PR #122): `mda backend` repairing a config that names `claude-cli` without the acknowledgement used to save the defaults over the user's other settings; it now keeps them (`Config::load_unvalidated`).
- Follow-ups: re-read the terms page before each release that touches this backend; if Anthropic narrows the allowance, revert the README section by a new ADR.

## Alternatives considered

- Keep it unadvertised (ADR-0002 as is) — the owner chose to document it; the old README sentence was also stronger than the current terms.
- Make it the default when no key is set — no: the volume question is real, the user should choose it knowingly.
