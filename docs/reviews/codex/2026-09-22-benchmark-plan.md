# Codex review — benchmark plan (pre-mortem as a sceptical reader)

| | |
|---|---|
| Date | 2026-09-22 |
| Scope | `docs/plans/2026-09-benchmarks.md` v1, with `docs/project-plan.md` §2 and §11, `docs/benchmarks.md`, `evals/README.md`, `scripts/eval/{ab,grade}.sh`, `evals/ab/results/2026-09-22-golden.md` as context |
| Reviewer | Codex CLI 0.155.1 via the shared companion runtime, model `gpt-6-astra`, fresh thread `01a0ca49-d99d-7282-8417-91b5c14ecaaf`, read-only |
| Pinned to | `2177471` (branch `plan/benchmarks`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 13 findings (7 High, 6 Medium): **13 accepted**, all folded into plan v2. Codex knew of no better public dataset than DocsQA-Repo for real questions over markdown documentation with labels. |

## 1. Packet

The reviewer prompt asked for findings ranked by severity as a sceptical future reader of the published page, answers to the plan's open questions, and an "Alternative datasets" section. (The prompt said seven questions; the plan had five; Codex answered the five and flagged the discrepancy rather than inventing two.)

## 2. Findings (Codex, condensed)

| # | Sev | Finding |
|---|---|---|
| F1 | High | The primary corpus cannot be ingested as planned: the walker rejects `.mdx` (three of four DocsQA projects are MDX) and DocsQA also carries normalised and image-derived text, so path↔page mapping alone does not preserve answer evidence. |
| F2 | High | Harness failures can become apparent parity: `ab.sh` suppresses process failures (`error: false` defaults), `grade.sh` turns missing grades into zeros, so zero-vs-zero counts as parity. |
| F3 | High | Conditional (per-question) savings do not establish answer-quality parity: no overall non-inferiority criterion, no floor, no uncertainty bound. |
| F4 | High | Git replay does not preserve the clocks being graded: `file_times()` reads filesystem mtimes and observation time; a checkout replay gives replay-time timestamps, not history. |
| F5 | High | The temporal comparison withholds the obvious competing solution (git log/diff) and starves the baseline of history, so "near zero by construction" would not persuade. |
| F6 | High | Repeated benchmark-driven tuning has no held-out evaluation; DocsQA has no split. |
| F7 | High | G6 (trust) has no publication gate: no grounding or citation check in grading, no human calibration, no no-retrieval control for well-known public docs. |
| F8 | Med | Retrieval scores lack a common unit: labels are pages, mda retrieves sections; aggregation and deduplication unspecified; "sparse labels hurt everyone equally" unjustified. |
| F9 | Med | Headline tokens are `chars/4`; the "median" takes the upper middle value. |
| F10 | Med | Freshness conflates indexing, fallback reads and generation; no polling or timeout protocol; grep missing despite "every table"; G1's save→card not measured directly. |
| F11 | Med | A FreshStack documentation subset changes answerability (nuggets supported by code vanish). |
| F12 | Med | Reproducibility depends on unpinned inputs (`sonnet` alias, competitor defaults, generated indexes) and private-corpus numbers contradict "every number regenerable". |
| F13 | Med | Competitor activation is not established: the harness disables settings and allows only our MCP tools; graphify's hook and query path need proving. |

Answers, condensed: DocsQA is a plausible primary conditional on ingestion and answerability validation, with pooled blinded judgments beside the sparse labels; the git temporal set is sound only as a separately labelled provenance benchmark with explicit clock semantics, equal evidence and a git baseline; no DocsQA project can be assumed large enough for a token gain (a 5× saving at a 1.3K-token floor needs ≈ 6.5K baseline tokens per answer); run qmd both with its full configuration and with reranking off, holding everything else constant; readers distrust parity-only wins, tuning on test questions, unequal evidence, incomplete ingestion and estimated tokens presented as exact.

## 3. Triage

| # | Decision | Where it landed in plan v2 |
|---|---|---|
| F1 | Accept | §2 ingestion gate (`.mdx` ingestion as task B0a; coverage ≥ 95%; image-modality exclusions; answerability check; "source-repository adaptation" label with canonical BM25 numbers alongside) |
| F2 | Accept | rule 0.3; B0 fail-loud validation and schema-checked grades |
| F3 | Accept | rule 0.4 (non-inferiority: mean index ≥ mean baseline − 0.25, paired bootstrap CI, per-question publication, labelled parity subsets) |
| F4 | Accept | §2 temporal protocol: historical mtimes set from commit author dates before each index pass; stored timestamps validated against `git log`; questions limited to stored facts |
| F5 | Accept | rule 0.5; git baseline arm in axis D; wording "convenience and correctness at equal evidence" |
| F6 | Accept | rule 0.2; seeded stratified dev/test split; tuning decisions reference the development split only |
| F7 | Accept | rules 0.6 and 0.8: no-retrieval control in every B table, grounding check in the grader, card grounding pass rate published, 30-answer human calibration |
| F8 | Accept | axis A definition: page-level aggregation and deduplication frozen; pooled blinded column |
| F9 | Accept | rule 0.7: `count_tokens`, "estimated" label otherwise, conventional median |
| F10 | Accept | axis C: three distributions, fixed polling and timeout, grep arm, fallback reads recorded |
| F11 | Accept | §2 FreshStack: subset definition and coverage published first, "derived benchmark" label |
| F12 | Accept | rule 0.1 (freeze everything incl. resolved model ids) and 0.9 (private results labelled and exempt) |
| F13 | Accept | rule 0.5 and §3: per-arm manifests, smoke tests with traces, effective configs published |

## 4. Patterns → rules

- A benchmark page is a claim under adversarial reading: freeze the method in git before the first number, give every arm the same evidence, and count failures as results.
- Never let a harness default hide a failure (`error: false`, missing grade → 0).
- Our clocks are filesystem clocks: any historical replay must set mtimes deliberately and validate them.

## 5. Follow-ups

- Concurrence pass by Codex on plan v2 (same thread) before implementation starts.
- `.mdx` support is a product feature with value beyond the benchmark (Docusaurus, Nextra, Mintlify sites); it gets its own small plan line and tests.
