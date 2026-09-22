# Benchmark plan — public datasets, competitors, and where we can shine

Status: **v3.2, accepted by Codex on the third pass, three owner decisions applied (2026-09-22, §0a)** (`docs/reviews/codex/2026-09-22-benchmark-plan.md`); implementation not started (`docs/reviews/codex/2026-09-22-benchmark-plan.md`) · 2026-09-22 · plan §11, §2.3, §2.4 · owner intent: "a benchmarking strategy where we can shine, on a golden dataset others use, clearly beating the alternatives"

Goal: publish numbers that (a) other people can rerun from public data with one command per table, (b) are computed on questions we did not write, (c) compare like with like against the tools a Claude Code user would otherwise install, and (d) show the three things markdownattractor is built for: answer quality at parity with far fewer source tokens on large corpora, freshness, and time questions. Where we lose (tiny corpora, questions the model already knows) the page says so; that is what makes the wins credible.

## 0. Rules that apply to every table (the reader's trust, pre-empted)

Scope statement for the page, from the review: the results cover the declared dataset adaptations, a simulated historical replay and a disclosed test-set reuse; they do not claim universal superiority, and the replay does not claim production recovery of historical timestamps.

1. **Frozen before any run:** dataset commit, question sample (seeded, stratified by project), development/test split, every tool's version and configuration, the resolved model ids (never an alias), prompts, and the analysis. Recorded in `evals/results/<dataset>/FROZEN.md` before the first measurement.
2. **Tune on development, publish test.** DocsQA has no split; we make one per project with seed 20260922: 30% development, 55% test, 15% **sealed holdout**. Development is for tuning; test is published on every release and its reuse across releases is stated on the page; the sealed holdout is opened once, at 1.0, and published as the un-reused number.
3. **Failures are results.** A run that errored, timed out, produced no answer or no grade is counted and shown; it never qualifies for savings and never becomes a zero-versus-zero "parity".
4. **Quality first, then cost.** A table reports token or cost savings only if all three pre-declared criteria hold over the whole test sample: (a) the **lower bound** of the paired bootstrap 95% interval of (index − baseline) mean score is ≥ −0.25 on the 0–6 scale; (b) the index arm's mean score is ≥ 4.0 (absolute floor); (c) the index arm's grounding pass rate (rule 8) is ≥ 95%. Otherwise the table shows quality and cost with no savings claim. Per-question scores and costs are all published; parity-subset savings are labelled with their denominator.
5. **Every arm gets the same evidence and a smoke test.** Before a table is run, each arm has a recorded trace proving its index or hook was actually used on three probe questions, and the effective configuration is published with the traces. Temporal questions give every arm the git history.
6. **Arms are compared as whole systems.** The answering model's own knowledge of public documentation is part of every arm alike and counts for whichever system uses it best; no no-retrieval control is required, and none of the gates depends on one. (Owner decision, §0a.)
7. **Tokens are counted, not guessed:** every token number comes from the API's own tokenizer as reported in the Claude Code transcript (`usage` on each assistant turn); source tokens for a turn are the difference between consecutive turns' input totals minus the previous turn's output, which is what the tool results and framing added. No `chars/4`; where an estimate is unavoidable it is labelled "estimated". Medians are the conventional median (mean of the two middle values).
8. **Grounding is graded, and the graders are audited by a model panel.** The grader checks that every claim in an answer is supported by a cited page (an answer passes when every claim is supported); the pass rate is a gate (rule 4c). Card metadata is audited, not only counted: for every corpus a 100-card sample has its dates and entities checked against the source, and a calibration sample of 30 answers per dataset is re-graded blind. Both audits are done by a **two-model panel, Claude Fable 5.1 (through `claude -p`) and GPT Astra (through the Codex CLI), grading independently** with the same rubric; the panel's agreement with each other and with the Sonnet grader is published, and a disagreement of more than one point on the 0–6 scale is resolved by the two panel scores' mean. No human gate: the owner has decided the panel substitutes for it (§0a).
9. **Public results are reproducible; private results are labelled.** Generated artifacts are preserved: the cards of every public corpus are committed (`evals/results/<dataset>/cards-<mda-version>.json`, as the golden set does) so an index is rebuilt deterministically without a model, and the embedding model name and revision are recorded. Cache state is fixed and stated: one MCP server per arm per run, started cold, warm after its first question; a "cold first question" column is reported separately. The owner's private corpus appears in a clearly separated "supporting evidence" table and is exempt from the reproducibility claim.

