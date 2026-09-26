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

- **qmd full is the comparison that matters** (plan §1.1) and it is close: it beats mda hybrid on GitHub Docs (0.429 vs 0.408), ties Prisma (0.297) and Supabase's cards row (0.417, hybrid 0.500), and loses Tailwind (0.720 vs 0.840). Its reranker is worth +0.08 to +0.17 success@5 over its own no-rerank row; its BM25-only mode answers none of these long community questions (0.000 to 0.027) because qmd's lexical form requires every term (raw-question lexical retrieval, not a verdict on BM25). Failed MCP calls, scored as empty lists (rule 0.3): supabase bm25: 1; prisma bm25: 1; github-docs bm25: 1.
- graphify's graph retrieval is weak on these questions where its Sonnet-built graph exists (0.520 on Tailwind, 0.083 on Supabase); the arm exists for structural questions the tables do not measure. Its Prisma and GitHub Docs Sonnet builds did not complete (one killed by the headless 600 s background-wait ceiling, two by the account's weekly usage limit on 2026-09-23; recorded, not dropped).
- **graphify-haiku** (the model-matched configuration against mda's Haiku cards: Haiku 4.5 as the host of the whole build, fresh copies, no Sonnet cache) completed on all four projects in 340–454 s at $0.97–$6.14 list-price equivalent and answers almost nothing through `query_graph`: **0.040 / 0.000 / 0.000 / 0.061** success@5 (tailwind / supabase / prisma / github-docs). The graphs differ in what they hold: tailwind 82 source files across 470 nodes, 48 of 197 corpus pages; supabase 108 source files across 1,991 nodes, 44 of 770 corpus pages; prisma 625 source files across 2,426 nodes, 585 of 685 corpus pages; github-docs 3614 source files across 45,756 nodes, 3193 of 3208 corpus pages. Whether the Haiku host ran graphify's extraction as its skill prescribes is not established from the transcripts alone (it dispatched 6–49 subagents where the skill's chunking called for 10–172, but one subagent can process several chunks); a chunk-level audit against the extraction artifacts is a follow-up, and until then the row is what it says: a cheap build of this configuration that does not retrieve. It does not replace the Sonnet-built `graphify` arm, which stays where it completed, and it does not yet give the model-matched build-cost comparison (Codex, `docs/reviews/codex/2026-09-23-bench-m2.md`). Build records with per-model usage and the failed attempts: `evals/results/docsqa/arms/graphify-*.json`; every completed graph is archived under `arms/graphs/`.

- BM25-over-files, the no-model control, sits between mda's raw section row and its carded row on three projects and below raw on GitHub Docs: sections and cards, not just "an index", are what move the numbers.
- Development numbers: tuning (plan §3) has not run; T1 comes from the test split after it.

### DocsQA-Repo — the tuning loop (M3, development, 2026-09-23)

The greedy loop of the execution plan §3 ran on the dev split with `scripts/eval/tune.sh` (ledger `evals/results/docsqa/TUNING.md`, every trial archived under `evals/results/docsqa/tuning/`). Objective: mean success@5 of the hybrid row over the four projects; guardrail: no project more than 0.02 below the reference.

| trial | change | tailwind | supabase | prisma | github-docs | objective | decision |
|---|---|---|---|---|---|---|---|
| baseline | `pre-tuning defaults` | 0.800 | 0.500 | 0.297 | 0.408 | 0.5014 | reference |
| c1-questions-first | `embedding_text=questions-first` | 0.720 | 0.500 | 0.297 | 0.449 | 0.4916 | discard |
| c2-with-entities | `embedding_text=with-entities` | 0.760 | 0.417 | 0.324 | 0.429 | 0.4824 | discard |

Both embedding-text candidates moved projects in opposite directions (questions-first: GitHub Docs +0.04, Tailwind −0.08; entities appended: Prisma +0.03, Supabase −0.08) and lost on the mean, so the stop rule ("two consecutive candidates fail to improve by 0.01") ended the loop before the five search-time candidates; **the pre-tuning defaults are the winner** and remain the product configuration. Whether the unrun candidates get an explicitly labelled exploratory extension is an owner decision (it changes the pre-declared protocol); nothing from such a run could become the greedy winner.

### DocsQA-Repo — post-stop exploration of candidates 3–7 (information only, 2026-09-24)

After the stop rule fired, the five search-time candidates were run once each against the unchanged pre-tuning configuration, information only (execution plan §3, 2026-09-24 amendment, decided with Codex: `docs/reviews/codex/2026-09-24-post-stop-c3-c7.md`). Nothing here is adopted for this release, whatever the screen says; the product default stays the pre-tuning configuration and the final freeze records "Selection: original §3 winner; post-stop diagnostics excluded from selection". Table by `scripts/eval/tune.sh explore-table` from the archived trials (`evals/results/docsqa/tuning/post-stop-*/`); the interval is a descriptive 95% within-project paired bootstrap of the objective difference (5,000 draws, seed 20260922), not a significance test across five trials.

| trial | change | tailwind | supabase | prisma | github-docs | objective | Δ objective, 95% paired | wins/losses | decision |
|---|---|---|---|---|---|---|---|---|---|
| post-stop-c3 | `search_questions_weight=3` | 0.8 | 0.583 | 0.297 | 0.408 | 0.5222 | 0.0208 [0, 0.0625] | 1/0 | screen-pass-not-adopted |
| post-stop-c4 | `search_raw_weight=0.7` | 0.76 | 0.583 | 0.297 | 0.449 | 0.5224 | 0.021 [-0.0198, 0.0727] | 3/1 | screen-fail-not-adopted |
| post-stop-c5 | `search_rrf_k=30` | 0.8 | 0.583 | 0.297 | 0.449 | 0.5324 | 0.031 [0, 0.0778] | 3/0 | screen-pass-not-adopted |
| post-stop-c6 | `fetch=60` | 0.8 | 0.5 | 0.297 | 0.408 | 0.5014 | 0 [0, 0] | 0/0 | screen-fail-not-adopted |
| post-stop-c7 | `search_and_stopwords=true` | 0.8 | 0.5 | 0.297 | 0.408 | 0.5014 | 0 [0, 0] | 0/0 | screen-fail-not-adopted |

Reading: two candidates clear the engineering screen (≥ 0.01 with no project losing more than 0.02), RRF k 30 with three wins and no loss (two GitHub Docs questions and one Supabase question move into the top five) and the `questions_answered` weight 3 with one Supabase win. Every interval includes zero, and the whole screen is worth one to three questions on dev splits of 12–49, so these are hypotheses for a separately declared evaluation, not a result. The raw-list weight 0.7 gains the same three questions but loses one on Tailwind (guardrail). Fetch depth 60 and the stop-word AND form leave every dev question's hybrid success@5 unchanged; the ranked lists do move below the cutoff (fetch 60 lifts one GitHub Docs question from rank 14 to 9, the stop-word form brings another from unranked to 20), so this is "no change at the metric", not "no change in retrieval". The Δ column is formatted to four decimals from unrounded values.

### DocsQA-Repo — pooled labels, the diagnostic second column (M3, development, 2026-09-23)

**Correction (2026-09-24, Codex M4 F1).** The sampler that drew these 100 pairs per project did not exclude already-labelled pages (a jq binding error, fixed in `scripts/eval/pool.sh` before the T1 samples were drawn): 5 / 2 / 4 / 4 of the 100 pairs on GitHub Docs / Prisma / Supabase / Tailwind carried an original label. The pooled column is unaffected (an existing label is never added twice), but the "relevant among the sampled pairs" counts below include those pairs, and the judge scored 7 of the 15 labelled pairs 0 — a reading on the dataset's own labels, not on the arms. The T1 pooled column (§ below) uses the fixed sampler.

Axis A's second column (plan §2.3): from every arm's top-5 pages, 100 query-page pairs per project that carry no original label were sampled (seeded, round-robin over the eight arms; `scripts/eval/pool.sh sample`) and judged blind by one panel member so far, Claude Fable 5.1 through `claude -p --model claude-fable-5-1` with the frozen 0/1/2 rubric (`pool.sh judge`; the Astra half of the panel joins at M5); a pair is relevant when the mean judgment is ≥ 1. Judgments 0/1/2 per project: tailwind-css 93/5/2 · supabase 86/11/3 · prisma 82/17/1 · github-docs 84/13/3 — the sparse labels miss about one in seven of the pages the arms return. Every arm was then rescored over the judged questions only, with labels = original ∪ pooled (`pool.sh column`, the same scorer with `--extra-labels` and `--only-questions`); success@5:

| Project | judged questions | labels added | mda hybrid original → pooled | qmd full original → pooled | BM25-over-files | graphify (Sonnet) |
|---|---|---|---|---|---|---|
| tailwind-css | 25 | 5 | 0.800 → 0.840 | 0.720 → 0.760 | 0.640 → 0.680 | 0.520 → 0.600 |
| supabase | 12 | 12 | 0.500 → 0.667 | 0.417 → 0.667 | 0.333 → 0.417 | 0.083 → 0.167 |
| prisma | 34 | 18 | 0.294 → 0.500 | 0.324 → 0.529 | 0.265 → 0.412 | n/a |
| github-docs | 45 | 12 | 0.422 → 0.444 | 0.422 → 0.556 | 0.289 → 0.311 | n/a |

- The pooled labels favour qmd full more than mda on the two large corpora: on GitHub Docs the two arms tie on the original labels over the judged questions (0.422) and qmd leads by 0.11 with the pooled ones; on Prisma qmd leads by 0.03 either way; Supabase ties at 0.667; Tailwind keeps mda ahead. qmd's reranker surfaces relevant pages the dataset did not label more often than our fusion does; the tuning loop's objective is the original labels, so this is the reading to keep in mind when the test-split column is judged at M4 by both panel members.
- Development numbers over small judged sets (12 to 45 questions); the published column comes from the final test-split runs.


## T1 — axis A on DocsQA-Repo, test split (final, 2026-09-24)

The first published table. Test split, scored once per arm under the final freeze `evals/results/docsqa/T1/FROZEN.md` (protocol final; the selection is the pre-tuning configuration, the winner of the §3 loop; the post-stop diagnostics are excluded from selection). Every row regenerates from its archived page lists without a store (the preflight's `regenerate` check), every interval is a 95% bootstrap over the row's questions (5,000 draws, seed 20260922, `mda eval --interval`), and the runbook's §7.1 reproduces every command. Arms, page rule, denominators and labels are those of §2.1–2.3 of the execution plan; a missing query is a miss counted in every denominator. Reproduction recipe and disclosures: `evals/benchmark_it_with_claude.md` §7.1.

### Quality

| Project | test scored | run | success@5 [95%] | MRR@5 [95%] | nDCG@10 [95%] | truncated |
|---|---|---|---|---|---|---|
| github-docs | 87 | lexical (raw only) | 0.345 [0.241, 0.448] | 0.164 [0.106, 0.227] | 0.195 [0.136, 0.257] | 0 |
| github-docs | 87 | lexical (cards + raw) | 0.356 [0.264, 0.46] | 0.188 [0.125, 0.259] | 0.255 [0.195, 0.322] | 0 |
| github-docs | 87 | hybrid (cards + raw + vectors) | 0.483 [0.379, 0.586] | 0.255 [0.188, 0.327] | 0.331 [0.266, 0.397] | 0 |
| github-docs | 87 | BM25-over-files | 0.299 [0.207, 0.402] | 0.189 [0.119, 0.261] | 0.202 [0.141, 0.267] | 0 |
| github-docs | 87 | graphify-haiku (query_graph) | 0.034 [0, 0.08] | 0.011 [-0, 0.024] | 0.019 [0.005, 0.038] | 0 |
| github-docs | 87 | qmd BM25 (MCP lex-only, rerank off) | 0.011 [0, 0.034] | 0.011 [-0, 0.034] | 0.011 [-0, 0.034] | 0 |
| github-docs | 87 | qmd full (MCP query, rerank) | 0.46 [0.356, 0.563] | 0.261 [0.191, 0.337] | 0.299 [0.233, 0.369] | 0 |
| github-docs | 87 | qmd no-rerank (MCP query, rerank off) | 0.368 [0.264, 0.471] | 0.23 [0.157, 0.307] | 0.249 [0.184, 0.318] | 0 |
| prisma | 63 | lexical (raw only) | 0.286 [0.175, 0.397] | 0.166 [0.094, 0.244] | 0.207 [0.131, 0.287] | 0 |
| prisma | 63 | lexical (cards + raw) | 0.365 [0.254, 0.492] | 0.185 [0.113, 0.263] | 0.246 [0.172, 0.322] | 0 |
| prisma | 63 | hybrid (cards + raw + vectors) | 0.413 [0.302, 0.54] | 0.254 [0.168, 0.348] | 0.289 [0.212, 0.371] | 0 |
| prisma | 63 | BM25-over-files | 0.302 [0.19, 0.413] | 0.139 [0.078, 0.207] | 0.182 [0.119, 0.249] | 0 |
| prisma | 63 | graphify-haiku (query_graph) | 0.032 [0, 0.079] | 0.024 [-0, 0.063] | 0.016 [-0, 0.041] | 0 |
| prisma | 63 | qmd BM25 (MCP lex-only, rerank off) | 0.016 [0, 0.048] | 0.016 [-0, 0.048] | 0.016 [-0, 0.048] | 0 |
| prisma | 63 | qmd full (MCP query, rerank) | 0.365 [0.254, 0.492] | 0.202 [0.129, 0.285] | 0.241 [0.17, 0.319] | 0 |
| prisma | 63 | qmd no-rerank (MCP query, rerank off) | 0.27 [0.159, 0.381] | 0.124 [0.068, 0.186] | 0.179 [0.12, 0.242] | 0 |
| supabase | 21 | lexical (raw only) | 0.333 [0.143, 0.524] | 0.218 [0.075, 0.381] | 0.277 [0.137, 0.434] | 0 |
| supabase | 21 | lexical (cards + raw) | 0.381 [0.19, 0.619] | 0.302 [0.135, 0.5] | 0.353 [0.187, 0.531] | 0 |
| supabase | 21 | hybrid (cards + raw + vectors) | 0.429 [0.238, 0.667] | 0.345 [0.167, 0.548] | 0.388 [0.215, 0.566] | 0 |
| supabase | 21 | BM25-over-files | 0.429 [0.238, 0.619] | 0.287 [0.129, 0.454] | 0.342 [0.191, 0.499] | 0 |
| supabase | 21 | graphify-haiku (query_graph) | 0 [0, 0] | -0 [-0, -0] | -0 [-0, -0] | 3 |
| supabase | 21 | graphify (query_graph) | 0.095 [0, 0.238] | 0.063 [-0, 0.175] | 0.071 [-0, 0.19] | 0 |
| supabase | 21 | qmd BM25 (MCP lex-only, rerank off) | 0 [0, 0] | -0 [-0, -0] | -0 [-0, -0] | 0 |
| supabase | 21 | qmd full (MCP query, rerank) | 0.381 [0.19, 0.571] | 0.172 [0.069, 0.297] | 0.252 [0.127, 0.385] | 0 |
| supabase | 21 | qmd no-rerank (MCP query, rerank off) | 0.19 [0.048, 0.381] | 0.103 [0.016, 0.222] | 0.135 [0.03, 0.267] | 0 |
| tailwind-css | 47 | lexical (raw only) | 0.553 [0.404, 0.702] | 0.344 [0.236, 0.459] | 0.469 [0.376, 0.57] | 0 |
| tailwind-css | 47 | lexical (cards + raw) | 0.681 [0.553, 0.809] | 0.426 [0.315, 0.539] | 0.535 [0.437, 0.63] | 0 |
| tailwind-css | 47 | hybrid (cards + raw + vectors) | 0.702 [0.574, 0.83] | 0.452 [0.336, 0.57] | 0.564 [0.466, 0.661] | 0 |
| tailwind-css | 47 | BM25-over-files | 0.766 [0.638, 0.872] | 0.444 [0.339, 0.548] | 0.572 [0.49, 0.651] | 0 |
| tailwind-css | 47 | graphify-haiku (query_graph) | 0.021 [0, 0.064] | 0.011 [-0, 0.032] | 0.008 [-0, 0.025] | 0 |
| tailwind-css | 47 | graphify (query_graph) | 0.383 [0.255, 0.511] | 0.206 [0.117, 0.301] | 0.283 [0.189, 0.377] | 0 |
| tailwind-css | 47 | qmd BM25 (MCP lex-only, rerank off) | 0 [0, 0] | -0 [-0, -0] | -0 [-0, -0] | 0 |
| tailwind-css | 47 | qmd full (MCP query, rerank) | 0.681 [0.553, 0.809] | 0.367 [0.27, 0.469] | 0.52 [0.439, 0.601] | 0 |
| tailwind-css | 47 | qmd no-rerank (MCP query, rerank off) | 0.617 [0.468, 0.745] | 0.33 [0.237, 0.433] | 0.491 [0.413, 0.573] | 0 |

A `-0` is a zero (the nearest-rank percentile of an all-zero row). Generated by `scripts/eval/t1.sh table` from `evals/results/docsqa/T1/<project>/results.json` and `T1/<project>/arms/*.results.json` (split: test, scored once; mda 0.1.1; intervals: 95% bootstrap over the row's questions, 5,000 draws, seed 20260922, `mda eval --interval`).

### The product target: match qmd full

| Project | n | qmd full success@5 | mda hybrid success@5 | Δ (hybrid − qmd full), 95% paired | wins/losses | target (match qmd full) |
|---|---|---|---|---|---|---|
| github-docs | 87 | 0.46 | 0.483 | 0.023 [-0.092, 0.1379] | 13/11 | met (point estimate ≥); interval includes 0 |
| prisma | 63 | 0.365 | 0.413 | 0.0476 [-0.0952, 0.1746] | 11/8 | met (point estimate ≥); interval includes 0 |
| supabase | 21 | 0.381 | 0.429 | 0.0476 [-0.0952, 0.1905] | 2/1 | met (point estimate ≥); interval includes 0 |
| tailwind-css | 47 | 0.681 | 0.702 | 0.0213 [-0.1064, 0.1489] | 5/4 | met (point estimate ≥); interval includes 0 |

Generated by `scripts/eval/t1.sh target` (`mda eval --compare`: within-project paired bootstrap, 5,000 draws, seed 20260922; the product target of plan §4 is reported as met or not per project on the point estimate, with the interval beside it).

Reading: on every project the hybrid row's point estimate is at or above qmd full's, by 0.02 to 0.05, with 13/11, 11/8, 2/1 and 5/4 paired wins/losses; every interval includes zero, so the honest statement is "not behind qmd full on any project, not distinguishable from it on these sample sizes". The gap between qmd full and qmd without its reranker (0.09 to 0.19) says where qmd's quality comes from, and the latency table says what it costs.

### Where we lose

| Project | n | hybrid misses | … found by qmd full | … by BM25-over-files | … by graphify | … by none of the three | hybrid rank 6–10 on its misses | rank > 10 / absent | only hybrid gets |
|---|---|---|---|---|---|---|---|---|---|
| github-docs | 87 | 45 | 11 | 5 | n/a | 33 | 14 | 31 | 7 |
| prisma | 63 | 37 | 8 | 5 | n/a | 26 | 7 | 30 | 6 |
| supabase | 21 | 12 | 1 | 3 | 2 | 7 | 3 | 9 | 1 |
| tailwind-css | 47 | 14 | 4 | 6 | 0 | 5 | 8 | 6 | 1 |

Generated by `scripts/eval/t1.sh lose` from the archived rows (success@5 misses of the hybrid row; "found by" = that arm's first relevant page within its top five on the same question).

Two patterns. **Against qmd full**: of the hybrid misses qmd recovers, most are pages the hybrid ranked sixth to tenth; qmd's reranker reads the candidate text and reorders, which we do not do. **Against BM25-over-files on Tailwind**: a whole-page index scores 0.766 against our 0.702 on the same 47 questions, six questions to three, interval including zero; five of the six are two long hub guides (`adding-custom-styles.mdx`, `detecting-classes-in-source-files.mdx`) that a long community question matches everywhere at once, which a page-level index sums and our section-level index does not. The control trails us by 0.11 on Prisma and 0.18 on GitHub Docs, the corpora with thousands of pages. Aggregating section scores per page is a hypothesis for a future, separately declared tuning round; it is not tried on this table. The larger number in every project is "found by none of the three": 33 / 26 / 7 / 5 misses where no arm has the labelled page in its top five, the dataset's hard tail.

### Latency through each arm's MCP server

| Project | arm | queries | cold first call (startup + call) | warm median ms | warm p90 ms | failed |
|---|---|---|---|---|---|---|
| github-docs | graphify-haiku | 108 | 4961 + 1804 ms | 2006 | 4957 | 0 |
| github-docs | mda | 108 | 12 + 1322 ms | 741 | 2436 | 0 |
| github-docs | qmd | 108 | 403 + 25730 ms | 42374 | 63714 | 0 |
| prisma | graphify-haiku | 68 | 794 + 105 ms | 122 | 288 | 0 |
| prisma | mda | 68 | 11 + 904 ms | 543 | 2142 | 0 |
| prisma | qmd | 68 | 295 + 34263 ms | 47413 | 74867 | 0 |
| supabase | graphify-haiku | 28 | 645 + 56 ms | 85 | 245 | 0 |
| supabase | graphify | 28 | 706 + 125 ms | 164 | 320 | 0 |
| supabase | mda | 28 | 11 + 787 ms | 524 | 1143 | 0 |
| supabase | qmd | 28 | 308 + 27642 ms | 636 | 1868 | 0 |
| tailwind-css | graphify-haiku | 51 | 568 + 33 ms | 27 | 62 | 0 |
| tailwind-css | graphify | 51 | 744 + 36 ms | 35 | 77 | 0 |
| tailwind-css | mda | 51 | 10 + 293 ms | 173 | 461 | 0 |
| tailwind-css | qmd | 51 | 302 + 46727 ms | 44007 | 71825 | 0 |

Generated by `scripts/eval/t1.sh latency-table` from `T1/<project>/latency/<arm>.jsonl` (`scripts/eval/mcp-time.sh`: one rmcp stdio client, the server started cold, the first query includes process start and model load; plan §2.7).

Reading: through its MCP server, mda answers in 0.2–0.7 s warm median (0.3–1.3 s on a cold process, the model loaded on first use), graphify in 30–160 ms on the graphs it has (2 s on the GitHub Docs Haiku graph), and qmd in 42–47 s median on Tailwind, Prisma and GitHub Docs: its query expansion and reranker run a local LLM on every call. **Supabase's qmd row is a cache artefact, not a speed-up**: qmd keeps an on-disk LLM cache (`llm_cache`, excluded from the index fingerprint), the latency run re-asked questions the scoring run had asked hours earlier, and on Supabase every warm call was served in 0.4–2.8 s (median 636 ms) against 44–135 s for the same questions during the scoring run; on the other three projects the cache did not shorten the calls. The published comparison number for qmd is therefore its cold call and the three uncached medians. "Cold" means a new server process; no arm's model or query cache was cleared between the scoring run and this measurement, for any arm.

### Pooled labels, the second column

| Project | judged questions | pairs relevant by pool | agreement (exact / within one) | run | original success@5 [95%] | pooled success@5 [95%] |
|---|---|---|---|---|---|---|
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | lexical (raw only) | 0.568 [0.432, 0.727] | 0.636 [0.5, 0.773] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | lexical (cards + raw) | 0.705 [0.568, 0.841] | 0.727 [0.591, 0.841] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | hybrid (cards + raw + vectors) | 0.705 [0.568, 0.841] | 0.75 [0.614, 0.864] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | BM25-over-files | 0.773 [0.636, 0.886] | 0.773 [0.636, 0.886] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | graphify-haiku (query_graph) | 0.023 [0, 0.068] | 0.045 [0, 0.114] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | graphify (query_graph) | 0.386 [0.25, 0.523] | 0.432 [0.295, 0.591] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | qmd BM25 (MCP lex-only, rerank off) | 0 [0, 0] | 0 [0, 0] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | qmd full (MCP query, rerank) | 0.659 [0.523, 0.795] | 0.705 [0.568, 0.841] |
| tailwind-css | 44 | 7 of 100 | 0.96 / 1 | qmd no-rerank (MCP query, rerank off) | 0.614 [0.455, 0.75] | 0.659 [0.523, 0.795] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | lexical (raw only) | 0.333 [0.143, 0.524] | 0.429 [0.238, 0.667] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | lexical (cards + raw) | 0.381 [0.19, 0.619] | 0.571 [0.381, 0.762] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | hybrid (cards + raw + vectors) | 0.429 [0.238, 0.667] | 0.571 [0.333, 0.81] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | BM25-over-files | 0.429 [0.238, 0.619] | 0.429 [0.238, 0.619] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | graphify-haiku (query_graph) | 0 [0, 0] | 0 [0, 0] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | graphify (query_graph) | 0.095 [0, 0.238] | 0.143 [0, 0.286] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | qmd BM25 (MCP lex-only, rerank off) | 0 [0, 0] | 0 [0, 0] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | qmd full (MCP query, rerank) | 0.381 [0.19, 0.571] | 0.571 [0.333, 0.762] |
| supabase | 21 | 10 of 100 | 0.89 / 1 | qmd no-rerank (MCP query, rerank off) | 0.19 [0.048, 0.381] | 0.333 [0.143, 0.524] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | lexical (raw only) | 0.264 [0.151, 0.396] | 0.321 [0.208, 0.453] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | lexical (cards + raw) | 0.358 [0.226, 0.491] | 0.434 [0.302, 0.566] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | hybrid (cards + raw + vectors) | 0.434 [0.302, 0.566] | 0.509 [0.377, 0.642] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | BM25-over-files | 0.283 [0.17, 0.415] | 0.358 [0.226, 0.491] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | graphify-haiku (query_graph) | 0.019 [0, 0.057] | 0.075 [0.019, 0.151] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | qmd BM25 (MCP lex-only, rerank off) | 0.019 [0, 0.057] | 0.019 [0, 0.057] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | qmd full (MCP query, rerank) | 0.377 [0.245, 0.509] | 0.453 [0.321, 0.585] |
| prisma | 53 | 18 of 100 | 0.93 / 1 | qmd no-rerank (MCP query, rerank off) | 0.283 [0.17, 0.415] | 0.34 [0.208, 0.472] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | lexical (raw only) | 0.305 [0.186, 0.424] | 0.39 [0.271, 0.525] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | lexical (cards + raw) | 0.305 [0.186, 0.424] | 0.39 [0.271, 0.508] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | hybrid (cards + raw + vectors) | 0.441 [0.322, 0.576] | 0.559 [0.424, 0.695] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | BM25-over-files | 0.271 [0.153, 0.39] | 0.407 [0.288, 0.525] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | graphify-haiku (query_graph) | 0.051 [0, 0.119] | 0.051 [0, 0.119] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | qmd BM25 (MCP lex-only, rerank off) | 0.017 [0, 0.051] | 0.051 [0, 0.119] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | qmd full (MCP query, rerank) | 0.458 [0.339, 0.593] | 0.593 [0.475, 0.712] |
| github-docs | 59 | 21 of 100 | 0.93 / 1 | qmd no-rerank (MCP query, rerank off) | 0.356 [0.237, 0.475] | 0.492 [0.356, 0.61] |

Generated from `evals/results/docsqa/T1/<project>/pool/test-column.json` (`TABLE=T1 scripts/eval/pool.sh column <project> test`; the jq rendering command is in runbook §7.1). Labels = original ∪ pooled-relevant; a pair is relevant when both judges scored it and the mean is ≥ 1; judged questions = those with at least one complete pair; intervals: 95% bootstrap over the judged questions, 5,000 draws, seed 20260922.

Reading: 100 unlabelled top-five pairs per project, judged by Fable (`claude -p`) and Astra (`codex exec`) blind to the arm and to each other; exact agreement 0.89–0.96 and every disagreement within one point; 7 / 10 / 18 / 21 pairs relevant by pool. The pooled labels lift every retrieval arm by a similar amount (hybrid +0.05 to +0.14, qmd full +0.05 to +0.19); on the judged questions hybrid stays ahead on Tailwind and Prisma, ties qmd full on Supabase and trails it on GitHub Docs (0.559 vs 0.593, 59 questions). The second column is model-assisted and labelled so; it does not replace the first, and the judges' individual scores and rationales are committed (`test-judgments-<judge>.jsonl`).

### What travels with the table

T1 reuses the M2 artifacts (the committed cards, the qmd indexes, the BM25-over-files tables, the archived graphify graphs); it is not a fresh-build comparison, and every artifact's hash is in `T1/FROZEN.md`. graphify's Sonnet-built graphs exist for Tailwind and Supabase only (its builds did not complete on Prisma and GitHub Docs, recorded); its Haiku-built graphs exist for all four. The freeze was re-written during the run for driver and script fixes with identical inputs; each arm's manifest names the commit its rows were produced under, and the chronology is the git history of `T1/FROZEN.md`. The qmd and graphify drivers collapse whitespace in the question; mda receives it as written. graphify nodes without a source file are dropped (they name no page). The CPU and GPU chains ran concurrently, so the drivers' own timings reflect contention; the published latency was measured afterwards, alone, with the judging chain (network-bound) running beside it; "cold" means a new server process, not cleared model or query caches. The preflights (`evals/results/docsqa/preflight/T1-<project>.json`) passed every check on Tailwind, Prisma and GitHub Docs; on Supabase every check passed except one activation probe for the graphify-haiku arm, in two attempts (question `supabase-13977`: the headless agent read files instead of calling the graph tool; the arm's rows come from the tool directly and are unaffected; both attempts are committed). Codex's M4 pass on the scorer, the drivers and the freeze (`docs/reviews/codex/2026-09-24-bench-m4.md`) preceded every number on this page.

## T2 harness pilot — answer quality on DocsQA-Repo (M5, development, 2026-09-25; not a result)

The harness of execution plan §2.4–2.7 (runner, grader with grounding, analysis, two-model panel, card audit) exercised end to end on the dev split: 5 questions per project, one run per arm, the arms with a build (graphify's graphs exist on Tailwind and Supabase). Nothing here is a result: five questions per project cannot separate the arms, and the pilot's purpose is throughput, failure rate and the harness's own behaviour before M6 freezes T2 (25 test questions × 3 runs per arm and project). Every model call went through the owner's logins; the Codex M5 pass on the harness (`docs/reviews/codex/2026-09-25-bench-m5.md`, 11 findings fixed) preceded these numbers. Reproduction: runbook §5c.

### Throughput and consumption (rule 0.7 tokens)

| arm | runs | errors | median wall s | median source tokens (clipped) | runs with a clamped turn | median input tokens | median tool calls | median cost $ | median turns |
|---|---|---|---|---|---|---|---|---|---|
| graphify | 10 | 0 | 16.5 | 6828.5 | 0 | 106950.5 | 5 | 0.073 | 6 |
| grep | 20 | 0 | 17.5 | 4443.5 | 0 | 78739 | 6 | 0.057 | 7 |
| mda | 20 | 0 | 12 | 2212 | 0 | 59500 | 3 | 0.035 | 4 |
| qmd | 20 | 0 | 44.5 | 3711 | 0 | 70029.5 | 2 | 0.046 | 3 |

Pilot, development: 5 dev questions × 4 projects × the arms with a build (graphify on Tailwind and Supabase) × 1 run = 70 runs, 0 errors, $3.93 list-price total, 1892 s of wall-clock at one job (132 runs per hour); Sonnet answers; every arm launched as the probes launch it; every MCP server started cold per run.

Reading: one job sustains 132 answer runs per hour; no run failed or timed out (600 s limit, one retry allowed, none needed). mda's median run reads the fewest source tokens (2,212 against 4,444 grep, 3,711 qmd, 6,829 graphify), makes the fewest tool calls and costs the least; qmd's run is the slowest (its MCP query runs a local reranker per call). No turn had a negative source-token delta, so the clipped and signed sums coincide on this pilot.

### Grades, grounding and the gates (5 questions per project: descriptive only)

**tailwind-css** (5 dev questions, 1 run)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| graphify | 5 | 4.80 | 4.8 (5) | 5 | 5 | 0 | 40.0% | 8043.0 | 7.0 | \$0.086 |
| grep | 5 | 5.00 | 5.0 (5) | 5 | 5 | 0 | 20.0% | 3450.0 | 6.0 | \$0.053 |
| mda | 5 | 5.20 | 5.2 (5) | 5 | 5 | 0 | 40.0% | 2119.0 | 3.0 | \$0.030 |
| qmd | 5 | 5.20 | 5.2 (5) | 5 | 5 | 0 | 40.0% | 3998.0 | 2.0 | \$0.047 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 5 | +0.20 | [-0.60, +1.20] | 1/1/3 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 5 | +0.00 | [-1.20, +1.20] | 1/1/3 | FAIL | pass | FAIL | none claimed |
| mda vs graphify | 5 | +0.40 | [-0.80, +1.60] | 2/1/2 | FAIL | pass | FAIL | none claimed |

**supabase** (5 dev questions, 1 run)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| graphify | 5 | 3.80 | 3.8 (5) | 5 | 5 | 0 | 40.0% | 6571.0 | 4.0 | \$0.058 |
| grep | 5 | 4.80 | 4.8 (5) | 5 | 5 | 0 | 40.0% | 5198.0 | 3.0 | \$0.052 |
| mda | 5 | 3.20 | 3.2 (5) | 5 | 5 | 0 | 100.0% | 2305.0 | 3.0 | \$0.033 |
| qmd | 5 | 3.60 | 3.6 (5) | 5 | 5 | 0 | 40.0% | 3707.0 | 2.0 | \$0.049 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 5 | -1.60 | [-4.00, +0.00] | 0/2/3 | FAIL | FAIL | pass | none claimed |
| mda vs qmd | 5 | -0.40 | [-1.60, +0.80] | 1/2/2 | FAIL | FAIL | pass | none claimed |
| mda vs graphify | 5 | -0.60 | [-1.40, +0.00] | 0/2/3 | FAIL | FAIL | pass | none claimed |

**prisma** (5 dev questions, 1 run)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| grep | 5 | 4.60 | 4.6 (5) | 5 | 5 | 0 | 20.0% | 9889.0 | 6.0 | \$0.087 |
| mda | 5 | 3.60 | 3.6 (5) | 5 | 5 | 0 | 20.0% | 2990.0 | 5.0 | \$0.045 |
| qmd | 5 | 4.00 | 4.0 (5) | 5 | 5 | 0 | 20.0% | 2976.0 | 2.0 | \$0.040 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 5 | -1.00 | [-4.20, +2.20] | 2/3/0 | FAIL | FAIL | FAIL | none claimed |
| mda vs qmd | 5 | -0.40 | [-3.60, +2.80] | 2/2/1 | FAIL | FAIL | FAIL | none claimed |

**github-docs** (5 dev questions, 1 run)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| grep | 5 | 3.60 | 3.6 (5) | 5 | 5 | 0 | 0.0% | 3558.0 | 7.0 | \$0.058 |
| mda | 5 | 3.80 | 3.8 (5) | 5 | 5 | 0 | 60.0% | 1918.0 | 3.0 | \$0.031 |
| qmd | 5 | 4.00 | 4.0 (5) | 5 | 5 | 0 | 20.0% | 4882.0 | 3.0 | \$0.083 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 5 | +0.20 | [-0.60, +1.20] | 1/1/3 | FAIL | FAIL | FAIL | none claimed |
| mda vs qmd | 5 | -0.20 | [-1.20, +0.60] | 1/1/3 | FAIL | FAIL | FAIL | none claimed |

Reading: with five questions the paired intervals are as wide as the scale, and no pair passes the three gates — gate (c), grounding ≥ 95%, fails for every arm on every project. That is the harness's finding to act on before M6, not a comparison: the grounding grader (every claim supported by a cited page, whole pages checked) treats an answer's own causal explanation or a paraphrased warning as unsupported, and the first grading pass showed that qmd's answers cite paths with a collection prefix or a bare file name, which the resolver now handles by one declared rule (exact path, or the unique suffix after `<project>/`). The regraded numbers above use that rule. Whether the rubric's strictness is the intended reading of rule 0.8 ("every claim supported") or should be narrowed to factual claims about the product is a pre-freeze decision recorded in STATUS.

### Panel calibration (8 answers per project, Fable and Astra, blind)

| project | answers | incomplete | Fable/Astra exact | within one | Fable/Sonnet exact | Astra/Sonnet exact | trigger fired | mean Sonnet / Fable / Astra |
|---|---|---|---|---|---|---|---|---|
| tailwind-css | 8 | 0 | 0.5 | 1 | 0.88 | 0.63 | 0 | 5.88 / 5.75 / 5.5 |
| supabase | 8 | 0 | 0.38 | 0.63 | 0.38 | 0.5 | 4 | 3.75 / 3.5 / 2.38 |
| prisma | 8 | 0 | 0.5 | 0.63 | 0.5 | 0.5 | 3 | 4.13 / 4.38 / 3.5 |
| github-docs | 8 | 0 | 0.38 | 0.75 | 0.13 | 0.13 | 3 | 4.38 / 4.13 / 2.75 |

Generated from `<out>/panel/regrade.json` (`scripts/eval/panel.sh regrade <out> 8`; archived under `evals/results/docsqa/t2-pilot/<project>/`). Trigger: either panel member differs from the Sonnet grade by more than one point → the panel mean.

Reading: the declared trigger (either member differs from the Sonnet grade by more than one point → the panel mean) fired on 10 of 32 sampled answers; Astra grades lower than Sonnet and Fable throughout; exact agreement between the two panel members is 0.38–0.50 and within one point 0.63–1.0. Individual scores and rationales are in `panel/regrade-<member>.jsonl`; the adjudicated grades (`panel/grades.adjudicated.jsonl`) were analysed beside the originals (`t2.sh analysis --adjudicated`); they move the paired means by up to 0.7 points and change no gate verdict on this pilot.

### Card audit (Prisma, 20 cards, 90 values, both members)

| project | cards | values | unsupported (Fable) | unsupported (Astra) | per-value agreement | incomplete verdicts | sections missing from the store |
|---|---|---|---|---|---|---|---|
| prisma | 20 | 90 | 0.024 | 0.024 | 0.976 | 1 | 0 |

Generated from `evals/results/docsqa/prisma/panel/cards.json` (`scripts/eval/panel.sh cards prisma 20`; each date judged as "raw → iso (precision)", each entity against the section text; a verdict that does not cover every value once is invalid).

Reading: 2.4% of the sampled dates and entities were judged unsupported by each member, with 98% per-value agreement; the M6 audit is 100 cards per corpus.

## T2 — axis B on DocsQA-Repo, answer quality (final, 2026-09-25)

The second published table. Test split, 25 questions per project (21 on Supabase, every eligible test question with a reference), 3 runs per question and arm, under the final freeze `evals/results/docsqa/T2/FROZEN.md` (protocol final; the grounding rubric as amended before the freeze, plan §2.4; every `claude -p` run starts its arm's MCP server cold, rule 0.9 as amended). Arms: grep (Read/Grep/Glob), mda (MCP + search-first rules), qmd full (MCP + qmd's skill), graphify (MCP from its checkout copy, on the two corpora where its Sonnet-built graph exists); every arm launched exactly as its activation probes were (`preflight/T2-<project>.json`). Sonnet answers and grades; the two-model panel re-graded 30 answers per project and audited 100 cards per corpus. The analysis of rule 0.4: question-level medians with failed runs as 0, a 10,000-draw paired bootstrap per comparator pair, the three gates, and savings only where they pass. Reproduction: runbook §7.2.

### Runs and consumption (rule 0.7 tokens)

| Project | arm | runs | errors | median wall s | median source tokens (clipped) | runs with a clamped turn | median signed source tokens | median input tokens | median tool calls | median cost $ | median turns |
|---|---|---|---|---|---|---|---|---|---|---|---|
| tailwind-css | graphify | 75 | 0 | 25 | 7343 | 0 | 7343 | 131524 | 7 | 0.076 | 8 |
| tailwind-css | grep | 75 | 0 | 17 | 2486 | 0 | 2486 | 63344 | 5 | 0.04 | 6 |
| tailwind-css | mda | 75 | 0 | 14 | 2612 | 0 | 2612 | 53577 | 4 | 0.032 | 5 |
| tailwind-css | qmd | 75 | 0 | 46 | 3487 | 0 | 3487 | 92630 | 3 | 0.045 | 4 |
| supabase | graphify | 63 | 0 | 17 | 7687 | 0 | 7687 | 82552 | 4 | 0.065 | 5 |
| supabase | grep | 63 | 0 | 14 | 3873 | 0 | 3873 | 50003 | 3 | 0.039 | 4 |
| supabase | mda | 63 | 0 | 13 | 2648 | 0 | 2648 | 51000 | 3 | 0.031 | 4 |
| supabase | qmd | 63 | 0 | 40 | 4219 | 0 | 4219 | 71095 | 3 | 0.047 | 4 |
| prisma | grep | 75 | 0 | 14 | 3927 | 0 | 3927 | 60166 | 4 | 0.043 | 5 |
| prisma | mda | 75 | 0 | 12 | 2331 | 0 | 2331 | 51637 | 3 | 0.029 | 4 |
| prisma | qmd | 75 | 0 | 39 | 4270 | 0 | 4270 | 71260 | 3 | 0.046 | 4 |
| github-docs | grep | 75 | 0 | 15 | 4080 | 0 | 4080 | 54817 | 3 | 0.039 | 4 |
| github-docs | mda | 75 | 0 | 15 | 3288 | 0 | 3288 | 68822 | 4 | 0.036 | 5 |
| github-docs | qmd | 75 | 0 | 41 | 4850 | 0 | 4850 | 72389 | 3 | 0.048 | 4 |

Totals: 1002 runs, 0 errors, 6 retried, $49.74 list-price, 26655 s of wall-clock at one job (2026-09-25 17:38 → 2026-09-26 01:05 UTC, 135 runs per hour). Generated from `runs.jsonl` per project (rule 0.7: per-turn usage from the transcript; source tokens = Σ over turns of the input-total growth minus the previous output, clipped at 0 per turn, the signed sum beside it).

Reading: 1,002 answers, no failures, 135 runs per hour at one job. mda's median run reads the fewest source tokens on three projects (2.3–3.3K against grep 2.5–4.1K, qmd 3.5–4.9K, graphify 7.3–7.7K; on Tailwind grep reads 5% fewer than mda), makes 3–4 tool calls, and costs the least on every project ($0.028–0.036 against grep $0.038–0.043, qmd $0.044–0.048, graphify $0.065–0.076); qmd's runs are the slowest (39–46 s median wall, its reranker per query). No run had a negative source-token delta, so the clipped and signed sums coincide.

### Quality, grounding and the gates (original grades)

**tailwind-css** (25 questions × 3 runs)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| graphify | 25 | 4.92 | 4.9 (25) | 75 | 75 | 0 | 70.7% | 7846.0 | 7.0 | \$0.076 |
| grep | 25 | 4.80 | 4.8 (25) | 75 | 75 | 0 | 69.3% | 2631.0 | 5.0 | \$0.040 |
| mda | 25 | 4.88 | 4.9 (25) | 75 | 75 | 0 | 77.3% | 2452.0 | 4.0 | \$0.030 |
| qmd | 25 | 4.68 | 4.7 (25) | 75 | 75 | 0 | 80.0% | 3487.0 | 3.0 | \$0.045 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 25 | +0.08 | [-0.60, +0.68] | 5/3/17 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 25 | +0.20 | [-0.12, +0.56] | 4/2/19 | pass | pass | FAIL | none claimed |
| mda vs graphify | 25 | -0.04 | [-0.76, +0.72] | 3/4/18 | FAIL | pass | FAIL | none claimed |

**supabase** (21 questions × 3 runs)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| graphify | 21 | 4.14 | 4.1 (21) | 63 | 63 | 0 | 66.7% | 7398.0 | 4.0 | \$0.066 |
| grep | 21 | 4.33 | 4.3 (21) | 63 | 63 | 0 | 73.0% | 3856.0 | 4.0 | \$0.040 |
| mda | 21 | 4.43 | 4.4 (21) | 63 | 63 | 0 | 61.9% | 2648.0 | 3.0 | \$0.031 |
| qmd | 21 | 4.57 | 4.6 (21) | 63 | 63 | 0 | 50.8% | 4303.0 | 3.0 | \$0.044 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 21 | +0.10 | [-0.29, +0.48] | 4/3/14 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 21 | -0.14 | [-0.62, +0.33] | 3/4/14 | FAIL | pass | FAIL | none claimed |
| mda vs graphify | 21 | +0.29 | [-0.24, +0.86] | 5/2/14 | pass | pass | FAIL | none claimed |

**prisma** (25 questions × 3 runs)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| grep | 25 | 4.96 | 5.0 (25) | 75 | 75 | 0 | 61.3% | 3888.0 | 4.0 | \$0.040 |
| mda | 25 | 4.96 | 5.0 (25) | 75 | 75 | 0 | 50.7% | 2400.0 | 3.0 | \$0.028 |
| qmd | 25 | 5.48 | 5.5 (25) | 75 | 75 | 0 | 44.0% | 4270.0 | 3.0 | \$0.047 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 25 | +0.00 | [-0.80, +0.76] | 6/4/15 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 25 | -0.52 | [-1.16, +0.00] | 2/4/19 | FAIL | pass | FAIL | none claimed |

**github-docs** (25 questions × 3 runs)

| arm | questions | mean score (failed = 0) | completed-only mean (n) | runs | completed | failed | grounding | median source tokens | median calls | median cost |
|---|---|---|---|---|---|---|---|---|---|---|
| grep | 25 | 3.72 | 3.7 (25) | 75 | 75 | 0 | 82.7% | 4102.0 | 3.0 | \$0.038 |
| mda | 25 | 3.68 | 3.7 (25) | 75 | 75 | 0 | 72.0% | 3543.0 | 4.0 | \$0.035 |
| qmd | 25 | 3.76 | 3.8 (25) | 75 | 75 | 0 | 70.7% | 4811.0 | 3.0 | \$0.044 |

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
|---|---|---|---|---|---|---|---|---|
| mda vs grep | 25 | -0.04 | [-0.60, +0.56] | 3/5/17 | FAIL | FAIL | FAIL | none claimed |
| mda vs qmd | 25 | -0.08 | [-0.52, +0.40] | 3/6/16 | FAIL | FAIL | FAIL | none claimed |

Reading: on the 0–6 scale, mda's mean is within 0.1 of grep's on every project (+0.08, +0.10, 0.00, −0.04), above qmd on Tailwind (+0.20, interval [−0.12, +0.56]) and below it on Prisma (−0.52, interval [−1.16, 0.00]) and within 0.15 elsewhere; against graphify +0.29 on Supabase and −0.04 on Tailwind. Every paired interval includes zero except none: the arms answer these questions about equally well with Sonnet, and the medians of three runs make most questions ties (14–19 of 21–25). **No pair passes the three gates, so no savings are claimed.** Gate (a) passes for mda vs qmd on Tailwind and mda vs graphify on Supabase; gate (b), mda's mean ≥ 4.0, passes on three projects and fails on GitHub Docs (3.68, where every arm scores 3.7–3.8); gate (c), grounding ≥ 95%, fails for every arm on every project (mda 51–77%, grep 61–83%, qmd 44–80%, graphify 67–71%) — under the amended rubric the graders still find unsupported product claims in a third to a half of the answers of every arm, and citations that do not resolve or point at oversized pages in a tenth. The consumption differences above are therefore reported as measurements, not as savings.

### The same, on the adjudicated grades (panel-resolved scores in place, rule 0.8)

**tailwind-css** (adjudicated)

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
| mda vs grep | 25 | +0.12 | [-0.58, +0.74] | 6/4/15 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 25 | +0.44 | [+0.00, +0.92] | 6/2/17 | pass | pass | FAIL | none claimed |
| mda vs graphify | 25 | +0.04 | [-0.72, +0.80] | 4/4/17 | FAIL | pass | FAIL | none claimed |

**supabase** (adjudicated)

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
| mda vs grep | 21 | +0.38 | [-0.05, +0.86] | 6/2/13 | pass | pass | FAIL | none claimed |
| mda vs qmd | 21 | +0.21 | [-0.31, +0.74] | 6/3/12 | FAIL | pass | FAIL | none claimed |
| mda vs graphify | 21 | +0.62 | [+0.10, +1.19] | 7/1/13 | pass | pass | FAIL | none claimed |

**prisma** (adjudicated)

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
| mda vs grep | 25 | +0.08 | [-0.72, +0.84] | 7/4/14 | FAIL | pass | FAIL | none claimed |
| mda vs qmd | 25 | -0.40 | [-1.02, +0.12] | 4/5/16 | FAIL | pass | FAIL | none claimed |

**github-docs** (adjudicated)

| pair | n | mean Δ | 95% paired | wins/losses/ties | gate a (lb ≥ −0.25) | gate b (mean ≥ 4.0) | gate c (grounding ≥ 95%) | savings (comparator / mda, completed pairs) |
| mda vs grep | 25 | -0.14 | [-0.78, +0.52] | 4/6/15 | FAIL | FAIL | FAIL | none claimed |
| mda vs qmd | 25 | -0.12 | [-0.52, +0.30] | 3/5/17 | FAIL | FAIL | FAIL | none claimed |

Reading: with the panel's resolved scores in place for the 30 sampled answers per project, the pair means move by up to 0.4 points and gate (a) additionally passes for mda vs grep on Supabase (lower bound −0.05) and mda vs qmd on Tailwind (0.00); gate (c) still fails everywhere, so nothing is claimed. Both analyses are published; the original is the table of record and the adjudicated one its rule-0.8 companion.

### Panel calibration (30 answers per project, Fable and Astra, blind)

| project | answers | incomplete | Fable/Astra exact | within one | Fable/Sonnet exact | Astra/Sonnet exact | trigger fired | mean Sonnet / Fable / Astra |
|---|---|---|---|---|---|---|---|---|
| tailwind-css | 30 | 0 | 0.37 | 0.53 | 0.5 | 0.37 | 18 | 4.87 / 4.77 / 2.73 |
| supabase | 30 | 0 | 0.13 | 0.13 | 0.4 | 0.17 | 28 | 4.37 / 4.87 / 1.1 |
| prisma | 30 | 0 | 0.33 | 0.47 | 0.57 | 0.37 | 18 | 4.83 / 4.93 / 2.8 |
| github-docs | 30 | 0 | 0.23 | 0.33 | 0.33 | 0.2 | 23 | 3.8 / 4.73 / 1.93 |

Generated from `evals/results/docsqa/T2/<project>/panel/regrade.json` (`scripts/eval/panel.sh regrade <out> 30`). Trigger: either panel member differs from the Sonnet grade by more than one point → the panel mean replaces it in the adjudicated grades.

Reading: the two panel members do not agree with each other or with Sonnet at the level the protocol assumed: exact agreement 0.13–0.37 between Fable and Astra and within one point only 0.13–0.53; Fable tracks Sonnet (means within 0.1–0.9), Astra grades two to three points lower on every project (means 1.1–2.8 against Sonnet's 3.8–4.9). The declared trigger fired on 18, 28, 18 and 23 of 30 answers; every individual score is committed. Before T2 is reused, the panel's rubric wording for Astra needs a calibration pass — that is a finding of this table, recorded in STATUS, not a correction applied to it.

### Card audit (100 cards per corpus, both members)

| project | cards | values | unsupported (Fable) | unsupported (Astra) | per-value agreement | incomplete verdicts | sections missing from the store |
|---|---|---|---|---|---|---|---|
| tailwind-css | 100 | 228 | 0.004 | 0.004 | 1 | 0 | 0 |
| supabase | 100 | 474 | 0.016 | 0.018 | 0.995 | 18 | 0 |
| prisma | 100 | 442 | 0.018 | 0.02 | 0.987 | 8 | 0 |
| github-docs | 100 | 352 | 0.038 | 0.032 | 0.987 | 10 | 0 |

Generated from `evals/results/docsqa/T2/<project>/panel/cards.json` (`scripts/eval/panel.sh cards <project> 100`): 100 cards per corpus with at least one date or entity, seeded; each date judged as "raw → iso (precision)", each entity against the section text; an incomplete verdict is one member not covering every value once, in order (counted, not scored).

Reading: 0.4–3.8% of the sampled dates and entities are judged unsupported by either member, with per-value agreement above 0.98; GitHub Docs has the highest rate (3–4%). Incomplete verdicts (a member's reply not covering every value once) are counted, not scored: 0, 18, 8 and 10 of 100 cards. This is the audit of rule 0.8 on the metadata G6 promises: every date and entity carries evidence in the source, and the members find that true for 96–99.6% of the sampled values.

### What travels with the table

Every model call went through the owner's logins; provider keys were unset by the scripts. The freeze `T2/FROZEN.md` was committed before any run and re-written once during the preflights, with identical inputs, because the T3 table renderer (`scripts/eval/t3.sh`, no T2 input) was added under it; both versions are in git history and the four preflights were rerun under the second (all passed: freeze check, sample hashes, three activation probes per arm with traces under `preflight/T2-<project>/`). The runs: 1,002 answers, none failed; 6 first attempts (4 graphify, 2 grep) hit the 12-turn cap and the declared single retry succeeded — the rows carry the final attempt's metrics and `cost_usd_all_attempts`. Grading: Sonnet through `claude -p`, one row without a grounding verdict (Prisma) is a failed grounding. The grounding rubric is the amended one (plan §2.4): factual claims about the product, whole cited pages, one citation rule for every arm; the failure causes per arm over all runs are cited-but-unsupported claims (mda 83 of 288 runs, qmd 93, grep 60, graphify 40 of 138), unresolvable citations (qmd 17, grep 12, mda 11, graphify 1), no citation (grep 10, mda 4, graphify 2) and a cited page over 120,000 characters (mda 11, grep 5, graphify 3, qmd 1), each an ungrounded answer by the declared policy. The panel's Astra member grades the 0–6 rubric two to three points below Sonnet and Fable on every project; the declared trigger therefore fired on 18–28 of 30 answers per project and the adjudicated analysis is shown beside the original, not in its place. Committed under `evals/results/docsqa/T2/<project>/`: the frozen sample, the manifest (home paths abbreviated), `grades.jsonl` and `grades.adjudicated.jsonl` with the answer text replaced by its sha256 (answers quote page text; they stay under the run directory), both analyses, the panel's per-answer scores and the card verdicts (rationales removed); the analysis regenerates byte-identically from the committed grades and manifest. T2's latency is not a table: every run started every arm's MCP server cold (rule 0.9 as amended) and the wall-clock medians above are that configuration's. T1 published this table's arms' retrieval quality; T2 does not reopen tuning.

## T3 — axis E, what a first build costs (final, 2026-09-25)

The first build of each arm on the four corpora, from the committed records: mda's cards from their provenance (`cards-0.1.1-<project>.json.provenance.json`), the raw index and attach-plus-embed times from the T1 preflight's clean reconstruction, the qmd, graphify and BM25-over-files arm records (`arms/<arm>-<project>.json`), and the size of the artifact each arm scores from, measured on the benchmark machine (`T3/sizes.json`). Costs are list-price equivalents of calls that went through the owner's Claude Code login. The incremental cost after one edited section is T4's measurement (M7) and the last column says so. This is a table of design properties, not a savings claim: a build-cost comparison across arms is only fair at matched models and matched outputs (Codex, `docs/reviews/codex/2026-09-23-build-cost-claim.md`), and the model each arm used is in its row.

| Project | sections | arm | first build wall-clock | model cost (list-price eq.) | tokens in / out | wall per 1K sections | cost per 1K sections | artifact size | incremental after one edit |
|---|---|---|---|---|---|---|---|---|---|
| github-docs | 23066 | mda (cards + vectors) | raw index 5 s · summarise: one `claude -p` per section, wall not recorded at M1 · attach + embed 1733 s | $99.93 (Haiku 4.5) | 55979737 / 8635389 | embed ≈ 75 s | $4.33 | 181.3 MB | one section: hash-keyed, only the edited section is re-summarised and re-embedded (measured at M7, T4) |
| github-docs | 23066 | qmd 2.8.3 | 1397 s (embed 1390 s, local EmbeddingGemma on Metal) | $0 (no remote model call) | – | 61 s | $0 | 92.8 MB | `qmd update && qmd embed` (measured at M7, T4) |
| github-docs | 23066 | graphify | did not complete (1 attempt(s), last 4745 s · 98 turns) | $73.54 spent, no graph | – | – | – | – | – |
| github-docs | 23066 | graphify-haiku (haiku-hosted `/graphify`) | 454 s · 11 turns | $1.6 + $0.62 in 1 failed attempt(s) | 5197298 / 118463 | 20 s | $0.07 | 45.7 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| github-docs | 23066 | BM25-over-files | 2 s | $0 | – | 0.1 s | $0 | 28.7 MB | rebuild the table (seconds) |
| prisma | 10438 | mda (cards + vectors) | raw index 2 s · summarise: one `claude -p` per section, wall not recorded at M1 · attach + embed 713 s | $39.29 (Haiku 4.5) | 21679828 / 3466004 | embed ≈ 68 s | $3.76 | 106.4 MB | one section: hash-keyed, only the edited section is re-summarised and re-embedded (measured at M7, T4) |
| prisma | 10438 | qmd 2.8.3 | 496 s (embed 494 s, local EmbeddingGemma on Metal) | $0 (no remote model call) | – | 48 s | $0 | 31.1 MB | `qmd update && qmd embed` (measured at M7, T4) |
| prisma | 10438 | graphify | did not complete (2 attempt(s), last 2518 s · 69 turns) | $82.77 spent, no graph | – | – | – | – | – |
| prisma | 10438 | graphify-haiku (haiku-hosted `/graphify`) | 340 s · 1 turns | $0.97 + $4.28 in 1 failed attempt(s) | 2758765 / 73155 | 33 s | $0.09 | 1.4 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| prisma | 10438 | BM25-over-files | 1 s | $0 | – | 0.1 s | $0 | 9.5 MB | rebuild the table (seconds) |
| supabase | 6548 | mda (cards + vectors) | raw index 1 s · summarise: one `claude -p` per section, wall not recorded at M1 · attach + embed 483 s | $30.48 (Haiku 4.5) | 16615918 / 2675968 | embed ≈ 74 s | $4.66 | 80.3 MB | one section: hash-keyed, only the edited section is re-summarised and re-embedded (measured at M7, T4) |
| supabase | 6548 | qmd 2.8.3 | 397 s (embed 394 s, local EmbeddingGemma on Metal) | $0 (no remote model call) | – | 61 s | $0 | 25 MB | `qmd update && qmd embed` (measured at M7, T4) |
| supabase | 6548 | graphify (sonnet-hosted `/graphify`) | 2053 s · 264 turns | $47.17 | 80562590 / 1783121 | 314 s | $7.2 | 2.2 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| supabase | 6548 | graphify-haiku (haiku-hosted `/graphify`) | 451 s · 71 turns | $6.14 | 12908648 / 441091 | 69 s | $0.94 | 1.2 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| supabase | 6548 | BM25-over-files | 0 s | $0 | – | 0 s | $0 | 7.7 MB | rebuild the table (seconds) |
| tailwind-css | 1518 | mda (cards + vectors) | raw index 0 s · summarise: one `claude -p` per section, wall not recorded at M1 · attach + embed 103 s | $6.51 (Haiku 4.5) | 3591960 / 498631 | embed ≈ 68 s | $4.29 | 11.5 MB | one section: hash-keyed, only the edited section is re-summarised and re-embedded (measured at M7, T4) |
| tailwind-css | 1518 | qmd 2.8.3 | 162 s (embed 161 s, local EmbeddingGemma on Metal) | $0 (no remote model call) | – | 107 s | $0 | 8 MB | `qmd update && qmd embed` (measured at M7, T4) |
| tailwind-css | 1518 | graphify (sonnet-hosted `/graphify`) | 825 s · 83 turns | $10.25 | 10340885 / 410586 | 543 s | $6.75 | 0.4 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| tailwind-css | 1518 | graphify-haiku (haiku-hosted `/graphify`) | 342 s · 40 turns | $2.31 | 5205526 / 143619 | 225 s | $1.52 | 0.4 MB | `/graphify --update` skill flow through the login (measured at M7, T4) |
| tailwind-css | 1518 | BM25-over-files | not recorded (the table was built at M2, its record written afterwards: `arms/bm25-files-tailwind-css.json`) | $0 | – | – | $0 | 2.1 MB | rebuild the table (seconds) |

Generated by `scripts/eval/t3.sh table` from `evals/results/docsqa/cards-0.1.1-<project>.json.provenance.json`, `arms/<arm>-<project>.json`, the T1 preflight reconstruction timings and `T3/sizes.json` (arm64 15.2, measured 2026-09-25T17:23:08Z). Sections = the corpus's markdown sections as mda parses them (the unit for every arm's per-1K figure). Model costs are list-price equivalents of calls that went through the owner's Claude Code login. mda's summarisation wall-clock was not recorded at M1 (cards were built across sessions); its per-section cost is the provenance's. Incremental cost after one edit is T4 (M7).

Reading:
- **mda** pays once per section, in bounded Haiku calls: $4.29 to $4.66 per 1,000 sections on every corpus, with no agent loop, and the summarisation is hash-keyed (an unchanged section is never sent again). Its store is the largest artifact (FTS content plus f32 vectors: 181 MB for GitHub Docs); the T1 page lists the levers.
- **qmd** makes no remote model call: a local embedding model on Metal, 48 to 107 seconds per 1,000 sections, an index of 8 to 93 MB that also holds its query cache.
- **graphify** with a Sonnet host completed on the two small corpora only ($6.75 and $7.20 per 1,000 sections, 314 and 543 seconds) and did not complete on Prisma ($82.77 over two attempts) or GitHub Docs ($73.54, one attempt), both stopped by the account's weekly limit mid-build; the Haiku-hosted builds completed everywhere at $0.07 to $0.94 per 1,000 sections and scored near zero on T1. The graphs are small (0.4 to 46 MB).
- **BM25-over-files** costs seconds and nothing else, and is the control T1 uses.

## Not measured yet

- The A/B protocol on corpora beyond the golden set (design-partner repos; this repository's own `docs/` is a candidate at 28 files / 5K lines).
