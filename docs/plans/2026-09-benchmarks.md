# Benchmark plan — public datasets, competitors, and where we can shine

Status: **draft for review** · 2026-09-22 · plan §11, §2.3, §2.4 · owner intent: "a benchmarking strategy where we can shine, on a golden dataset others use, clearly beating the alternatives"

Goal: publish numbers that (a) other people can rerun, (b) are computed on datasets we did not write, (c) compare like with like against the tools a Claude Code user would otherwise install, and (d) show the three things markdownattractor is built for: answer quality at parity with far fewer source tokens on large corpora, freshness (seconds from edit to correct answer), and time questions nobody else answers. Where we lose (tiny corpora) the page says so; that is what makes the wins credible.

## 1. What is measured, and what "shine" means

| Axis | Metric | Who competes | Where we expect to win |
|---|---|---|---|
| A. Retrieval | success@5, MRR@5, nDCG@10 against the dataset's relevance labels | mda lexical, mda hybrid, qmd, grep-as-retrieval (BM25 over files) | hybrid on paraphrased questions; must at least match qmd |
| B. Answer quality + tokens (the CodeGraph/claude-context protocol) | Sonnet-graded score 0–6 against a reference; parity gate; **source tokens** (tool results), total input tokens, tool calls, wall-clock, cost; medians of 3 runs | grep baseline, mda, qmd, graphify | corpora ≥ 1K sections: parity with 3–10× fewer source tokens; on ≤ 200 sections we lose and say so |
| C. Freshness | seconds from a file edit to a correct answer to a question that depends on the edit | mda daemon vs qmd (manual re-index) vs graphify (rebuild) | only mda is live; report the others' rebuild time honestly |
| D. Time questions | success@5 and answer score on questions whose answer needs *when* (changed since, created when, current vs superseded) | mda vs the same tools | uncontested lane; reported in its own table, never averaged into A/B |
| E. Cost to build | $, minutes and tokens per 1K sections, first index and incremental re-index after one edit | all | incremental cost near zero after one edit |

## 2. Datasets: public first, ours second

**Primary — DocsQA-Repo** (`PowderXu/docsqa-data`): 467 real community questions over 4,860 documentation pages from four markdown/MDX documentation repositories (GitHub Docs 197 q, Prisma 125, Supabase 52, Tailwind CSS 93), pinned to exact commits (`data/manifest.json`), with reference answers (`data/answers.jsonl`: original community answer, normalised answer, `qrel_ids` sparse relevant-page labels) and model-assisted grading aspects (`data/aspects.jsonl`). This is our use case almost exactly: real questions about real markdown documentation, four corpora of different sizes, and a frozen revision anyone can clone. It carries axes A, B and E directly. Caveats to state on the page: `qrel_ids` are sparse (a correct page outside the labels counts as a miss, which hurts every system equally); the aspects are model-assisted, not expert-validated; we index the markdown source at the pinned commit, not their extracted text, so page identity has to be mapped (path ↔ page id).

**Secondary — FreshStack** (arXiv 2504.13128, CC-BY-SA 4.0): retrieval over code and technical documentation from GitHub repositories on five recent programmer topics, queries from community Q&A, nugget-level relevance. Corpora mix code and docs; we run only the documentation subset and say so. Used because the retrieval community already quotes it (axis A only).

**Temporal — TEMPO** (arXiv 2601.09523, CC-BY 4.0): 1,730 queries needing "what changed" and validity reasoning across 13 domains with gold documents per step; the best published system reaches NDCG@10 of 32. Feasibility check first: its corpora are documents with time stamps, not versioned files, and our clocks are filesystem and git time. If it cannot be run faithfully, we do not bend it; instead **the DocsQA repositories give us a temporal set for free**: they are git repositories, so we replay a range of real commits through the daemon and generate questions of the form "what changed in <area> between <date> and <date>", "when was <page> last changed", "which pages were added in <month>", with answers computed from git itself (ground truth, no model). Axis D. This is the set where the competitors score near zero by construction, so it is published as its own table with that stated.

**Ours** (already fixed): `evals/golden` (32 docs, 117 sections, 60 queries); this repository's `docs/` (28 docs, 224 sections); the owner's trading-research repo (582 docs, 7,380 sections, private: numbers published, corpus not). They span the size range the plan asked for and anchor axis E.

Not used: CRAG, FRAMES, MultiHop-RAG (web/Wikipedia/news, not documentation), TechQA (IBM technotes, HTML, old), MTRAG (multi-turn), SWE-bench-style code tasks (code, not prose; CodeGraph and claude-context own that lane).

## 3. Competitors and fairness rules

