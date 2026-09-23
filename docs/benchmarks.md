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

- **Carding rate through the owner's Claude Code login** (`claude-cli` backend, Haiku 4.5, the plan's §0a.3): 200 Tailwind sections in 182 s wall (13 s of it embedding), 0 failures, 509K input / 69K output tokens, **$0.85 list-price equivalent**; ≈ 1.1 sections/s. The four corpora are ≈ 41.6K sections: ≈ 10 h and ≈ $175 list-price equivalent; the equivalent is reported for readers who would run this on the API, it is not what the run cost (the owner's Max plan, plan §0a.3).
- Query latency on the three larger corpora is 0.25–0.42 s per question in this configuration (long OR queries, deep candidate lists, one row read per candidate), far above the 30 ms budget; a lever to measure before axis B (fewer candidates, a prepared statement per section lookup, or the release build).

### DocsQA-Repo — carded and hybrid rows at full coverage (development, 2026-09-23)

The first numbers that measure the product's own design rather than the raw floor. Every section of the four checkouts carries a card (36,899 cards through the owner's Claude Code login, `claude-cli` backend, Haiku 4.5, prompt `section.v2`; a few tiny sections carded deterministically) and every card a vector (`bge-small-en-v1.5-q`). The cards are committed (`evals/results/docsqa/cards-0.1.1-<project>.json`, rule 0.9) and the run is the first one under a freeze (`evals/results/docsqa/FROZEN.md`, protocol *development*): dev split only, one run, release build, in-process adapter timing. **Development numbers are for tuning and are never the published T1 result** (execution plan §2.0); T1 comes from the test split after the tuning loop and the competitor arms (M3–M4).

| Project | dev scored | run | success@5 | MRR@5 | nDCG@10 | mean ms | truncated |
|---|---|---|---|---|---|---|---|
| github-docs | 49 | lexical (raw only) | 0.306 | 0.175 | 0.227 | 476 | 0 |
| github-docs | 49 | lexical (cards + raw) | 0.388 | 0.249 | 0.295 | 959 | 0 |
| github-docs | 49 | hybrid (cards + raw + vectors) | 0.408 | 0.293 | 0.341 | 1584 | 0 |
| prisma | 37 | lexical (raw only) | 0.216 | 0.108 | 0.132 | 279 | 0 |
| prisma | 37 | lexical (cards + raw) | 0.297 | 0.181 | 0.21 | 723 | 0 |
| prisma | 37 | hybrid (cards + raw + vectors) | 0.297 | 0.181 | 0.221 | 1134 | 0 |
| supabase | 12 | lexical (raw only) | 0.333 | 0.118 | 0.208 | 433 | 0 |
| supabase | 12 | lexical (cards + raw) | 0.417 | 0.208 | 0.292 | 832 | 0 |
| supabase | 12 | hybrid (cards + raw + vectors) | 0.5 | 0.325 | 0.391 | 1176 | 0 |
| tailwind-css | 25 | lexical (raw only) | 0.6 | 0.34 | 0.42 | 40 | 0 |
| tailwind-css | 25 | lexical (cards + raw) | 0.84 | 0.473 | 0.596 | 114 | 0 |
| tailwind-css | 25 | hybrid (cards + raw + vectors) | 0.8 | 0.457 | 0.564 | 504 | 0 |

Generated by `scripts/eval/table.sh` from `evals/results/docsqa/<project>/results.json` (split: dev; mda 0.1.1; mean ms is the adapter in-process, release build, and includes the deeper fetches the page rule needs).

