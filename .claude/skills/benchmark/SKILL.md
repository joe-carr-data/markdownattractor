---
name: benchmark
description: Reproduce the markdownattractor benchmark (DocsQA-Repo retrieval, coverage gate, golden-corpus A/B) from public data by following evals/benchmark_it_with_claude.md step by step, checking every checksum and tolerance, and writing a reproduction report. Use when asked to benchmark, reproduce the numbers, or verify docs/benchmarks.md.
---

# /benchmark — reproduce the published numbers

Read `evals/benchmark_it_with_claude.md` in full first. Then, in order:

1. Confirm prerequisites (§1): toolchain, `mda --version` equal to the table's `FROZEN.md`, Claude Code logged in, `ANTHROPIC_API_KEY` unset, `MDA_MODEL_DIR` exported.
2. Data (§2): clone and verify checksums; stop and report if any checksum differs.
3. Index and coverage (§3): run the adapter per project; compare every count with the expected table; differences are findings.
4. Cards (§4): use the committed cards when they exist; otherwise regenerate through the owner's own Claude Code login and record the wall-clock.
5. Answer quality (§5) only if asked or if the table under verification needs it (it spends Claude Code usage).
6. Write the report (§7) under `evals/results/reproductions/`, numbers next to expected, tolerance verdicts, deviations. Never edit `evals/results/docsqa/` or `docs/benchmarks.md` from a reproduction.

Never tune, never pass `--open-holdout`, never rerun a step until it matches: a mismatch outside tolerance is the result.