- **grep baseline**: Claude with Read/Grep/Glob only. Every table has it.
- **qmd** (BM25 + vectors + LLM rerank over markdown, MCP): the closest tool. Run with its documented defaults and its own MCP server in the same headless harness.
- **graphify** (LLM-built knowledge graph, "consult graph first" hook): run its `/graphify` build at the pinned revision, its own cost recorded under axis E, its hook enabled.
- Not run: CodeGraph, Understand Anything, claudix, claude-context (code indexes; we cite their own published numbers and say the lanes differ).
- Same questions, same model (`sonnet`), same grader, same `--setting-sources "" --strict-mcp-config` harness, medians of 3 runs, corpora and question sets committed before any run, raw logs published under `evals/results/`. Each tool gets its recommended configuration and one round of "did it actually use the index" verification, as their own guides ask for.

## 4. Features that change the numbers, and whether to wait

| Feature | Axis it moves | Effort | Decision |
|---|---|---|---|
| Leaner hit payload: `k` default 5 for MCP, drop `snippet` when a card exists, shorter `tldr`-only mode | B (source tokens): today 8 hits ≈ 1.3K tokens whatever the corpus | small | **Do before B runs.** Cheap, and it is the one number readers quote. |
| Read ledger (schema v4) | measurement only | small | not needed; the harness measures tokens from the transcript |
| Document-level cards (plan §4.4 reducer) | A on "which page covers X" questions, B on overview questions | medium | **Do not wait.** Run the benchmark, ship doc cards, rerun: two rows on the page show the gain. |
| Content dates as a ranking signal (`mentioned_dates`) and `since`-style filters on them | D | medium | build after the first D results say how often filesystem time is not the right clock |
| Phase 3 `superseded_by` / validity | D ("is this still current") | large | **Do not wait.** Keep "is this current" out of the question set until it exists; the D set is "what changed / when" which the stored data already answers. |
| Reranker / better query encoder | A | medium | only if hybrid loses to qmd on A |

Rule: the benchmark is versioned by `mda` release and rerun on every tag (a CI job for axis A; B/C/D by hand because they spend). Publishing early does not lock the numbers in; it locks the method in.

## 5. Tasks

- [ ] B0 Harness: `mda eval` gains a dataset adapter (`--dataset docsqa <dir>`) mapping page ids ↔ paths and reporting success@k / MRR / nDCG@10 against `qrel_ids`; `scripts/eval/ab.sh` gains `--arm qmd|graphify` and per-arm setup scripts; results land in `evals/results/<dataset>/<date>-<mda-version>.md` with raw JSONL.
- [ ] B1 Own corpora, mda vs grep (axes B, E): golden, `docs/`, trading repo (private numbers). Break-even corpus size measured.
- [ ] B2 DocsQA-Repo, axis A (mda lexical, mda hybrid, qmd) on all four corpora; axis B on a 25-question sample per corpus (mda, grep, qmd, graphify).
- [ ] B3 Freshness (axis C): scripted edit → question loop on the Prisma corpus for mda, qmd, graphify.
- [ ] B4 Temporal set from DocsQA git history (axis D): generator (`scripts/eval/temporal-questions.sh`), 40 questions, ground truth from git; mda vs qmd vs graphify vs grep.
- [ ] B5 FreshStack docs subset, axis A only.
- [ ] B6 `docs/benchmarks.md` restructured by axis with one table each, the "where we lose" section, and links to raw logs; README "How it compares" gets a numbers row and a link.
- [ ] Leaner hit payload PR before B2 (see §4).

## 6. Exit criteria

- [ ] Every number on the page can be regenerated from `evals/` and public data with one command per axis, and the raw logs are in the repo.
- [ ] DocsQA-Repo: retrieval results for mda and qmd on all four corpora, and a B table with the parity gate.
- [ ] A temporal table with ground truth from git, where the competitors' scores are reported, not omitted.
- [ ] A freshness table with seconds, including the competitors' rebuild times.
- [ ] The "where we lose" section exists and is honest about corpus size.
- [ ] Budget: ≤ $60 API for cards across the public corpora, ≤ $100 of Claude Code usage for the B runs, graphify's own build cost recorded separately. Owner asked before anything above that.

## 7. Open questions for review

1. Is DocsQA-Repo the right primary, or is its sparse labelling too harsh to show retrieval differences?
2. Is generating the temporal set from git history sound, or does it favour us in a way a reader would reject?
3. Which of the four DocsQA corpora is large enough for axis B to show a token gain, given the 1.3K-token floor per search?
4. Should qmd be run with its LLM reranker on (its best) or off (comparable cost)? Proposal: both rows.
5. What would make a reader distrust this page, and how do we pre-empt it?