- **Cards move every project; vectors move two.** Cards + raw over raw: success@5 +0.24 on Tailwind, +0.08 on GitHub Docs and Prisma and Supabase. Adding the vector list helps GitHub Docs (0.388 → 0.408, nDCG 0.295 → 0.341) and Supabase (0.417 → 0.500, 12 questions), leaves Prisma at 0.297, and costs Tailwind 0.04 (0.840 → 0.800). Hybrid is not a free improvement over lexical-with-cards on documentation questions the way it was on the golden set; the tuning loop's candidates (plan §3: embedding text, list weights, RRF k, fetch depth) are exactly the knobs this points at, and they are tried on this split only.
- **Still far from the plan's target on the large corpora.** GitHub Docs 0.41, Prisma 0.30, Supabase 0.50 success@5 against qmd full's row that M2 will produce; the profile in the execution plan (§1.1) expected qmd's 300M embedder plus reranker to be strong here. Reported as measured.
- **Latency is now the loudest number.** The adapter's page rule fetches deeper until it holds ten distinct pages, so a question costs several searches; hybrid embeds the query on every one. 0.5–1.6 s per question in-process, release build, is far outside the 30 ms budget and is a product problem before it is a benchmark one: candidate depth, one query embedding per question, and a prepared statement per section lookup are the levers (plan §3 candidate 6 is the fetch depth). The MCP-server latency the tables report is measured separately (M2, `mcp-time.sh`).
- **Reconstruction check (M1).** `scripts/eval/preflight.sh` rebuilds a clean copy of each checkout from the committed cards and the hashed model files and requires every row and question to score identically to the store the cards came from; its reports are under `evals/results/docsqa/preflight/`. Outcome: identical on every row and question for all four projects (attach + re-embed + score: tailwind-css 117 s · supabase 612 s · prisma 975 s · github-docs 2165 s, release build, machine shared with other work), so the committed cards plus the model files rebuild these numbers without a model. The same reports carry the three activation probes per arm with their traces.

### DocsQA-Repo — competitor arms on the dev split (M2, development, 2026-09-23)

Same split, same page rule, same scorer (`mda eval --arm-output`): every arm's driver hands back ranked repository paths from the arm's own interface, the MCP tool an agent would call. qmd 2.8.3 through its MCP `query` (one index per project, collection mask widened to `.mdx` with qmd's own `--mask` option since its default indexes `**/*.md` only; full = query expansion + vectors + reranker; no-rerank = the same request with `rerank: false`; BM25 = a lex-only sub-query, which ANDs every term with no OR fallback, qmd's documented behaviour). graphify 0.9.66 through `query_graph` (every returned node's source file in tool order; token budget raised to 8,000 when fewer than ten pages came back; built by a headless Claude session running graphify's own skill on a copy of the checkout). BM25-over-files is the control: FTS5 over whole pages, mda's query form, no model. Arm records with versions, effective configurations, build times and coverage: `evals/results/docsqa/arms/`; rows, scorer output, the request sent and per-question latency: `evals/results/docsqa/<project>/arms/`. Latency for the tool-driven arms is measured separately through each MCP server (`scripts/eval/mcp-time.sh`); the qmd full and no-rerank queries take about 60 s and 10 s per question on this machine (query expansion is the cost), graphify's `query_graph` about 50 ms.

