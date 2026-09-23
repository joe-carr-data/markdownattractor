# Benchmark execution plan — how we run it, and how we beat them credibly

Status: **v3.1 after three Codex passes (third pass: no new High; verdict "not yet without qualifications", quoted in the review; the qualifications are milestones M1–M5)** · 2026-09-23 · companion to `2026-09-benchmarks.md` v3.2 (the rules, axes, datasets and owner decisions there bind this file; where the two disagree, v3.2 wins and this file gets fixed) · review: `docs/reviews/codex/2026-09-23-execution-plan.md` · owner intent: "tailored at beating competitors" and "reproducible for anyone in a Claude session" (`evals/benchmark_it_with_claude.md`)

Goal: turn the strategy plan into ordered milestones, each a table we can run with one command, each preceded by a machine-checked preflight, each with a competitor profile that says where we expect to win, where we expect to lose, what would make us lose, and what we do then. "Beating" means: on the questions a Claude Code user asks of a folder of markdown, markdownattractor answers at least as well as qmd and graphify while reading fewer source tokens, is fresh within seconds without a rebuild, and answers time questions correctly with less effort at equal evidence; and the page shows every case where that is not true.

## 0. What is in hand and what is not (2026-09-23)

| Item | State |
|---|---|
| DocsQA-Repo (`PowderXu/docsqa-data` @ `19af578`, manifest sha256 `c6193cc8…`), four projects at pinned commits | cloned, raw-indexed, **fully carded and embedded** (36,899 cards, 0 failures, 7 h 17 min through the owner's Claude Code login); coverage 100%; evidence anchors 100/100/98/82% (`docs/benchmarks.md`) |
| Split | seeded (20260922), written per project: dev 30 / test 55 / holdout 15; holdout sealed behind `--open-holdout` |
| Adapter | `mda eval --dataset docsqa`: page-level success@5, MRR@5, nDCG@10; coverage and anchor report; fetch until ten distinct pages (truncation recorded) |
| Harness | `scripts/eval/ab.sh` (baseline/index arms, manifest at the end of the loop, fail-loud), `scripts/eval/grade.sh` (`claude -p` grader, completeness check, conventional medians) |
| **Exploratory numbers so far** | the DocsQA raw/carded rows on the dev split and the golden-corpus A/B are **exploratory** (no freeze predates them); they stay on the page under that label and are never the published T1/T2 numbers |
| Not yet | card export and commit (rule 0.9); `FROZEN.md` writer and freeze lifecycle (§2.0); qmd and graphify installed; competitor arms; scorer for external ranked lists; pooled blinded judgments; paired-bootstrap analysis; transcript-counted tokens (rule 0.7); grounding check (rule 0.4c); panel scripts (rule 0.8); resumable runner with a manifest written first; freshness and temporal harnesses; the runbook |

## 1. Competitor profiles: where we win, where we lose, what would prove it

Each profile ends with a **must-not-lose** line (the result that contradicts the pitch) and a **response**. Claims marked *claimed* are theirs, unverified here. Fairness evidence for each arm (install recipe, effective configuration, coverage of the corpus, three activation probes with traces) is produced by the preflight of §2.0 and published with the table; a profile is opinion until that evidence exists.

### 1.1 qmd (tobi/qmd, `@tobilu/qmd` 2.8.3, MIT, ≈ 30K stars)

What it is: an on-device markdown search engine. BM25 (SQLite FTS5) + vectors (EmbeddingGemma-300M) + LLM reranker (Qwen3-Reranker-0.6B) + a fine-tuned 1.7B query-expansion model, fused by reciprocal rank fusion with position-aware blending; 900-token chunks with 15% overlap; MCP server (`qmd mcp`: `query`, `get`, `multi_get`, `status`); CLI `qmd query` (hybrid), `qmd search` (BM25 only), `qmd vsearch` (vector only), `qmd query --no-rerank`; `qmd update` re-indexes incrementally on demand; no watch mode; Node ≥ 22 or Bun; ≈ 2 GB of models on first use. To verify in preflight: whether it indexes `.mdx` (if not, its corpus coverage is smaller and the table says so), the resolved model revisions, and that `--no-rerank` changes nothing but reranking (configuration diff published).

Where it is strong, and we should expect to lose or tie:
- **Axis A with the reranker on.** A 300M embedder plus a cross-encoder reranker plus query expansion is a heavier retrieval stack than our 33M bge-small over cards. **qmd full is the primary comparison** (strategy §1: "must at least match qmd on labels"); the `--no-rerank` and BM25-only rows are ablations that locate the gap, not the target.
- Chunk-level snippets with query terms highlighted are good for exact identifiers.

Where it is structurally weak, and the tables that show it:
- **No cards.** A 900-token chunk is what the model reads; we hand back a ≈ 80-token card and then the exact lines. T2 (source tokens per answer at parity) on corpora where grep is expensive.
- **No watcher, no time model.** `qmd update` is a manual step; nothing records when a section changed. T4 gives qmd its documented update command inside the loop and measures the same endpoint for every arm (save → correct answer); T5 gives every arm the git history.
- **Cost to build and to run.** Vectors for 900-token chunks of a 23K-section corpus on a 300M model and a reranker call per query → T3 build minutes per 1K sections and T1 latency, measured through its MCP server warm, with the cold first query separate (§2.7).

Must-not-lose: mda hybrid below **qmd full** on success@5 on the test split of any project, outside the paired 95% interval. Product target: match qmd full; publication target: the table is complete whatever the outcome. Response: the tuning loop of §3 on the dev split; if the vector-only ablation shows the embedder is the gap, an ADR for a larger local embedder as an *option* (download size and latency on the page); if we still lose, the page says so per project and the pitch rests on T2/T4/T5, which do not depend on winning T1.

### 1.2 graphify (safishamsi/graphify, `graphifyy` 0.9.66, Apache-2.0, ≈ 120K stars)

What it is: `/graphify .` builds a knowledge graph of a folder (code through tree-sitter locally; docs, PDFs and media through the assistant's model), tags edges EXTRACTED / INFERRED, writes `graph.json`, `GRAPH_REPORT.md` and an HTML view; Claude Code integration through a PreToolUse hook that nudges toward the graph, a skill, and an MCP server (`query_graph`, `get_node`, `shortest_path`); rebuilds on commit through git hooks, `graphify update` incremental; *claimed*: LOCOMO recall@10 0.497, LongMemEval-S 76%. Its arm uses the **MCP tools in both T1 and T2** (one interface, the one Claude would use); the CLI is not used for scoring.

Where it is strong: scope (37 languages, PDFs, media) and structural questions ("what connects X to Y"). None of our axes measure that; the page says so and we do not race on scope.

Where it is structurally weak for the questions we measure:
- **Prose retrieval is a side effect.** Nodes are concepts, not passages with line ranges; T1 measures whether the labelled page surfaces through `query_graph` (node → source file, in the order the tool returns nodes, deduplicated; the mapping is frozen and published); T2 measures source tokens and tool calls to a graded answer.
- **Rebuild, not live.** An uncommitted edit is invisible until a rebuild; the docs part of a rebuild is a model pass (T3 build cost; T4 with `graphify update` in the loop).
- **No time model.** T5 gives it the git history like every arm; the question is correctness and effort at equal evidence, never "others score zero".

Must-not-lose: graphify at parity on T2 with fewer source tokens than ours on any project, outside the paired interval. Response: publish it, examine the questions; document-level cards (strategy §4) are the planned answer for overview questions.

### 1.3 The baselines that keep us honest

- **grep baseline** (Read/Grep/Glob): wins on tiny corpora and exact identifiers; the break-even corpus size is published. Passing the rule-0.4 gate against grep establishes parity with grep only; parity with qmd and graphify is gated separately (§2.6).
- **BM25-over-files** (one FTS5 index over whole pages): the control that separates "sections and cards help" from "any index helps".
- **git baseline** (T5): Claude with `git log`/`git diff` in Bash; every answering arm may read history the same way.

## 2. Protocol, common to every table

### 2.0 Freeze lifecycle and the preflight (rules 0.1, 0.5, 0.9)

Three kinds of numbers, labelled on the page: **exploratory** (before any freeze: what exists today), **development** (after the development-protocol freeze, dev split only, used for tuning and pilots, published in `TUNING.md`, never as a result), **final** (after the per-table freeze, test split, published once per release with reuse stated).

`FROZEN.md` per table (written by `scripts/eval/freeze.sh`, validated by every run, so a run against changed inputs refuses to start): dataset commit and manifest hash; the four repo SHAs; `mda` version and git SHA; embedding model name and revision; cards file hashes; every arm's version, resolved model ids and effective configuration; prompts and rubric hashes; the split file hash and the frozen question sample; the analysis script hash; hardware and OS.

`scripts/eval/preflight.sh <table> <project>` is machine-checked and must pass before a table runs: `FROZEN.md` matches the inputs; corpus coverage per arm (pages indexed by the arm / pages in the dataset corpus, published: an arm that skips `.mdx` shows it here); a clean reconstruction check for mda (rebuild the index from the committed cards, re-embed with the hashed model files → identical T1 metrics on the whole dev split, not a sample); for graphify, the built `graph.json` is archived under `evals/results/` with its hash and reloaded for scoring; every artifact an arm scores from (cards, embeddings, qmd index, graph) has its hash in `FROZEN.md`; **three activation probes per arm** with the traces proving the arm's tool was used (mda MCP, qmd MCP, graphify MCP and its hook, grep); timing boundaries recorded (§2.7).

### 2.0b Reproduction: regeneration is the gate, reruns are published, never a gate (Codex N1)

Two different checks, never confused. **Regeneration**: every table is recomputed from its archived raw observations (logs, grades, ranked lists) by the analysis command on a clean checkout, and must be byte-identical; this is the publication gate. **Independent rerun**: for stochastic tables (T2, T4, T5) a preregistered rerun (same frozen inputs, same repeat count, fresh samples) is run once after publication and its results are published beside the original with the paired difference, whatever it shows; agreement is reported, it is not a pass/fail and never suppresses or replaces the original. The runbook (`evals/benchmark_it_with_claude.md`) states which check each step is.

### 2.1 Arms and their fairness evidence

| Arm | Install (pinned) | Index / build | Query interface for scoring |
|---|---|---|---|
| mda | this repo at the frozen SHA, `cargo build --release`, model `bge-small-en-v1.5-q` | `mda index --no-summarize`; cards from the committed files; `mda rebuild --embeddings` | `mda mcp` (`mda_search`, `k` up to the page rule) |
| qmd full | `npm i -g @tobilu/qmd@2.8.3` | **one qmd home per project** (`QMD_HOME`/config dir, so a query cannot see another project's collection), `qmd collection add <checkout> --name <p>`, `qmd update`, `qmd embed`; model files hashed, revisions recorded | `qmd mcp` `query`; the request (including its result-count parameter and any collection filter) captured verbatim in preflight |
| qmd no-rerank | same | same index | the MCP `query` with reranking disabled if the tool exposes it (request captured), else `qmd query --no-rerank` on the CLI, in which case the row has no latency column and the table says so; the configuration diff against the full row is published and must contain nothing but reranking |
| qmd BM25 | same | same index | the MCP `query` with a single lex-only sub-query and `rerank: false` (qmd's lexical engine through the same tool; quality only, no latency column; its AND-of-every-term form is qmd's own, the CLI `qmd search` behaves the same) — implemented at M2 |
| BM25-over-files | `scripts/eval/bm25-files.sh` (FTS5 over whole pages, unicode61) | one table per project | the script (quality only, no latency column) |
| graphify | `uv tool install graphifyy==0.9.66`, then `graphify install` (writes its skill and hook; the written files are archived) | `/graphify <checkout>` from a Claude Code session with the docs model recorded, `graph.json` archived and hashed, build time and tokens recorded (T3) | MCP `query_graph` (request captured; node → file mapping frozen: each node's source file(s) in the order the tool lists them, then by path; all nodes of the response are taken, and the response size limit, if any, is recorded as the truncation) |
| grep | none | none | Read/Grep/Glob |

### 2.2 Scoring external arms

`mda eval --dataset docsqa … --arm-output <jsonl>` scores a ranked list of repository paths per question (`{"question_id": …, "paths": […], "truncated": bool}`) with the same metrics, exclusions, split and page rule as the mda rows. Every arm's driver requests enough results to reach ten distinct pages or exhaustion (qmd: raise `-n` until ten pages; graphify: all returned nodes), normalises paths to `repository_source_path`, deduplicates in rank order, and records truncation; a truncated question is scored and counted.

### 2.3 Labels: original and pooled (axis A, second column)

Original sparse labels first. Then, **on the final test-split runs** (after the freeze, M4), per project, pool the top-5 pages of every arm, keep the **unlabelled** query-page pairs, sample 100 (seeded, stratified over arms) and have the Fable + Astra panel judge each blind to the arm and to each other on a frozen 0/1/2 rubric ("does this page answer the question"). A pair is relevant when the mean of the two judgments is ≥ 1; every individual judgment and the agreement are published. The second column recomputes success@5, MRR@5 and nDCG@10 with labels = original ∪ pooled-relevant, over the judged questions, and is labelled "pooled, model-assisted, 100 pairs/project". The development pooled judgments of M3 are diagnostic only.

### 2.4 Answer quality, grading, panel (T2)

Three runs per arm per question; Sonnet answers through `claude -p`; Sonnet grades through `claude -p` with a JSON schema, the rubric as system prompt, the submission as tagged data, plus the grounding check (every claim supported by a cited page; pass/fail per answer). **One failure policy for everything** (§2.5): a failed, missing or empty run is an observation that scores 0, fails grounding, and contributes no token, call or cost value to any saving; it is counted in every denominator and listed per arm. The panel (Fable through `claude -p --model <resolved id>`, Astra through the Codex CLI) re-grades 30 answers per project blind and audits 100 cards per corpus (dates and entities against the source); every individual score is kept; agreement published; **a disagreement of more than one point on the 0–6 scale is resolved by the panel mean** (rule 0.8 verbatim, no other threshold).

### 2.5 Analysis (rule 0.4, implemented in `mda eval --analysis <grades.jsonl>`, in `mda_core::eval`, unit-tested)

The sample is the frozen question set, whole. Question-level score per arm = median of its three runs; **a failed or missing run scores 0 for that run** (rule 0.3: a failure is a result) and fails grounding, so it lowers the arm's median and its grounding rate instead of vanishing; the number of such runs is published per arm. Paired difference per question over the whole sample; 10,000 paired bootstrap resamples over questions with seed 20260922; percentile 95% interval (2.5th and 97.5th); the three gates (interval lower bound ≥ −0.25; mda mean over the whole sample ≥ 4.0; grounding pass rate over all graded-or-failed answers ≥ 95%) **per comparator pair** (mda vs grep, mda vs qmd full, mda vs graphify); savings claimed only for the pairs and projects that pass; a secondary "completed runs only" view is published for information and never gates. Per-question rows published.

### 2.6 Runner (rule 0.3, resumable)

`ab.sh` writes the manifest **before** the loop (question × arm × run ids), resumes by skipping rows already present, bounds concurrency, records retries and exit codes, and marks a failed row as such so the analysis scores it 0 (§2.5) and excludes it from every cost figure; `grade.sh` checks completeness against the manifest. A development pilot (5 questions × 4 projects × all arms) measures runs per hour before T2 is scheduled.

### 2.7 Timing boundaries

Latency is measured through each tool's MCP server (one server per arm per project, started cold, the first query reported as the cold number including process start and model load, the rest as warm), with the same client (`scripts/eval/mcp-time.sh` over the rmcp client); arms without an MCP server (qmd BM25, BM25-over-files, grep) have no latency column. In T2, every `claude -p` invocation starts every arm's MCP server cold, for every arm alike; the page says so and no warm-server advantage is claimed for anyone. **This deviates from strategy rule 0.9's "one server per arm per run, warm after the first question"**, because `claude -p` owns the server lifecycle; the deviation is recorded here as a proposed amendment to v3.2 rule 0.9 (owner to accept at M5), and until accepted the T2 table carries the note. Hardware, OS, cache state and the build profile (release) are in `FROZEN.md`.

## 3. The tuning loop (development numbers only, T1; T2 gets a pilot, not tuning)

Objective: mean success@5 over the four projects' dev splits, equal weights, with the guardrail that no project drops by more than 0.02 from the pre-tuning configuration. Candidates are pre-declared, **one change each, at most eight, applied by greedy forward selection**: each candidate is evaluated on top of the current winner (the pre-tuning configuration at the start), kept if it improves the objective by ≥ 0.01 and passes the guardrail, otherwise discarded; the list, in order:
1. Card embedding text: prepend `questions_answered` before `tldr`.
2. Card embedding text: append `entities`.
3. `cards_fts` weight `questions_answered` 2 → 3.
4. RRF per-list weight for the raw list 1.0 → 0.7.
5. RRF k 60 → 30.
6. Fetch depth 30 → 60 before page deduplication.
7. Query form: stop-words dropped for the AND form before the OR fallback.
8. Larger local embedder (bge-base) **only through an ADR**, and only if a vector-only diagnostic (run outside the candidate list, never selected) shows the embedder is the gap.

Selection is the greedy rule above; "simpler" (for a tie within 0.01) means fewer settings different from the pre-tuning default, and the pre-tuning configuration wins a tie against everything. Regressions are logged, not retried with variations. Every trial goes to `evals/results/docsqa/TUNING.md`: hypothesis, config diff, code SHA, cards and embeddings hashes, per-project metrics and denominators, latency, build cost, failures, elapsed time, decision. Stop: the list is exhausted, or the last two candidates both fail to improve the objective by ≥ 0.01. The winner becomes the product default in `config.toml`; test and holdout are never looked at; a test loss is a result, not a new round.

## 4. Tables

- **T1 axis A on DocsQA**: arms of §2.1; original and pooled columns (§2.3); latency (§2.7); per project, dev during tuning, test once. Acceptance: complete, with per-project intervals; the product target (match qmd full) is reported as met or not per project.
- **T2 axis B on DocsQA**: arms grep, mda, qmd full, graphify; 25 frozen test questions per project (stratified from the test split, seed in `FROZEN.md`); reference answers from the dataset's `normalized_answer` (an export from the adapter, `--export-questions`); §2.4–2.6. Headline claims restricted to the projects and comparator pairs that pass §2.5.
- **T3 axis E**: per arm and project: first build wall-clock, tokens, list-price equivalent, per 1K sections; incremental cost after one edited section; store size per 1K sections (mda today: 192 MB for GitHub Docs; FTS content duplication and f32 vectors are the levers).
- **T4 axis C on Prisma**: one edit trigger (a dated sentence appended to one section), one update trigger per edit for the arms that need one (`qmd update && qmd embed`; graphify's documented `/graphify <path> --update` skill flow through the login, since its docs pass needs the model and the CLI's `graphify update` re-extracts code only — Codex, 2026-09-23), 1 s polling, 300 s timeout, twenty edits; endpoints: save → raw-searchable (mda, qmd BM25), save → card (mda only; n/a elsewhere), **save → correct grounded answer (every arm, the comparison endpoint)**; timeouts, fallback reads and raw-search latency published. Acceptance: complete; mda p50 save → card under 15 s (G1) reported against the goal.
- **T5 axis D**: simulated historical replay (labelled as such): a fixed commit range per repository; per step, checkout, mtimes from the author date, `MDA_NOW=<author date>` (test-only override), `mda index`; stored timestamps validated against `git log`, table published, a failed validation voids the replay; 40 questions with git-computed ground truth; every arm gets `.git` and may read history in Bash; the git baseline is an arm. Score per question, by question type: "which pages/sections" questions, the answer's set against the git ground truth, F1 ≥ 0.8 → 1, else 0; "when" questions, the stated date within one day of the git date → 1, else 0; "added in month M" questions, the set F1 as above. Effort has two measures, tool calls and source tokens, both published. Criterion, paired bootstrap over the 40 questions (seed 20260922), mda vs the git baseline and mda vs each other arm: the claim "correct with less effort at equal evidence" is made for a pair only when the correctness difference's interval lower bound is ≥ −0.05 **and both** effort differences' upper bounds are < 0; otherwise the row is published without the claim.
- **T6 own corpora**: golden set, this repo's `docs/`, the owner's private repo (supporting evidence only), rerun with the T2 method.

## 5. Milestones (ordered; publication depends on completion and verification, not on a session number)

| # | Milestone | Exit |
|---|---|---|
| M1 | Card export and commit; `freeze.sh` and the development-protocol freeze; `preflight.sh` with reconstruction check and probes for the mda and grep arms; `--arm-output` scorer with tests; the runbook covers everything above | preflight passes for mda; runbook reproduces the exploratory rows exactly |
| M2 | qmd and graphify installed and built on the four checkouts (times recorded); their drivers, coverage, probes and traces; BM25-over-files | preflight passes for every arm on every project |
| M3 | T1 development rows for all arms; pooled judgments sampled and judged; tuning loop (§3) run and logged | `TUNING.md` complete; winner frozen |
| M4 | T1 final freeze; test split once; page section with "where we lose"; Codex pass on the scorer, drivers and freeze | **T1 published** |
| M5 | T2 harness: resumable runner, transcript tokens, grounding check, analysis command with tests, panel scripts, failure-matrix test, question export; development pilot (throughput measured); Codex pass on the harness (v3.2 §7 follow-up) | harness verified against rules 0.3, 0.4, 0.5, 0.7, 0.8 |
| M6 | T2 final freeze; runs (4 projects × 4 arms × 25 × 3 = 1,200 answers, detached, resumable); grading; panel calibration and card audit; T3 | **T2, T3 published** |
| M7 | T4 harness and runs | **T4 published** |
| M8 | `MDA_NOW`, replay harness, validation, T5 | **T5 published** |
| M9 | T6; README numbers row limited to gates passed; page restructured by axis; holdout still sealed | strategy v3.2 exit criteria ticked |

Estimates come from the pilots (M1 runbook timing for builds, M5 pilot for T2 throughput) and are written into the milestone when known; until then no dates. T1 and T2 are independent of T4/T5; a slip there never delays or changes a published T1/T2.

## 6. Risks, and what we do

| Risk | Signal | Response |
|---|---|---|
| qmd full beats mda hybrid on T1 outside the interval | dev gap after the candidate list | vector-only ablation; ADR for a larger embedder as an option; the loss is published per project |
| Small dev samples (Supabase: 12 eligible dev questions) | interval width | report intervals; the aggregate objective weights projects equally but the page shows each; no per-project tuning |
| Cards bias fusion below full coverage | any partial index | never publish a carded row below 100% coverage; product follow-up (fusion by coverage) |
| An arm skips `.mdx` or a directory | preflight coverage | published in the arm's coverage column; the arm is still run |
| graphify build slow or failing on 3.7K pages | build > 1 h or errors | recorded in T3; a failed build is "did not complete" with the log (rule 0.3), never dropped |
| Activation probe fails | trace without the tool call | fix the manifest/hook before the table; never publish without three passing probes |
| Grader injection or drift | panel disagreement | published; the rule-0.8 adjudication applies per answer |
| Liquid includes on GitHub Docs (18% of anchors) | known | stated per table; a second row with `data/reusables` indexed as support text is a follow-up |
| Long runs interrupted or rate-limited | rows missing | resumable runner; wall-clock and retries published (T3) |
| The harness or the analysis is wrong | Codex passes at M4 and M5 | fixed before publication; numbers regenerated by script from the raw files, never retyped |

## 7. Deliverables

- `docs/benchmarks.md` by axis: T1–T6, "where we lose" (corpus sizes, projects, comparators), "what the model already knew" (rule 0.6), links to `FROZEN.md`, `TUNING.md`, raw logs and traces.
- `evals/benchmark_it_with_claude.md`: the runbook any Claude session follows to reproduce every published table; **updated in the same change as every harness, arm, freeze or table change (owner rule, 2026-09-23), never after**; **a table is published only after the runbook regenerates it byte-identically from the archived observations on a clean checkout** (§2.0b); independent reruns of stochastic tables are published beside the original, never gated; the runbook grows a table-specific section (commands, expected artifacts, hashes) with every published table; `/benchmark` (`.claude/skills/benchmark`) invokes it.
- README: one numbers row per headline claim, each limited to the projects and comparators whose gate passed, worded as measured: fewer source tokens at parity (T2), fresh within seconds (T4), time questions answered correctly with less effort at equal evidence (T5).

## 8. Tasks

- [ ] M1 export, freeze, preflight, `--arm-output`, runbook v1.
- [ ] M2 competitor arms installed, built, driven, probed.
- [ ] M3 T1 development rows, pooled judgments, tuning loop.
- [ ] M4 T1 published (test split), Codex pass.
- [ ] M5 T2 harness (runner, tokens, grounding, analysis, panel, pilot), Codex pass.
- [ ] M6 T2 + T3 published.
- [ ] M7 T4 published.
- [ ] M8 T5 published.
- [ ] M9 T6, README row, page restructure.

## 9. Exit criteria

- [ ] T1–T6 published on the test split with the arms of §2.1, every table preceded in git history by its `FROZEN.md` and a passing preflight, regenerated byte-identically by the runbook on a clean checkout, and (T2, T4, T5) followed by one published independent rerun.
- [ ] For every profile in §1, the must-not-lose line is met or the loss is on the page with its interval and reason.
- [ ] `TUNING.md` public; the product default equals the published configuration.
- [ ] Both Codex passes (M4, M5) triaged in `docs/reviews/codex/`.
