---
name: benchmark
description: Reproduce the markdownattractor benchmark (DocsQA-Repo retrieval, coverage gate, golden-corpus A/B) from public data by following evals/benchmark_it_with_claude.md step by step, asserting every checksum, distinguishing regeneration (must be exact) from independent reruns (published beside the original, never gated), and writing a reproduction report. Use when asked to benchmark, reproduce the numbers, or verify docs/benchmarks.md.
---

# /benchmark — reproduce the published numbers

Read `evals/benchmark_it_with_claude.md` in full first. Then, in order:

1. Environment (§1): run the asserted block as written from `$REPO`; every `|| exit 1` is a stop-and-report; keep `$REPO`, `$RUN` and `$MDA` absolute for every later command.
2. Data (§2): fresh `$RUN` directory; clones refuse an existing target; every checksum and SHA is asserted.
3. Raw index and coverage (§3): every count is exact; a difference is a finding.
4. Cards (§4): 4a committed cards (regeneration, exact) when the table has them; 4b regeneration through the user's own Claude Code login only when asked (hours of wall-clock), published as an independent rerun.
5. Answer quality (§5): 5a regeneration from archived logs when the table has them; 5b independent rerun only when asked (spends Claude Code usage); read resolved model ids from the run logs.
6. Report (§8) under `evals/results/reproductions/`, every attempt logged, regeneration marked exact yes/no, reruns with the paired difference. Never edit `evals/results/docsqa/` or `docs/benchmarks.md` from a reproduction.

Never tune, never pass `--open-holdout`, never rerun a step "until it matches": a regeneration mismatch is the result, and a rerun's difference is published, not judged.
