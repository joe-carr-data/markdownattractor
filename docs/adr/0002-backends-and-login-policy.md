# ADR-0002 — Summarization backends: API key by default, local model second, CLI spawn opt-in only

Status: **Accepted** · 2026-09-22 · Amends ADR-0001 · Owner decision

## Context

ADR-0001 chose to spawn the user's own `claude -p` for summaries so that no API key was needed. Two documents rule that out as a default for a public plugin:

- Claude Code, *Legal and compliance*: "OAuth authentication is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and Enterprise subscription plans and is designed to support ordinary use of Claude Code and other native Anthropic applications. Developers building products or services that interact with Claude's capabilities, including those using the Agent SDK, should use API key authentication … Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their users."
- Agent SDK overview: "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK."

The timeline matters: subscription tokens outside official apps were first blocked in January 2026, the Terms were revised in February, and from April 2026 Pro/Max access was cut off for third-party harnesses, with Anthropic citing cache-unfriendly "invoke the model fresh every time" usage — which is exactly what a background summarization daemon does. A daemon firing hundreds of automated calls through `claude -p` is technically "running Claude Code" while behaving like a harness. That is the risky middle a public repository should not occupy.

Every comparable plugin avoids it: Understand Anything and llm-wiki-plugin do their model work *inside* the user's interactive session; graphify uses provider API keys or a local Ollama fallback; CodeGraph, claudix, claude-vault, claude-context-local and qmd use no remote model at all. None spawns `claude -p` from a daemon.

## Decision

1. **`api` is the default backend.** The engine calls the Messages API directly from Rust with the user's own key (`ANTHROPIC_API_KEY`, configurable via `api_key_env`). Haiku 4.5 (`claude-haiku-4-5`) for cards, one Sonnet 5 (`claude-sonnet-5`) attempt after two failures. Structured outputs via `output_config.format` (single turn, no reminder round), the system prompt cached with `cache_control`, no extended thinking.
2. **`local` is the documented second option.** An OpenAI-compatible server (llama.cpp with `gpt-oss-20b` is the reference setup, `docs/guides/local-model.md`), zero cost, no policy question, works while Claude Code is closed. JSON-schema-constrained output; `reasoning_effort: low` for gpt-oss.
3. **`claude-cli` stays in the code, opt-in only, unadvertised.** Selecting it requires `claude_cli_policy_ack = true` in the config; the error message quotes the policy. It is never the default and does not appear in the README's pitch. If Anthropic approves the use case (request sent via the contact-sales form, the channel the compliance page names), this ADR is revisited.
4. **In-session summarization** (Claude Code's own subagents doing the work during the user's session, the Understand Anything pattern) goes to the backlog as B-0002: unambiguously permitted, but interactive and context-hungry.

## Consequences

- The README's headline changes from "no API key" to "bring your own key, or run a local model". The first-run flow asks for a key or points at `mda backend local`.
- The API backend is faster and cheaper per section than the CLI path: one API turn instead of two, cache reads on the system prompt, no CLI startup. Measured numbers replace the spike's in `docs/design/summarization.md` when available.
- Cost becomes visible dollars instead of plan usage; `mda cost` and the daily budget already exist.
- `mda doctor` checks per backend: key present and a `GET /v1/models` succeeds; local server reachable and reports the model; `claude` on PATH and acknowledgement set.
- The engine gains an HTTP client (`reqwest` with rustls); binary size grows by a few MB.
- The spike's policy exit criterion is closed by this decision rather than by an answer from Anthropic.

## Alternatives considered

- **Keep `claude-cli` default and ask for approval first** — leaves a public repo shipping a pattern the Terms name, on the hope of an exception. Rejected.
- **In-session only** — permitted but ties summarization to an open session and consumes its context; kept as backlog.
- **Agent SDK** — same policy note applies; adds a runtime. Rejected.
