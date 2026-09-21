# Phase 0 — Spike

Status: **done** · all exit criteria closed (login policy resolved by ADR-0002 on 2026-09-22) · 2026-09-21

Goal: replace guesses in the plan with measurements before writing the engine. Every number below came from real `claude -p` calls on this machine (Apple M3, Claude Code 2.1.278, Max plan, Haiku 4.5), on sections of `docs/project-plan.md`. Raw outputs are in the session scratchpad; the scripts are reproduced under `evals/spike/`.

## Tasks

- [x] Confirm the flags exist and combine: `--system-prompt`, `--json-schema`, `--output-format json`, `--tools ""`, `--setting-sources ""`, `--strict-mcp-config`, `--no-session-persistence`, `--max-budget-usd`. All present in 2.1.278.
- [x] Measure cold start, tokens per call with a replaced system prompt, and the default-prompt cost for comparison.
- [x] Measure Haiku latency and cost per section, with and without extended thinking.
- [x] Measure parallel behaviour at 4 and 8 workers: rate limits, failure modes.
- [x] Check date grounding quality on a section with nine dates.
- [x] Probe guardrail behaviours: budget cap, empty input, bad model, `--bare` without a key.
- [x] Login policy: resolved by ADR-0002 (the compliance page rules out routing through Pro/Max credentials; the API is the default). No approval needed for the shipped defaults.

## Results

### Call shape that works

```bash
MAX_THINKING_TOKENS=0 MARKDOWNATTRACTOR_WORKER=1 \
claude -p --model haiku \
  --system-prompt "$(cat prompts/section.v1.txt)" \
  --output-format json --json-schema "$(cat prompts/section.schema.v1.json)" \
  --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence \
  --max-budget-usd 0.05 \
  < chunk.md        # cwd = empty scratch dir
```

| Measurement | Value |
|---|---|
| CLI overhead (wall − API) | ~1.5–3 s per call |
| Input tokens with replaced system prompt (~1K-token section) | ~2,500 |
| Input tokens with the *default* system prompt | 8,481 cached + section; first call pays cache creation |
| Haiku API latency, thinking **on** (default) | 20–115 s (!) — 7–12K thinking tokens per call |
| Haiku API latency, `MAX_THINKING_TOKENS=0` | **7–12 s**, mean 8.7 s at 8 parallel |
| Haiku API latency, `MAX_THINKING_TOKENS=1024` | 14–32 s, mean 22 s |
| Sonnet, thinking on | 20 s (thinks less than Haiku: 600 tokens) |
| Output tokens per card, thinking off | 580–1,150 |
| Cost per section (list price, Haiku) | $0.006–0.011 |
| 8 parallel workers | no `rate_limit`/`overloaded` events, total wall 14 s |
| Structured-output failure rate, thinking off, plain prompt | **2–3 of 8** |
| Structured-output failure rate, thinking off, protocol prompt | **0 of 8** (three runs) |
| Date grounding (9 dates in one section) | 9/9 found, 9/9 evidence verbatim after quote normalisation |

### Findings that change the design

1. **Extended thinking is the latency problem, not the model.** Haiku 4.5 spends 7–12K tokens thinking about a 400-word section and takes up to two minutes. `MAX_THINKING_TOKENS=0` brings it to 7–12 s. The worker always sets it. The plan's 3–6 s target was wrong; the honest number is **8–12 s per section**.
2. **Structured output is a tool call, and Haiku sometimes doesn't make it on turn one.** With thinking off, the model tends to write the JSON as text; the CLI then sends a reminder turn ("call StructuredOutput"), which doubles input tokens and, in about a quarter of cases, ends with Haiku saying "I need the markdown section" because the reminder displaced the content. Fix: one line at the end of the system prompt, *"your ONLY action is to call the StructuredOutput tool… Call the tool immediately on your first turn"*. Result: 7 of 8 calls single-turn, 0 failures.
3. **Retry policy** (guardrails the worker implements):
   - `structured_output == null` → retry once with the same prompt, then escalate to Sonnet if configured, then mark the section `failed` with the raw `result` kept for diagnostics.
   - `is_error == true` or `api_error_status != null` → classify: 401/403 → stop the pool and surface "not logged in"; 404 → bad model, stop; 429/529 or `rate_limit`/`overloaded` in stream events → AIMD halve concurrency and retry with backoff (1 s, 4 s, 16 s); anything else → retry once then fail the job.
   - `subtype` is **not** a reliable signal (a 404 came back as `subtype: "success"` with `is_error: true`). Never branch on it alone.
   - Wall-clock timeout per call: 90 s (p99 seen ≈ 57 s on an outlier). Kill the process, count as a retryable failure.
   - `--max-budget-usd` per call (default 0.05) as a hard stop against runaway output; it is checked *after* the call, so it bounds damage, not spend.
   - Empty stdin → the CLI exits 1 immediately with a clear message; the planner never sends empty chunks, but the worker treats it as a permanent (non-retryable) failure.
4. **Deliver the section over stdin and close it.** The CLI waits up to 3 s for stdin when it is not a TTY; a worker that forgets to close stdin loses 3 s per call. Argument delivery works with `--` but gains nothing.
5. **Evidence check needs normalisation.** Haiku straightens curly quotes (“ ” → ") and sometimes dashes. The validator compares after NFKC + quote/dash folding + whitespace collapse. With that, grounding was 100%.
6. **Raw-text FTS is what makes G1 achievable.** Save → raw-searchable can be < 1 s (parse + FTS insert). Save → card is ~10–15 s for a small doc. The G1 metric is split accordingly (see plan §1).
7. **`--bare` is useless without an API key** (exits 1, empty result). Only the `api` backend uses it.

### Exit criteria

| Criterion | Status |
|---|---|
| Haiku grounding pass-rate ≥ 95% on the test docs | ✅ 100% on 9 dates (single section; expand to the 20-doc set in Phase 1 evals) |
| Defaults decided | ✅ Haiku, thinking off, concurrency start 4, protocol prompt v1, per-call budget $0.05, timeout 90 s |
| Policy answer received or dated follow-up + fallback pitch ready | ✅ closed 2026-09-22 by ADR-0002: API key default, local model second, CLI spawn opt-in only |

## Decisions taken

- ADR-0001 (to write): worker backend = spawned `claude -p`, thinking disabled, structured output via `--json-schema`, stdin delivery.
- Plan §4.2 and §7 updated to the measured numbers.
