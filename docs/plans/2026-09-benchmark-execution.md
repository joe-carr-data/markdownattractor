# Benchmark execution plan — how we run it, and how we beat them

Status: **draft v1 for Codex pre-mortem** · 2026-09-23 · companion to `2026-09-benchmarks.md` v3.2 (the rules, axes, datasets and owner decisions there are unchanged and bind this file; where the two disagree, v3.2 wins and this file gets fixed) · owner intent: "tailored at beating competitors"

Goal: turn the strategy plan into a schedule of tables we can run one command at a time, each with a competitor profile that says where we expect to win, where we expect to lose, what would make us lose, and what we do then. "Beating" means: on the questions a Claude Code user actually asks of a folder of markdown, markdownattractor answers at least as well as qmd and graphify while reading fewer source tokens, staying fresh without a rebuild, and answering time questions they cannot; and the page shows the cases where that is not true.

## 0. What is already in hand (2026-09-23)

| Item | State |
|---|---|
| DocsQA-Repo, four projects at pinned commits | cloned, raw-indexed, **fully carded and embedded** (36,899 cards, 0 failures, 7 h 17 min through the owner's Claude Code login), coverage 100%, evidence anchors 100/100/98/82% (`docs/benchmarks.md`) |
| Split | seeded (20260922), written per project: dev 30 / test 55 / holdout 15; holdout sealed behind `--open-holdout` |
| Adapter | `mda eval --dataset docsqa`: page-level success@5, MRR@5, nDCG@10; coverage and anchor report; fetch until ten distinct pages |
| Harness | `scripts/eval/ab.sh` (arms baseline/index, manifest, fail-loud), `scripts/eval/grade.sh` (`claude -p` grader, completeness check, conventional medians) |
| Golden corpus | 32 docs; A/B at 11–12/12 parity; source tokens 762.5 vs 245.5 (the index loses on tiny corpora, published) |
| Not yet | card export and commit (rule 0.9); `FROZEN.md` (rule 0.1); qmd and graphify installed; competitor arms in `ab.sh`; transcript-counted tokens (rule 0.7); grounding check (rule 0.4c); `panel.sh` (rule 0.8); freshness and temporal harnesses; card export tooling |

## 1. Competitor profiles: where we win, where we lose, what would prove it

Each profile ends with a **must-not-lose** line (the result that would contradict our pitch) and a **response** (what we do if it happens). Numbers marked *claimed* are theirs, unverified here.

### 1.1 qmd (tobi/qmd, `@tobilu/qmd` 2.8.3, MIT, ≈ 30K stars)

What it is: an on-device markdown search engine. BM25 (SQLite FTS5) + vectors (EmbeddingGemma-300M) + LLM reranker (Qwen3-Reranker-0.6B) + a fine-tuned 1.7B query-expansion model, fused by reciprocal rank fusion with position-aware blending; 900-token chunks with 15% overlap; MCP server (`qmd mcp`: `query`, `get`, `multi_get`, `status`); CLI `qmd query` (hybrid), `qmd search` (BM25 only), `qmd vsearch` (vector only), `qmd query --no-rerank`; `qmd update` re-indexes incrementally on demand; no watch mode; Node ≥ 22 or Bun; ≈ 2 GB of models downloaded on first use.

Where it is strong, and we should expect to lose or tie:
- **Axis A with the reranker on.** A 300M embedder plus a cross-encoder reranker plus query expansion is a heavier retrieval stack than our 33M bge-small over cards. On paraphrased questions qmd's full configuration may beat our hybrid on success@5. This is the one table where "beating" is not the goal; **approaching it is**, and the honest comparison is three rows: qmd full, qmd `--no-rerank` (same fusion class as ours), mda hybrid.
- Chunk-level snippets with query terms highlighted are good for exact identifiers.

Where it is structurally weak, and the tables that show it:
- **No cards.** A 900-token chunk is what the model reads; we hand back a ≈ 80-token card with `tldr` and then the exact lines. Axis B (source tokens per answer at parity) is where the difference in *what the model reads* shows, on corpora where grep is expensive.
- **No watcher, no time model.** `qmd update` is a manual step; a section edited a minute ago is invisible until someone runs it, and nothing records when a section changed. Axes C (save → answer) and D (what changed / when) are ours by construction; rule 0.5 requires giving qmd the same evidence (a re-index command in the loop, the git history for D), and the table still shows the rebuild cost and the absence of section-level time.
- **Cost to build and to run.** Vectors for 900-token chunks of a 23K-section corpus on a 300M model, plus a reranker call per query (0.6B model, CPU) → axis E build minutes and axis A latency columns. We publish their numbers next to ours without editorialising.

Must-not-lose: mda hybrid below qmd `--no-rerank` on success@5 on the dev split of any DocsQA project by more than 0.05. Response: the tuning loop in §3 (card embedding text, fusion weights, fetch depth, a larger local embedder behind an ADR); if after tuning we still lose, the page says so and the pitch shifts weight to B/C/D, which do not depend on winning A.

### 1.2 graphify (safishamsi/graphify, `graphifyy` v0.9.66, Apache-2.0, ≈ 120K stars)

What it is: `/graphify .` builds a knowledge graph of a folder (code through tree-sitter locally; docs, PDFs and media through the assistant's model), tags edges EXTRACTED / INFERRED, writes `graph.json`, `GRAPH_REPORT.md` and an HTML view; `graphify query "…"` returns a scoped subgraph, `graphify path`, `graphify explain`; Claude Code integration through a PreToolUse hook nudging toward `graphify query`, a skill, and an MCP server (`query_graph`, `get_node`, `shortest_path`); rebuilds on commit through git hooks, `graphify update` incremental; benchmark *claimed*: LOCOMO recall@10 0.497, LongMemEval-S 76% QA accuracy.

Where it is strong:
- Scope (37 languages, PDFs, media) and structural questions ("what connects X to Y"). None of our axes measure that, and the page says so: we do not race on scope.
- Community size and the hook pattern we adopted from it.

Where it is structurally weak for the questions we measure:
- **Prose retrieval is a side effect.** Nodes are concepts, not passages with line ranges; for "how do I roll back", the model gets a subgraph and still has to open files. Axis B measures source tokens per answer and tool calls; axis A measures whether the labelled page surfaces at all (through `query_graph`, mapped to the files its nodes cite).
- **Rebuild, not live.** Git-hook rebuilds mean an uncommitted edit is invisible; the docs part of a rebuild is a model pass over the corpus (axis E build cost, in minutes and tokens; our incremental re-card of one edited section is seconds).
- **No time model.** Axis D questions have no graph answer; with the git history as evidence (rule 0.5) the model can still answer through Bash, which is what the git baseline measures.

Must-not-lose: graphify's arm at parity on axis B with fewer source tokens than ours on any DocsQA project. Response: that would mean the graph's summary beats our cards for those questions; we would publish it and examine the questions (document-level cards, plan v3.2 §4, are the planned answer for overview questions).

### 1.3 The baselines that keep us honest

- **grep baseline** (Read/Grep/Glob, no index): wins on tiny corpora and on exact identifiers; our break-even corpus size is a published number, not a hidden one. Must-not-lose: parity gate (rule 0.4) failing on a DocsQA project; response: it is a quality problem before a cost problem, and no saving is claimed.
- **BM25-over-files** (one FTS5 index over whole pages): the control for chunking; it separates "sections and cards help" from "any index helps".
- **git baseline** (axis D): Claude with `git log`/`git diff` in Bash. Convenience and correctness at equal evidence is the claim; "others score zero" is not.

## 2. Tables, in the order they run

Every table: frozen inputs first (`FROZEN.md`), dev split for tuning, test split published, holdout sealed; raw logs, effective configs and traces under `evals/results/`; failures shown (rule 0.3).

### T1. Axis A on DocsQA (retrieval) — first, because everything is in hand

| Arm | Command (per project) | Config published |
|---|---|---|
| mda raw | `mda eval --dataset docsqa … --split dev` row `lexical (raw only)` | bm25 weights, fetch |
| mda cards | same, row `lexical (cards + raw)` | card fields and weights |
| mda hybrid | same, row `hybrid` (bge-small-en-v1.5-q) | embedding text, RRF k, recency off |
| qmd full | `qmd collection add <checkout> --name <p>`; `qmd update`; `qmd embed`; per question `qmd query "<q>" -n 30 --json` → page dedupe → same metrics | qmd version, models, chunking |
| qmd no-rerank | `qmd query --no-rerank` | idem |
| qmd BM25 | `qmd search` | idem |
| BM25-over-files | a one-off FTS5 index over whole pages (script under `scripts/eval/`) | tokenizer |
| graphify | `graphify query "<q>"` → the files its returned nodes cite, in order → same metrics | version, model used for docs, build time |

Adapter work: a `--arm-output <jsonl>` mode of the DocsQA scorer that scores an externally produced ranked list of paths per question (one JSONL row `{question_id, paths[]}`), so competitor arms reuse the same metrics, exclusions and split. Published: one table per project, dev split during tuning, then the test split once, plus latency per query per arm and build minutes per arm (feeds T5).

Acceptance: mda hybrid ≥ qmd no-rerank − 0.05 on success@5 on every project's test split, or the loss stated per project with the tuning history.

### T2. Axis B on DocsQA (answer quality and cost) — after T1's tuning is frozen

Arms: grep baseline, mda, qmd (full), graphify. Sample: the 25-question frozen test sample per project (stratified from the test split, seed in `FROZEN.md`), three runs per arm per question, Sonnet answering through `claude -p`, Sonnet grading through `claude -p` with the grounding check, Fable + Astra panel on 30 answers per project (rule 0.8).

Harness work (B0 leftovers): per-arm manifests in `ab.sh` (`--arm mda|qmd|graphify|grep`: tools, MCP config, skill/rules file, smoke-test question and the trace assertion that the arm's tool was used), transcript-counted tokens (rule 0.7), grounding check and citation extraction in `grade.sh`, `scripts/eval/panel.sh`, `FROZEN.md` writer, the harness failure-matrix shell test.

Published per project: mean score with the paired-bootstrap CI, parity fraction, median source tokens, median total input tokens, tool calls, wall-clock, list-price equivalent; the rule-0.4 gate applied before any saving is claimed; the cold-first-question column.

Acceptance: gates of rule 0.4 met for mda on at least three of four projects; source tokens per answer for mda below grep, qmd and graphify on those projects; where not, the "where we lose" section names the project and the reason.

### T3. Axis E (cost to build) — measured alongside T1/T2

Per arm and project: first index wall-clock, tokens and list-price equivalent; incremental re-index after one edited section (mda: seconds and one card; qmd: `qmd update` + `qmd embed` minutes; graphify: `graphify update` minutes and model tokens); store size in MB per 1K sections (mda: 192 MB for GitHub Docs today; the FTS content duplication and f32 vectors are the known levers, plan follow-up).

### T4. Axis C (freshness) on Prisma

Protocol from v3.2: same edit trigger (append a dated sentence to one section), fixed 1 s polling, 300 s timeout, three distributions per arm: save → raw-searchable, save → card, save → correct grounded answer. Arms: mda daemon, qmd (`qmd update && qmd embed` in the loop, its documented path), graphify (`graphify update`), grep (reads the file: the floor, always fresh, always expensive). Twenty edits per arm. Harness: `scripts/eval/freshness.sh`, new.

Acceptance: mda p50 save → card under 15 s (G1) and save → correct answer under the others' rebuild time; fallback reads recorded.

### T5. Axis D (time questions) on the DocsQA repositories

Protocol from v3.2 (historical replay): a fixed range of real commits per repository; per step, check out, set mtimes from the commit author date, `MDA_NOW=<author date>` (test-only override to build in `file_times`, event timestamps and `first_seen_at`), `mda index`; validate stored timestamps against `git log` and publish the validation table; 40 questions with git-computed ground truth ("which pages changed between A and B", "when was P last changed", "which pages were added in month M", "which sections of P changed since D"). Arms: git baseline (Bash allowed), grep, mda (`mda_timeline`, `mda_recent`, section `updated_at`), qmd, graphify — every arm gets the repository with `.git`.

Acceptance: mda success@5 and answer score at least the git baseline's on "which sections" questions and within its CI on the rest; the validation table has no unexplained mismatch.

### T6. Own corpora and break-even (axis B on `evals/golden`, this repo's `docs/`, the owner's private repo)

Already partly done for the golden set; rerun after T2's harness lands so the numbers share a method; the private repo appears in the "supporting evidence" table only.

## 3. The tuning loop (dev split only, T1 then T2)

Allowed knobs, each a one-line config or a documented option, each change logged in `evals/results/docsqa/TUNING.md` with the dev numbers before and after:
1. Card embedding text (which fields, order): today `title + heading_path + tldr + summary + keywords + questions_answered`.
2. `cards_fts` column weights (`heading_path` 3, `tldr` 3, `summary` 1, `keywords` 1, `questions_answered` 2, `entities` 1) and the raw list's weights.
3. RRF constant and per-list weights (cards, raw, vector), fetch depth, and whether recency is off for frozen corpora (it is, in the adapter).
4. Query form: the question as typed (default), or the question with stop-words dropped for the AND form before OR fallback.
5. A larger local embedder as an option (bge-base or EmbeddingGemma-300M through fastembed) **only through an ADR** and only if the gap to qmd no-rerank is the embedder, shown by a vector-only ablation.
6. Latency: candidate depth, one prepared statement per section lookup, release build. Reported, and never traded for quality without saying so.

Forbidden: looking at test or holdout numbers while tuning; changing the split, the question sample, the grader or the rubric after `FROZEN.md`; per-project settings (one configuration for all four projects; the published config is the product default).

Stop rule: tuning ends when two successive changes move dev success@5 by less than 0.01, or after the session budgeted for it (§4); the final dev configuration is frozen and becomes the product default in `config.toml` if it differs from today's.

## 4. Schedule (sessions of ≈ 4 hours; each ends with its exit criterion met or the shortfall written down)

| Session | Work | Exit |
|---|---|---|
| S5 | Card export (`--export-cards`), commit the four card files; `FROZEN.md` writer and the first `FROZEN.md`; `--arm-output` scorer; qmd installed and indexed on the four checkouts; mda rows (raw/cards/hybrid) on dev | T1 mda rows on dev published in a draft table; qmd indexes built with times recorded (T3) |
| S6 | qmd rows (full, no-rerank, BM25), BM25-over-files, graphify install/build/rows on dev; tuning loop started | T1 dev table complete for all arms; `TUNING.md` started |
| S7 | Tuning loop finished; freeze; T1 on the test split; the page's T1 section with "where we lose" | T1 published |
| S8 | B0 leftovers: per-arm manifests with smoke traces, transcript tokens, grounding check, `panel.sh`, failure-matrix test; Codex pass on the harness (v3.2 §7 follow-up) | harness verified against rules 0.3, 0.5, 0.7, 0.8 |
| S9 | T2 runs (four projects × four arms × 25 × 3) and grading; panel calibration; T3 | T2 and T3 published |
| S10 | T4 freshness harness and runs | T4 published |
| S11 | `MDA_NOW` + replay harness, validation, T5 questions and runs | T5 published |
| S12 | T6 rerun, README numbers row, page restructured by axis, sealed holdout stays sealed | plan v3.2 exit criteria ticked |

Wall-clock that does not fit a session runs detached (`nohup`, as the carding did) through the owner's Claude Code login (plan §0a.3; no dollar budget).

## 5. Risks, and what we do

| Risk | Signal | Response |
|---|---|---|
| qmd full beats mda hybrid on A by a wide margin | dev gap > 0.10 after knobs 1–4 | vector-only ablation; if the embedder is the gap, ADR for a larger embedder as an *option* (download size and latency on the page); the pitch does not depend on A |
| Cards bias fusion (seen at 14% coverage) | any project with partial coverage | never publish a carded row below 100% coverage; product follow-up: fusion weights by coverage or cards-list off until backfill completes |
| graphify build on 3.7K pages is slow or costly | build > 1 h or model errors | build once, record it (T3); if it cannot complete, the arm is reported as "did not complete" with the log (rule 0.3), not dropped |
| Competitor arm not actually used by Claude (rule 0.5) | smoke trace shows no tool call | fix the arm's manifest/hook before the table; never publish a table whose trace assertion failed |
| Grader drift or injection | panel disagreement > 1 point on > 10% of the calibration sample | publish the disagreement; move to the panel mean for the affected table |
| Liquid includes on GitHub Docs (18% of anchors) | already known | stated per table; consider indexing `data/reusables` as support text for a second row |
| Test-split reuse across releases | every release | stated on the page; holdout opened once at 1.0 |
| Rate limits during long runs | rounds without progress | the runner sleeps and resumes; wall-clock published under T3 |
| The harness itself is wrong | Codex harness pass (S8) | fix before T2; T1 numbers are recomputed from `results.json` by script, never retyped |

## 6. Deliverables

- `docs/benchmarks.md` restructured by axis: T1–T6 tables, one "where we lose" section that names corpus sizes and questions, one "what the model already knew" note (rule 0.6), links to raw logs, `FROZEN.md` and `TUNING.md`.
- README: one numbers row with the three headline claims only when their tables pass their gates: fewer source tokens at parity (T2), fresh within seconds (T4), answers time questions the others cannot (T5).
- `evals/results/docsqa/`: cards per project and version, `FROZEN.md`, `TUNING.md`, per-arm logs and traces.

## 7. Tasks

- [ ] S5: card export + commit; `FROZEN.md` writer + first freeze; `--arm-output` scorer; qmd installed and indexed; mda dev rows.
- [ ] S6: qmd rows, BM25-over-files, graphify rows on dev; tuning loop started.
- [ ] S7: tuning frozen; T1 test split published; "where we lose" written.
- [ ] S8: harness leftovers; Codex harness pass.
- [ ] S9: T2 + T3 published; panel calibration.
- [ ] S10: T4 published.
- [ ] S11: T5 published (with `MDA_NOW`, validation table).
- [ ] S12: T6, README row, page restructure.

## 8. Exit criteria

- [ ] T1–T6 published with the arms of §2, on the test split, `FROZEN.md` predating every number in git history.
- [ ] For every competitor profile in §1, the must-not-lose line is either met or the loss is on the page with its reason.
- [ ] The tuning history is public and the product default equals the published configuration.
- [ ] Codex pre-mortem of this plan and the S8 harness pass triaged in `docs/reviews/codex/`.