### 0a. Owner decisions (2026-09-22)

1. **No human gate.** Calibration of the grader and the metadata audit are done by a two-model panel (Claude Fable 5.1 + GPT Astra), independently, rubric-scored, with agreement published (rule 0.8). Reason: the owner cannot invest the time; two strong models from different vendors are a defensible substitute and the agreement numbers make the substitution visible.
2. **No no-retrieval control.** A model's prior knowledge of public documentation is a property of the model, available to every arm equally, and choosing a model that knows the domain is a legitimate advantage of the system as a whole. Tables compare whole systems (rule 0.6).
3. **Every model call in the benchmark goes through the owner's own Claude Code login, not an API key.** Cards for the public corpora are produced by the `claude-cli` backend (`backend = "claude-cli"`, `claude_cli_policy_ack = true` on the benchmark roots), the answer-quality arms run through `claude -p`, and the Sonnet grader is `claude -p --model sonnet` with a JSON schema; nothing in `evals/` or `scripts/eval/` reads `ANTHROPIC_API_KEY`. Reason, in the owner's words: "We are gonna use the same auth of Claude code, not the Anthropic API Key. This is my Claude account and not a third party app (it is my code) so there is no legal issue for our own tests." This is the personal-use case ADR-0002 §3 keeps open behind the acknowledgement; the plugin's default backend for users is unchanged. Consequences: rule 0.7 counts tokens from the transcript instead of `count_tokens`; the "cost" columns are the CLI's list-price equivalent (`total_cost_usd`), labelled as such; the practical ceiling is the subscription's rate limit, so card generation for the four corpora is scheduled in bounded rounds and the wall-clock per corpus is published under axis E; the committed cards (rule 0.9) carry `backend: claude-cli` in their provenance.

## 1. What is measured, and what "shine" means

| Axis | Metric | Arms | Where we expect to win |
|---|---|---|---|
| A. Retrieval | success@5, MRR@5, nDCG@10 at **page** granularity: a page counts as retrieved at rank r if its first section appears at r after page-level deduplication of the section list. Original sparse labels first; a blinded pooled judgment of top-5 unlabeled pages (sample of 100 query-page pairs per project) as a second column. | mda lexical, mda hybrid, qmd (full, and reranker-off ablation), BM25-over-files | hybrid on paraphrased questions; must at least match qmd on labels |
| B. Answer quality + cost | Score 0–6 vs reference (correctness, completeness) plus grounding check; source tokens (tool results), total input tokens, tool calls, wall-clock, $; medians of 3 runs; rule 4 applies | grep baseline, mda, qmd, graphify | corpora where grep+read costs thousands of tokens per answer; reported break-even size |
| C. Freshness | three distributions per arm, same edit trigger, fixed 1 s polling, 300 s timeout: save→raw-searchable, save→card, save→correct grounded answer; fallback reads recorded | mda daemon, qmd (documented re-index command), graphify (rebuild), grep | only mda is live; the others' rebuild time is the honest comparison |
| D. Time questions | success@5 and answer score on "what changed / when / added in" questions with **ground truth from git**; rule 5 (every arm gets the history) and a **git baseline** (Claude with `git log`/`git diff` in Bash) | git baseline, grep, mda, qmd, graphify | convenience and correctness at equal evidence; never "others score zero by construction" |
| E. Cost to build | $, minutes, tokens per 1K sections; first index and incremental re-index after one edit; graphify's own build cost | all | incremental cost near zero after one edit |

## 2. Datasets: public first, ours second

**Primary — DocsQA-Repo** (`PowderXu/docsqa-data`, schema v3): 467 real community questions over 4,860 documentation pages from four repositories pinned to exact commits, with reference answers (`answers.jsonl`: original and normalised answers, `qrel_ids`, `qrel_anchors`, 601 relevance judgments), grading aspects (`aspects.jsonl`, model-assisted, not expert-validated) and a frozen `corpus.jsonl.gz` with rendered text.