| Project | dev scored | run | success@5 | MRR@5 | nDCG@10 | mean ms | truncated |
|---|---|---|---|---|---|---|---|
| github-docs | 49 | lexical (raw only) | 0.306 | 0.175 | 0.227 | 398 | 0 |
| github-docs | 49 | lexical (cards + raw) | 0.388 | 0.249 | 0.295 | 799 | 0 |
| github-docs | 49 | hybrid (cards + raw + vectors) | 0.408 | 0.293 | 0.341 | 1114 | 0 |
| github-docs | 49 | BM25-over-files | 0.265 | 0.15 | 0.198 | n/a | 0 |
| github-docs | 49 | qmd BM25 (MCP lex-only, rerank off) | 0 | 0 | 0 | n/a | 0 |
| github-docs | 49 | qmd full (MCP query, rerank) | 0.429 | 0.259 | 0.353 | n/a | 0 |
| github-docs | 49 | qmd no-rerank (MCP query, rerank off) | 0.327 | 0.204 | 0.252 | n/a | 0 |
| prisma | 37 | lexical (raw only) | 0.216 | 0.108 | 0.132 | 253 | 0 |
| prisma | 37 | lexical (cards + raw) | 0.297 | 0.181 | 0.21 | 654 | 0 |
| prisma | 37 | hybrid (cards + raw + vectors) | 0.297 | 0.181 | 0.221 | 934 | 0 |
| prisma | 37 | BM25-over-files | 0.243 | 0.131 | 0.157 | n/a | 0 |
| prisma | 37 | qmd BM25 (MCP lex-only, rerank off) | 0.027 | 0.027 | 0.017 | n/a | 0 |
| prisma | 37 | qmd full (MCP query, rerank) | 0.297 | 0.189 | 0.229 | n/a | 0 |
| prisma | 37 | qmd no-rerank (MCP query, rerank off) | 0.243 | 0.147 | 0.2 | n/a | 0 |
| supabase | 12 | lexical (raw only) | 0.333 | 0.118 | 0.208 | 377 | 0 |
| supabase | 12 | lexical (cards + raw) | 0.417 | 0.208 | 0.292 | 777 | 0 |
| supabase | 12 | hybrid (cards + raw + vectors) | 0.5 | 0.325 | 0.391 | 930 | 0 |
| supabase | 12 | BM25-over-files | 0.333 | 0.211 | 0.208 | n/a | 0 |
| supabase | 12 | graphify (query_graph) | 0.083 | 0.042 | 0.096 | n/a | 0 |
| supabase | 12 | qmd BM25 (MCP lex-only, rerank off) | 0 | 0 | 0 | n/a | 0 |
| supabase | 12 | qmd full (MCP query, rerank) | 0.417 | 0.267 | 0.313 | n/a | 0 |
| supabase | 12 | qmd no-rerank (MCP query, rerank off) | 0.25 | 0.194 | 0.219 | n/a | 0 |
| tailwind-css | 25 | lexical (raw only) | 0.6 | 0.34 | 0.42 | 35 | 0 |
| tailwind-css | 25 | lexical (cards + raw) | 0.84 | 0.473 | 0.596 | 99 | 0 |
| tailwind-css | 25 | hybrid (cards + raw + vectors) | 0.8 | 0.457 | 0.564 | 333 | 0 |
| tailwind-css | 25 | BM25-over-files | 0.64 | 0.318 | 0.459 | n/a | 0 |
| tailwind-css | 25 | graphify (query_graph) | 0.52 | 0.288 | 0.377 | n/a | 0 |
| tailwind-css | 25 | qmd BM25 (MCP lex-only, rerank off) | 0 | 0 | 0 | n/a | 0 |
| tailwind-css | 25 | qmd full (MCP query, rerank) | 0.72 | 0.396 | 0.559 | n/a | 0 |
| tailwind-css | 25 | qmd no-rerank (MCP query, rerank off) | 0.64 | 0.347 | 0.511 | n/a | 0 |

Generated by `scripts/eval/table.sh` from `evals/results/docsqa/<project>/results.json` and `<project>/arms/*.results.json` (split: dev; mda 0.1.1; mean ms is the adapter in-process, release build, and includes the deeper fetches the page rule needs; external arms have no latency column here, their MCP latency is measured by `scripts/eval/mcp-time.sh`).

- **qmd full is the comparison that matters** (plan §1.1) and it is close: it beats mda hybrid on GitHub Docs (0.429 vs 0.408), ties Prisma (0.297) and Supabase's cards row (0.417, hybrid 0.500), and loses Tailwind (0.720 vs 0.840). Its reranker is worth +0.08 to +0.17 success@5 over its own no-rerank row; its BM25-only mode answers none of these long community questions (0.000 to 0.027) because qmd's lexical form requires every term.
- graphify's graph retrieval is weak on these questions where it is built (0.520 on Tailwind, 0.083 on Supabase); the arm exists for structural questions the tables do not measure, and its build cost is the finding (below). Prisma and GitHub Docs graphs are being built with Haiku as the extraction model after two Sonnet attempts exhausted the owner's weekly allowance; they are marked per project.
- BM25-over-files, the no-model control, sits between mda's raw section row and its carded row on three projects and below raw on GitHub Docs: sections and cards, not just "an index", are what move the numbers.
- Development numbers: tuning (plan §3) has not run; T1 comes from the test split after it.


## Not measured yet

- The A/B protocol on corpora beyond the golden set (design-partner repos; this repository's own `docs/` is a candidate at 28 files / 5K lines).
