# Plan — M5: the T2 answer-quality harness (2026-09-25)

Milestone M5 of the execution plan (`2026-09-benchmark-execution.md` §5), protocol §2.4–2.7, strategy rules 0.3, 0.4, 0.5, 0.7, 0.8, 0.9 (`2026-09-benchmarks.md` §0). T2 is axis B on DocsQA: arms grep, mda, qmd full, graphify; 25 frozen test questions per project; three runs per arm per question; Sonnet answers and grades through `claude -p`; grounding checked; a two-model panel re-grades a calibration sample and audits cards; analysis with paired bootstrap and the three gates. M5 builds and verifies the harness on a development pilot; M6 freezes and runs T2.

## Decisions carried in (2026-09-25, owner: "decide for me and continue working")

- **T1 stands as published**: the Supabase graphify-haiku probe (2 of 3 in two attempts) is disclosed, not retried a third time; the arm's rows come from its tool directly.
- **Page-level score aggregation** ("where we lose", T1) is a declared hypothesis for a future tuning round: written into the plan before any run, evaluated on the dev split only, never on the opened test split. Not part of M5.
- **Rule 0.9 amendment accepted** (execution plan §2.7): in T2 every `claude -p` invocation starts every arm's MCP server cold, for every arm alike, because `claude -p` owns the server lifecycle; the T2 table says so and claims no warm-server advantage for anyone.

## Goal

A harness a fresh session can run from the runbook that produces, for every (project, question, arm, run): the answer, the transcript's own token counts (rule 0.7), source tokens from consecutive turns, tool calls, wall-clock, cost, the exit code and the failure flag; then grades (0–6) and a grounding verdict per answer; then the analysis (§2.5) with the gates per comparator pair; then the panel's calibration and card audit — with failures as results everywhere (rule 0.3), resumable (§2.6), and verified by a failure-matrix test before any pilot number is read.

## Tasks

- [x] **Q1 question export**: `mda eval --dataset docsqa … --export-questions <file> --sample 25 --sample-seed 20260922`: eligible questions of the chosen split, stratified over `community_category` by seeded order (`blake3(seed ‖ id)`), with `reference` = the dataset's `normalized_answer`, the relevant pages, the category; one file per project, committed at M6's freeze.
- [x] **R1 runner** `scripts/eval/t2.sh run <project> <questions.jsonl> <out> [runs=3] [arms…]`: manifest before the loop; every arm launched as `probe.sh` launches it (shared `arm_launch` in `lib.sh`: mda MCP + search-first rules, grep, qmd MCP + qmd skill, graphify MCP from the checkout copy with its project settings); resume by skipping rows present; bounded concurrency; retries and exit codes recorded; a failed row marked (`error: true`) and kept.
- [x] **R2 tokens (rule 0.7)**: per assistant turn `usage` from the stream; `source_tokens` = Σ over turns of (input total of turn t − input total of turn t−1 − output of turn t−1), never `chars/4`; the estimate label removed from the T2 rows.
- [x] **G1 grading** `scripts/eval/t2.sh grade <out>`: Sonnet, rubric 0–3 + 0–3 with the reference, structured output; **grounding**: a second structured call with the answer and the text of every page it cites (repository-relative citations resolved in the checkout; an uncited claim or an unresolvable citation fails); failed/empty runs → score 0, grounding fail, counted, listed per arm; completeness against the manifest.
- [x] **A1 analysis** `mda eval --analysis <grades.jsonl>` (`mda_core::eval::analysis`, unit-tested): question-level score per arm = median of runs with failed runs as 0; paired differences per comparator pair (mda vs grep, mda vs qmd full, mda vs graphify) over the whole sample; 10,000 paired bootstrap resamples, seed 20260922, percentile interval; gates (a) lower bound ≥ −0.25, (b) mda mean ≥ 4.0, (c) mda grounding pass rate ≥ 95%; savings (source tokens, tool calls, cost) claimed only for pairs and projects that pass; a secondary "completed runs only" view, never gating; per-question rows and per-arm failure counts in the output; `--json` and a Markdown table.
- [x] **P1 panel** `scripts/eval/panel.sh regrade <out> [n=30]` (Fable through `claude -p`, Astra through `codex exec`, blind, same rubric; agreement with each other and with Sonnet; a disagreement > 1 point on 0–6 resolved by the panel mean, recorded beside the original) and `panel.sh cards <project> [n=100]` (dates and entities of a seeded card sample checked against the source section by both members; agreement published).
- [x] **F1 failure matrix**: `scripts/eval/tests/failure-matrix.sh` (run by `make check`): synthetic rows for errored / timed-out / empty-answer / ungraded / missing runs through grade completeness and `--analysis`; asserts each scores 0, fails grounding, is counted in every denominator, contributes to no saving, and never yields parity; plus the Rust unit tests of the analysis.
- [x] **D1 pilot** (development, labelled): 5 dev questions × 4 projects × the arms available (graphify where its graph exists) × 1 run; runs per hour, cost per run, failure rate; grades and analysis exercised end to end; numbers on the page as "pilot, development, not a result".
- [x] **C1 Codex pass** on the harness (v3.2 §7 follow-up) — triaged in `docs/reviews/codex/`, findings fixed before M6.
- [x] **Docs**: runbook §5 rewritten for T2 (commands, rules, the 0.9 amendment), `evals/README.md`, CHANGELOG, STATUS, handoff, this plan ticked.

## Exit criteria

- [x] The failure matrix passes: every failure kind is a result (rule 0.3), never a zero-versus-zero parity.
- [x] Token numbers on T2 rows come from the transcript's `usage` (rule 0.7), source tokens from turn differences, no `chars/4`.
- [x] Every arm's launch configuration is the probe's (rule 0.5) and is recorded in the manifest.
- [x] `--analysis` reproduces the gates of rule 0.4 on a fixture with known answers, and its output regenerates byte-identically from `grades.jsonl`.
- [x] The panel scripts run both members on one sample and publish agreement (rule 0.8).
- [x] The pilot ran end to end on the dev split with throughput and failure rate recorded; nothing from it is published as a result.
- [x] Codex pass triaged; runbook §5 reproduces the pilot.

## Outcome (2026-09-25)

Pilot: 70 runs, 0 errors, 132 runs per hour at one job, $3.93 list-price; grades, grounding, analysis, panel (32 answers) and a 20-card audit exercised end to end; artifacts under `evals/results/docsqa/t2-pilot/<project>/` (manifest with hashes, analysis, regrade, cards; answers and grades stay under the run directory, they carry page text). The harness finding that must be decided before M6's freeze: with rule 0.8 read literally, grounding passes 20–100% per arm and project, so gate (c) cannot pass for anyone; the resolver is no longer the cause (one declared citation rule), the rubric's treatment of an answer's own reasoning is.
