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

### Same corpus after the lean MCP payload (benchmark plan B0b, 2026-09-22; exploratory)

`mda_search` over MCP now returns five hits without ranking diagnostics and without a snippet when a card exists (`docs/design/mcp.md`). Same corpus and questions; both arms re-run the same day; one run per arm; the grader moved to `claude -p` and the search-first instructions changed with the payload, so the comparison is indicative, not controlled. Tokens are the runner's estimate (chars/4 for source tokens). Full table and the list of what changed: `evals/ab/results/2026-09-22-golden-lean.md`.

| | before | after |
|---|---|---|
| Parity (index score ≥ baseline) | 12 of 12 | 11 of 12 (one question 4 vs 5) |
| Mean score, index / baseline | 5.67 / 5.58 | 5.50 / 5.42 |
| Median source tokens read, index (all 12 questions) | 1,304.5 | **762.5** |
| Median total input tokens, index (all 12) | 32,523 | 48,506.5 |
| Mean tool calls, index | 1.5 | 2.8 |

The search result itself shrank from ≈ 1,400 to ≈ 400 tokens and the median source tokens per answer fell by 42%, but Claude now opens sections it used to answer from the tldr, so it takes more turns and total input tokens (the whole context, once per turn) went up. On this corpus the index still reads three times the baseline's source tokens; the break-even size will be measured on the DocsQA corpora (plan B2).

## DocsQA-Repo — ingestion gate and the first axis-A row (benchmark plan B2, 2026-09-22)

Source-repository adaptation of DocsQA-Repo (`PowderXu/docsqa-data` schema v3; `evals/README.md`): the four repositories indexed at their pinned commits, labels mapped to repository paths, every question assigned to the seeded split (seed 20260922). Numbers below are the **development split** only (rule 0.2), the raw-text lexical configuration (BM25 over section text, no cards, no vectors), one run, debug binary (latency is indicative; the adapter fetches deeper until it holds ten distinct pages, so a page that hogs the section list cannot hide the next one). Raw files: `evals/results/docsqa/<project>/{coverage,split,results}.json`.

| Project | docs / sections indexed | labels indexed (coverage) | evidence anchors found | questions → eligible | dev/test/holdout | dev scored | success@5 | MRR@5 | nDCG@10 | mean ms |
|---|---|---|---|---|---|---|---|---|---|---|
| github-docs | 3742 / 23066 | 260/260 (100%) | 183/222 | 197 → 161 (36 image-evidence) | 59/108/30 | 49 | 0.306 | 0.175 | 0.227 | 401 |
| prisma | 693 / 10438 | 179/179 (100%) | 176/176 | 125 → 118 (7 image-evidence) | 37/68/20 | 37 | 0.216 | 0.108 | 0.132 | 245 |
| supabase | 836 / 6548 | 63/63 (100%) | 47/48 | 52 → 40 (12 image-evidence) | 15/28/9 | 12 | 0.333 | 0.118 | 0.208 | 381 |
| tailwind-css | 198 / 1518 | 99/99 (100%) | 96/96 | 93 → 84 (9 image-evidence) | 27/51/15 | 25 | 0.600 | 0.340 | 0.420 | 52 |

- **Ingestion gate (plan §2 F1): passed on all four projects.** (iii) 100% of the 601 labels map to an indexed file (the `.mdx` work of B0a made three projects possible). (iv) 64 questions whose reference evidence is image-derived text are excluded and counted; none for a missing page. (v) The evidence-presence check compares the dataset's resolved section anchors (`anchor_resolution`, canonical headings from the rendered pages) with the headings and titles of our indexed source, after folding case, backticks, Liquid tags and whitespace: Tailwind and Prisma 100%, Supabase 47 of 48, **GitHub Docs 183 of 222**. Of GitHub Docs' 39 misses, 35 sit on pages that render Liquid includes or version variants (`{% data reusables… %}`, `{% ifversion %}`; a heading such as "About {% data variables.product.prodname_registry %}" renders as "About GitHub Packages"), which the source-repository adaptation cannot reproduce without the site's variable data; the affected questions stay eligible because their label is the page, and their ids are listed in `coverage.json`. This is the adaptation's stated limitation, not a parser gap.
- The raw-lexical row is the floor, not the product: DocsQA questions are long community questions ("how do I…", with error strings and context), which an AND query over every term rarely matches, so most queries fall to the OR form and BM25 over raw text ranks the page with the most repeated words. Cards (`questions_answered`, `tldr`) and vectors are what the plan expects to move these numbers; the carded and hybrid rows come with the committed cards (B2, `claude-cli` backend per §0a.3).
- **Partial cards bias the fusion.** Tailwind's index carries cards and vectors for 216 of 1,518 sections (14%, from the carding-rate measurement below), and on that index the carded and hybrid rows are far *below* raw: reciprocal-rank fusion gives every carded section a place on the cards list and on the vector list, so the 14% that happen to be carded crowd out uncarded relevant pages. The rows are published for what they are; the real carded numbers need every section carded, and the product implication (a daemon half-way through its first backfill can rank worse than raw) goes to the plan as a follow-up.

| Tailwind, dev split, 14% of sections carded | n | success@5 | MRR@5 | nDCG@10 | mean ms |
|---|---|---|---|---|---|
| lexical (raw only) | 25 | 0.600 | 0.340 | 0.420 | 52 |
| lexical (cards + raw) | 25 | 0.360 | 0.211 | 0.309 | 63 |
| hybrid (cards + raw + vectors) | 25 | 0.120 | 0.038 | 0.060 | 204 |

- **Carding rate through the owner's Claude Code login** (`claude-cli` backend, Haiku 4.5, the plan's §0a.3): 200 Tailwind sections in 182 s wall (13 s of it embedding), 0 failures, 509K input / 69K output tokens, **$0.85 list-price equivalent**; ≈ 1.1 sections/s. The four corpora are ≈ 41.6K sections: ≈ 10 h and ≈ $175 list-price equivalent, which is above the plan's $60 card figure and is an owner decision before it runs (plan §6).
- Query latency on the three larger corpora is 0.25–0.42 s per question in this configuration (long OR queries, deep candidate lists, one row read per candidate), far above the 30 ms budget; a lever to measure before axis B (fewer candidates, a prepared statement per section lookup, or the release build).

## Not measured yet

- The A/B protocol on corpora beyond the golden set (design-partner repos; this repository's own `docs/` is a candidate at 28 files / 5K lines).
