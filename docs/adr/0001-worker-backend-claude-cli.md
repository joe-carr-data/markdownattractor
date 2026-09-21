# ADR-0001 — Summarization workers spawn `claude -p` with thinking disabled and structured output

Status: **Accepted** · 2026-09-22 (amended same day, see §Amendments) · Supersedes: — · Evidence: `docs/plans/2026-09-phase0-spike.md`

## Context

Every section of every markdown file needs a structured card. The plan's zero-config goal (G2) rules out asking for an API key, so summaries must go through the user's existing Claude Code login. The only sanctioned way to do that from a third-party process is to spawn the user's own `claude` CLI; reading OAuth tokens from the keychain or `~/.claude` is off the table (golden rule 7).

The Phase 0 spike measured the call shape on Claude Code 2.1.278 with Haiku 4.5:

| Variant | API latency | Failure rate at 8 parallel |
|---|---|---|
| default (extended thinking on) | 20–115 s, 7–12K thinking tokens | 0 % |
| `MAX_THINKING_TOKENS=0`, plain prompt | 7–12 s | 25–37 % (structured output missing) |
| `MAX_THINKING_TOKENS=0`, protocol line in prompt | 7–12 s | **0 %** (3 × 8 runs) |
| `MAX_THINKING_TOKENS=1024` | 14–32 s | 0 % |

The structured-output failures came from Haiku writing the JSON as text on turn one; the CLI then sends a reminder turn, which sometimes displaces the section content.

## Decision

1. **Backend `claude-cli` is the default.** The daemon spawns `claude -p` per chunk with: `--model <cfg>`, `--system-prompt <prompts/section.vN.txt>`, `--output-format json`, `--json-schema <generated from SectionSummary>`, `--tools ""`, `--setting-sources ""`, `--strict-mcp-config`, `--no-session-persistence`, `--max-budget-usd <cfg>`; env `MAX_THINKING_TOKENS=0` and `MARKDOWNATTRACTOR_WORKER=1`; cwd an empty scratch directory; chunk over stdin, stdin closed immediately.
2. **Extended thinking is disabled for summarization.** It is the dominant latency cost and adds nothing to a 400-word section summary.
3. **The system prompt ends with a response-protocol line** instructing the model to call the StructuredOutput tool on its first turn and never reply with text.
4. **The Rust type `SectionSummary` is the schema's source of truth.** `mda schema section` prints it; `prompts/section.schema.v1.json` is a generated copy checked by a test.
5. **Worker results are trusted but verified**: the JSON is re-validated in Rust, evidence strings are checked against the section after quote/dash/whitespace normalisation, and the retry table in plan §4.2 applies. `subtype` is never used as a signal.
6. **`api` backend (`claude --bare -p` with `ANTHROPIC_API_KEY`) is optional**, opt-in, and shares everything above except the auth path.

## Consequences

- Latency budget per section is 8–12 s API plus ~2 s CLI overhead. The plan's §7 was updated; the warm pool is now the lowest-impact lever and is built last.
- A future Claude Code release could change flag behaviour; `mda doctor` verifies the flags and the `--version`, and CI runs a smoke test against the latest CLI when a login is available.
- If Anthropic's policy answer (spike exit criterion, still open) forbids spawning the CLI from a plugin, the `api` backend becomes the default and the README's "no API key" line goes; the code path does not change.
- Fable-class models cannot have thinking disabled; they are never valid summarization models. `mda summarization_model` rejects them with a message.

## Amendments

- **2026-09-22 — data delimiters and prompt v2.** The first live run showed that a section whose content looks like instructions (a code block with a `claude -p … --json-schema` line) makes Haiku ask for "the section" instead of summarizing it. The user message is now `<section path="…" heading="…">…</section>` and the prompt states that everything inside is data. 6/6 on the failing chunk afterwards; `PROMPT_VERSION = section.v2`.
- **2026-09-22 — escalation defaults to Sonnet.** `escalation_model` defaults to `sonnet`: after two Haiku failures on a section, one Sonnet attempt. Sonnet costs ~3× per call but only runs on the sections Haiku cannot handle, so the blended cost stays close to Haiku's.
- **2026-09-22 — heading-only sections never reach the model.** They get a deterministic card (tldr = the heading) so they are searchable and cost nothing.

## Alternatives considered

- **Agent SDK (Python/TypeScript)** — adds a runtime the plugin promised not to need, and the SDK's login policy note points at the same constraint.
- **Direct Messages API** — requires an API key; kept as the `api` backend's future fast path.
- **Keep thinking with a small budget** — twice the latency for no measurable quality gain on this task; rejected.
- **Local model for summaries** — no model download blocking first use is a goal (G2); revisit for an offline mode later.