| Project | Repository @ commit | Docs path | Questions |
|---|---|---|---|
| GitHub Docs | `github/docs` @ `c34e3dc` | `content` | 197 |
| Prisma | `prisma/web` @ `c4ac0e9` | `apps/docs/content/docs` | 125 |
| Tailwind CSS | `tailwindlabs/tailwindcss.com` @ `bd868a3` | `src/docs` | 93 |
| Supabase | `supabase/supabase` @ `6ea3567` | `apps/docs/content` | 52 |

**Ingestion gate (F1), before any DocsQA number:** we index the markdown/MDX source at the pinned commit, not the rendered text, so (i) the walker must accept `.mdx` (today it rejects it; three of the four corpora are MDX), (ii) MDX components, imports and includes must degrade to text without dropping headings, (iii) page ↔ path mapping coverage is measured and published (fraction of `qrel_ids` we can map to an indexed file; target ≥ 95%), (iv) questions whose evidence is image-derived text (`question_modalities`, `image_text.jsonl`) are excluded with the count stated, (v) a per-question answerability check confirms the reference evidence is present in our indexed text. Results are labelled "source-repository adaptation of DocsQA-Repo", with the canonical-corpus numbers alongside where we can compute them (BM25 over `rendered_text`).

**Secondary — FreshStack** (arXiv 2504.13128, CC-BY-SA 4.0): mixed code and documentation corpora; we define the documentation subset (retained documents, supported nuggets, eligible queries) and publish coverage before running; labelled a derived benchmark. Axis A only, after B2.

**Temporal — a git-derived set on the DocsQA repositories** (TEMPO was checked: document time stamps, not versioned files; not run). Protocol (F4, both passes): the harness replays a fixed range of real commits with `mda index` per step (not the daemon); for each commit it checks out the tree, **sets the mtime of every changed file to the commit's author date**, and runs the index step with the engine's observation clock pinned to the same date (`MDA_NOW=<commit author date>`, a documented test-only override honoured by `file_times`, event timestamps and `first_seen_at`; `created_at` uses first-seen in replay, never birthtime, since birthtime cannot be set portably). So `created_at`, `updated_at`, `first_seen_at` and every event carry historical time. Stored timestamps are validated against `git log` before questions are asked and the validation table is published; a replay whose validation fails is not used. Questions are limited to facts the index stores and exposes (`mda timeline`, `recent`, section `updated_at`): "which pages changed between A and B", "when was page P last changed", "which pages were added in month M", "which sections of P changed since D". 40 questions, ground truth computed from git, never from a model. Every arm receives the repository with its `.git` history; the git baseline may run `git log`/`git diff`.

**Ours** (fixed): `evals/golden` (117 sections), this repository's `docs/` (224), the owner's trading-research repo (7,380; private, supporting-evidence table only). They anchor axis E and the break-even size.

Not used: CRAG, FRAMES, MultiHop-RAG (web, Wikipedia, news), TechQA (HTML technotes), MTRAG (multi-turn), SWE-bench-style code tasks (CodeGraph and claude-context own that lane; cited, not raced). Codex was asked for a better public dataset for real questions over markdown documentation with labels and knew of none.

## 3. Competitors and fairness

- **grep baseline** in every answer-quality table; **git baseline** in the temporal table.
- **qmd** (BM25 + vectors + LLM rerank, MCP): primary row with its recommended full configuration, ablation row with reranking off, everything else held constant; its local compute and latency reported next to quality.
- **graphify**: `/graphify` build at the pinned revision, its hook enabled and proven active (rule 5), its build cost under axis E.
- The harness (`scripts/eval/ab.sh`) is generalised per arm: each arm declares the tools and MCP servers it needs, and the smoke test asserts they were used. Raw JSONL logs, effective configs and tool-use traces are published under `evals/results/`.

## 4. Features that change the numbers, and whether to wait

