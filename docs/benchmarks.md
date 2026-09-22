# Benchmarks

Numbers measured on this repository, never tuned to look good. Corpora are fixed before results are seen (plan §11). Apple M3, 24 GB, macOS 15, release builds unless stated.

## Retrieval — golden set (`evals/golden`, 32 docs, 117 sections, 60 queries, 16 temporal)

A query's expected sections are alternatives (any of them answers it), so the headline metric is **success@5** (an expected section is in the top 5); **MRR@5** is the mean reciprocal rank of the first expected section within the top 5. The plan calls this recall@5; the eval harness names it precisely.

`mda eval --golden evals/golden -k 5`, cards recorded once with Haiku 4.5 through the `api` backend ($0.31, 117 cards, 0 failures, 45 s).

| Run | success@5 | MRR@5 | mean query, in-process |
|---|---|---|---|
| lexical, raw text only (no cards) | 0.883 | 0.747 | 0.8 ms |
| lexical, cards + raw | 0.900 | 0.777 | 2.2 ms |
| **hybrid, cards + raw + vectors** (bge-small-en-v1.5-q) | **0.983** | **0.853** | 60 ms |

- G5 asks for success@5 (the plan's recall@5) ≥ 0.85: met by every run; hybrid leaves one miss out of 60 ("ship a new version of the API" → the v1 deprecation section outranks the release runbook).
- The seven lexical misses were all paraphrases ("undo the last release", "mass logout incident", "escalation policy for outages", "new hire reading list", …); vectors recover six.
- Hybrid query time is the query embedding (ONNX on CPU), not the scan: the dot-product scan over 117 vectors is microseconds, and the same scan over 10K synthetic 384-d vectors is under 10 ms.

## Search latency — CLI process on `docs/` (18 files, 163 sections)

| Command | p50 wall (fresh process) |
|---|---|
| `mda search … --raw` (lexical) | 20 ms |
| `mda search …` (hybrid) | 257 ms, of which ≈ 200 ms is loading the embedding model |

The MCP server keeps the model loaded, so a tool call pays the ≈ 50 ms embedding, not the load. The plan's 30 ms hybrid budget is not met by the query embedding itself; lexical search meets it with margin. Levers if it matters: a smaller query encoder, ONNX thread settings, or caching query vectors.

## Indexing and summarization (from Phase 1)

| What | Number |
|---|---|
| Parse + raw index, full `docs/` (119 sections) | 28 ms |
| Save → raw-searchable (daemon, 1 s debounce) | 1.27 s |
| Save → card attached (daemon, Haiku via `api`) | 4.57 s |
| Backfill of 61 sections at daemon start | 60 s, $0.28 |
| Haiku via `api`, 10 sections | 15 s wall, 1 turn each, $0.043 |
| Embedding 162 cards with bge-small (first run, includes the 33 MB download) | 38 s |

## Answer-quality A/B with the parity gate (plan §11) — golden corpus, 2026-09-22

`scripts/eval/ab.sh` runs each of the 12 questions in `evals/ab/questions.jsonl` through headless `claude -p` (Sonnet) twice: **baseline** (Read, Grep, Glob over the corpus) and **index** (the same plus the `mda` MCP server and the search-first rules). `scripts/eval/grade.sh` scores both answers against a reference with Sonnet (correctness + completeness, 0–6). Savings count only where the index answer scores at least the baseline. Full table: `evals/ab/results/2026-09-22-golden.md`.

| | baseline | index |
|---|---|---|
| Parity (index score ≥ baseline) | | **12 of 12** |
| Median source tokens read per answer (what the tools returned) | **341** | 1,305 |
| Median total input tokens per answer (incl. system prompt, tool schemas) | 38,141 | 32,548 |
| Mean tool calls | 2.4 | 1.6 |
| Mean wall-clock | 8.2 s | 6.9 s |
| Total cost, 12 questions | $0.215 | $0.269 |

**The index does not save source tokens on this corpus, and this page says so.** The golden set is 32 files and 450 lines: one `Grep` and one `Read` fetch the answer in a few hundred tokens, while eight search hits with their cards are about 1,300 tokens whatever the corpus size. Answer quality was the same in both arms (the four sub-perfect grades were the same omissions on both sides). Fewer turns and less wall-clock with the index are real but small. G3 (≥ 5× fewer source tokens at parity) is a claim about corpora where grep-and-read costs thousands of tokens per question; it has not been measured on one yet, and the README makes no token-saving claim until it has.

Two levers the numbers point at: a leaner hit payload (`k` and per-hit fields are the whole 1,300), and corpora of realistic size (the plan's ≥ 5 corpora from ~50 to ~5K docs, still to be named).

## Not measured yet

- The A/B protocol on corpora beyond the golden set (design-partner repos; this repository's own `docs/` is a candidate at 28 files / 5K lines).
