# Benchmarks

Numbers measured on this repository, never tuned to look good. Corpora are fixed before results are seen (plan §11). Apple M3, 24 GB, macOS 15, release builds unless stated.

## Retrieval — golden set (`evals/golden`, 32 docs, 117 sections, 60 queries, 16 temporal)

`mda eval --golden evals/golden -k 5`, cards recorded once with Haiku 4.5 through the `api` backend ($0.31, 117 cards, 0 failures, 45 s).

| Run | recall@5 | MRR | mean query, in-process |
|---|---|---|---|
| lexical, raw text only (no cards) | 0.883 | 0.747 | 0.8 ms |
| lexical, cards + raw | 0.900 | 0.777 | 2.2 ms |
| **hybrid, cards + raw + vectors** (bge-small-en-v1.5-q) | **0.983** | **0.853** | 60 ms |

- G5 asks for recall@5 ≥ 0.85: met by every run; hybrid leaves one miss out of 60 ("ship a new version of the API" → the v1 deprecation section outranks the release runbook).
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

## Not measured yet

- The answer-quality A/B parity protocol (plan §11): same question with and without the index through `claude -p`, Sonnet-graded, tokens and tool calls compared. No token-saving claim is made until it runs.
- Corpora beyond this repository's docs and the golden set.