| Feature | Axis | Effort | Decision |
|---|---|---|---|
| `.mdx` ingestion (walker + parser tolerance for JSX/imports) | gate for everything on DocsQA | small | **Before B2.** Required. |
| Leaner hit payload: MCP `k` default 5, no `snippet` when a card exists, compact fields | B: today 8 hits ≈ 1.3K tokens whatever the corpus; a 5× saving needs ≈ 6.5K baseline tokens per answer at that floor | small | **Before B2.** The break-even is recomputed after. |
| Harness hardening (fail-loud, counted tokens, schema-validated grades, arms, smoke tests) | all | medium | **B0.** Rules 3, 5, 7 depend on it. |
| Historical replay: mtimes from commit dates plus the `MDA_NOW` observation-clock override | D | small | **B4.** The override is test-only and documented as such. |
| Read ledger (schema v4) | none (harness counts from transcripts) | small | not needed for the benchmark |
| Document-level cards (plan §4.4) | A on "which page" questions, B on overview questions | medium | **Do not wait.** Second row on the page when shipped. |
| Content dates as a ranking signal | D | medium | after the first D results |
| Phase 3 validity / `superseded_by` | D ("is this current") | large | **Do not wait.** Such questions stay out of the set until it exists. |
| Reranker / query encoder | A | medium | only if hybrid loses to qmd on the **development** split |

The benchmark is versioned by `mda` release: axis A reruns in CI on every tag; B, C, D by hand (they spend). Publishing early locks the method, not the numbers.

## 5. Tasks

- [ ] B0 Harness: dataset adapters (`mda eval --dataset docsqa <dir>` with page-level aggregation and coverage report); `ab.sh` per-arm manifests, fail-loud validation, transcript-counted tokens (rule 0.7), conventional medians, smoke tests with traces; `grade.sh` through `claude -p` (no API key), grounding check and schema validation; `FROZEN.md` writer; dev/test split tool.
- [ ] B0a `.mdx` ingestion; B0b leaner hit payload (both PRs before B2).
- [ ] B1 Own corpora, mda vs grep (axes B, E), break-even size.
- [ ] B2 DocsQA ingestion gate, then axis A (all four projects, all arms) and axis B on the frozen 25-question test sample per project.
- [ ] B3 Freshness on Prisma (axis C), three distributions, all arms.
- [ ] B4 Temporal set with historical replay (mtimes + `MDA_NOW`), git validation (axis D), all arms including the git baseline.
- [ ] B5 FreshStack documentation subset (axis A).
- [ ] B6 `docs/benchmarks.md` restructured by axis, the "where we lose" and "what the model already knew" sections, links to raw logs; README numbers row.
- [ ] Panel calibration (Fable + Astra) of the 30-answer sample and the 100-card metadata audit per corpus, agreement published (rule 8); `scripts/eval/panel.sh` runs both models through their APIs/CLIs with the same rubric.
- [ ] Sealed holdout split written and never touched before 1.0 (rule 0.2).

## 6. Exit criteria

- [ ] Every public number regenerates from `evals/` and public data with one command per table; raw logs, effective configs and traces are in the repo; `FROZEN.md` predates the results in git history.
- [ ] DocsQA: ingestion coverage ≥ 95% published; axis A for mda and qmd on all four projects; axis B with the rule-4 criterion.
- [ ] Temporal table with git ground truth, timestamp validation, and a git baseline.
- [ ] Freshness table with the three distributions per arm.
- [ ] "Where we lose" section present and specific (corpus size).
- [ ] Budget: no API-key spend at all (§0a.3); Claude Code usage tracked as the CLI's list-price equivalent, ≤ $60 for cards across the public corpora and ≤ $150 for the B/C/D runs (five arms, three runs, four projects); graphify's build cost recorded separately; owner asked before exceeding either figure or when rate limits stretch a corpus past one day.

## 7. Resolved questions (from the reviews)

Second-pass status (thread `01a0ca49-d99d-7282-8417-91b5c14ecaaf`): F1, F2, F5, F8–F11, F13 resolved; F3, F4, F6, F7, F12 were "partly" and are closed in v3 by rules 0.2 (sealed holdout), 0.4 (CI lower bound, absolute floor, grounding gate), 0.8 (metadata audit), 0.9 (committed cards, cache state) and the `MDA_NOW` replay clock.

1. DocsQA-Repo stays primary, conditional on the ingestion gate; sparse labels are reported as-is plus a pooled blinded column; "others use it" is documented, not assumed.
2. The git-derived temporal set is sound only with historical mtimes, timestamp validation, equal evidence for every arm and a git baseline; all four are in the protocol.
3. No project is assumed large enough for a token gain; all four are measured on the frozen sample and the break-even is computed after the payload change.
4. qmd: full configuration primary, reranker-off ablation, compute and latency reported.
5. Distrust pre-empted by rules 0.1–0.9.
