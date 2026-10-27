# Benchmarks — development and exploratory rows (2026-09-22 to 2026-09-25)

Moved verbatim from `docs/benchmarks.md` on 2026-10-26 when the page was reorganised by axis (M9). These are the rows that led to the published tables: the golden-corpus A/B runs, the DocsQA-Repo ingestion gate and development rows, the competitor arms on the dev split, the tuning loop, the post-stop exploration, the pooled-label diagnostic and the T2 harness pilot. Nothing here is a published number; the tables of record are on `docs/benchmarks.md`. `scripts/eval/tests/page-blocks.sh` checks that every table, caption and heading of these sections is unchanged.

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
