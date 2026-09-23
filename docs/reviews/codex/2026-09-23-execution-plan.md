# Codex review — benchmark execution plan (pre-mortem, PR #21)

| | |
|---|---|
| Date | 2026-09-23 |
| Scope | `docs/plans/2026-09-benchmark-execution.md` v1, with the strategy plan v3.2, its review, `docs/benchmarks.md`, `evals/README.md`, `scripts/eval/{ab,grade}.sh`, `crates/mda-core/src/eval/docsqa.rs` and the adapter review as context |
| Reviewer | Codex CLI 0.155.1 via the shared companion runtime, model `gpt-6-astra` (reports itself as GPT-6), thread `01a0ccb1-a678-7041-9dad-1f08485d767f`, read-only |
| Pinned to | `e023d65` (branch `plan/benchmark-execution`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 13 findings (10 High, 3 Medium): **13 accepted**, folded into plan v2 on the same branch. Second pass on v2 requested. |

## 1. Packet

The pre-mortem frame ("it is four months later and the effort failed to beat the competitors credibly or produced numbers nobody trusted: why, from this plan?") with six questions: contradictions with v3.2; fairness and completeness of the competitor profiles; safety of the tuning loop; looseness of the must-not-lose lines and acceptance criteria; realism of the schedule; the three additions that would most raise trust.

## 2. Findings (Codex, condensed)

| # | Sev | Finding |
|---|---|---|
| F1 | High | T1 publishes (S7) before the smoke-trace infrastructure and the harness review (S8); the accepted strategy review requires harness verification before the first published table. |
| F2 | High | The paired-bootstrap CI and the rule-0.4 gate are promised but no task implements the analysis; the grader only computes medians and per-question parity. |
| F3 | High | Panel adjudication adds a ">10% of answers" threshold the strategy does not have, and the 100-card metadata audit is not scheduled. |
| F4 | High | T1 drops axis A's second column (blinded pooled judgments of 100 query-page pairs per project). |
| F5 | High | Competitor page ranking is not specified to mda's standard (fetch until ten distinct pages, truncation recorded); graphify switches between MCP and CLI. |
| F6 | High | Installation and activation are not reproducible: no pinned recipes, `.mdx` coverage, isolated collections, resolved model revisions, configuration diffs; one smoke question instead of three probes; graphify's hook not proven. |
| F7 | High | Timing boundaries favour mda: in-process adapter timing next to cold competitor CLIs; rule 0.9's server lifecycle not implemented; `claude -p` starts every MCP server per question. |
| F8 | High | The tuning loop has no aggregate objective, candidate list, selection or tie rule, and freezes before a T2 development stage exists. |
| F9 | High | The headline promises "time questions the others cannot" while every arm gets `.git`; the strategy limits the claim to correctness and effort at equal evidence. |
| F10 | High | S9 packs 1,200 answers, grading, calibration and publication into one session; the runner writes its manifest after the loop and refuses to resume. |
| F11 | Med | T4 compares different endpoints (our save → answer vs their rebuild time) and demands card latency from arms without cards. |
| F12 | Med | Acceptance thresholds allow narrower wins than the pitch: −0.05 against the no-rerank ablation instead of qmd full; three of four projects; "within its CI" undefined. |
| F13 | Med | The freeze and artifact lifecycle cannot support the reproducibility exit: first freeze before competitor builds; exploratory numbers already exist; no reconstruction check. |

Answers, condensed: (1) rules 0.1/0.2 need a freeze lifecycle and an auditable selection rule; 0.3 a resumable runner; 0.4 an implementation; 0.5 three probes; 0.8 the audit and the verbatim adjudication; 0.9 a server lifecycle; axis A the pooled column and qmd full as target; axis E per-1K-section normalisation; the panel routes named. (2) Fairness not yet demonstrated; both competitors are not installed; needs pinned recipes, coverage, isolated collections, embeddings completed, build provenance, hook activation, deterministic page mapping, equal timing boundaries. (3) Log every candidate, hashes, denominators, latency, failures, elapsed budget, selection reason; T2 needs a development stage or an explicit exclusion; test losses are results. (4) qmd full is the primary target; small dev samples (Supabase 12) need intervals; T2's three-of-four cannot support an unqualified claim; T5 needs a paired criterion. (5) Reorder: verified inputs and probes → common scorer and pooled judgments → bounded tuning → reviewed T1 → T2 pilot with verified grading and statistics → frozen T2; the adapter must export reference answers for T2. (6) A machine-checked prepublication manifest; a frozen judging and analysis specification; a complete experiment ledger.

## 3. Triage

| # | Decision | Where it landed in v2 |
|---|---|---|
| F1 | Accept | Milestones M1–M4: preflight, probes and the scorer Codex pass precede T1; publication depends on verification, not on a session number (§5). |
| F2 | Accept | §2.5: `mda eval --analysis` in `mda_core::eval` with the aggregation, paired bootstrap, missing-run rule, grounding denominator and per-comparator gates; task in M5. |
| F3 | Accept | §2.4: 100-card audit per corpus and 30-answer calibration by the panel, individual scores kept, rule 0.8 adjudication verbatim (no extra threshold); panel routes named. |
| F4 | Accept | §2.3: pooled blinded judgments, seeded sample of 100 pairs per project, rubric frozen, second column published. |
| F5 | Accept | §2.2: `--arm-output` scorer with the same page rule; drivers fetch until ten distinct pages, normalise paths, record truncation; graphify scored through MCP `query_graph` in T1 and T2 with a frozen node → file mapping. |
| F6 | Accept | §2.1 pinned install recipes; §2.0 preflight with coverage per arm, resolved models, configuration diffs and three probes per arm with traces (graphify's hook included). |
| F7 | Accept | §2.7: latency through each tool's MCP server with the same client, cold first query separate; T2's per-question cold start stated for every arm; hardware and build profile in `FROZEN.md`. |
| F8 | Accept | §3: aggregate objective, guardrail, eight pre-declared candidates, selection and tie rule, ledger, stop rule; T2 gets a development pilot (M5), not tuning. |
| F9 | Accept | §1.2, §4 T5, §7: the claim is correctness and effort at equal evidence; every arm may read history; replay labelled simulated. |
| F10 | Accept | §2.6: manifest before the loop, resumable rows, bounded concurrency, retry accounting; M5 pilot measures throughput; M6 runs detached; no dates until pilots. |
| F11 | Accept | §4 T4: save → correct grounded answer is the comparison endpoint for every arm; save → card only for mda (n/a elsewhere); one update trigger per edit; timeouts, fallback reads and raw-search latency published. |
| F12 | Accept | §1.1 and §4: qmd full is the primary comparison and target; publication completion separated from product targets; T2 claims limited to passing pairs and projects; T5 paired bootstrap criterion. |
| F13 | Accept | §2.0 freeze lifecycle (exploratory / development / final), `FROZEN.md` validated on every run, reconstruction check in the preflight; §0 labels today's numbers exploratory. |

## 4. Patterns → rules

- **A published table has a preflight that a machine checks**: frozen inputs, coverage per arm, activation traces, reconstruction. "We ran it carefully" is not evidence.
- **Comparisons are specified to the same standard on both sides**: page rule, timing boundary, interface, evidence given.
- **A headline claim is worded as the measurement**: "correctly with less effort at equal evidence", never "the others cannot".

## 5. Follow-ups

- Second Codex pass on v2 before merge; a third only if it finds new High findings.
- The runbook (`evals/benchmark_it_with_claude.md`) is the preflight's human-readable twin and must stay in step with `preflight.sh`.
