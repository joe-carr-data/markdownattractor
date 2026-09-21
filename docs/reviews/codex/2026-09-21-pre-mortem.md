# Codex review — pre-mortem on the project plan

| | |
|---|---|
| Date | 2026-09-21 |
| Scope | `docs/project-plan.md` (then `idea.md`), full document, pre-mitigation version |
| Reviewer | Codex CLI 0.155.1, model `gpt-6-astra`, reasoning effort medium |
| Pinned to | pre-initial-commit; the reviewed text is the plan as of 2026-09-21 22:53 local, before the edits triaged below |
| Triaged by | Claude (Fable 5.1) with Joe, same day |
| Outcome | 11 findings: 9 accepted, 1 partially accepted, 2 rejected. All accepted items folded into the plan the same day. |

## 1. Packet

**Prompt given to Codex**

> Adversarial pre-mortem review of the project plan at `idea.md`. Read the whole file. Frame: it is 4 months after the project launch and the project failed. It was built to become the reference plugin for handling markdown files in Claude Code sessions but is not getting adoption. What are the points that make it fail? Be concrete, cite sections of the plan, rank by severity.

**Inputs:** the plan only. No code existed yet. Codex read the file with `cat -n` and `sed` in a read-only sandbox.

## 2. Raw findings (Codex, verbatim apart from link formatting)

> I read the entire plan, including the references, Rust stack notes, and SDLC sections. These are ranked failure hypotheses grounded in the plan—not claims that failure has already occurred. I treat competitor and platform descriptions as the plan's assumptions, without independently verifying them.

### F1 — The product solved a feature gap without establishing a compelling adoption reason. — **Critical**

**Evidence:** §2.4 declares, "Nobody does time" and calls temporal provenance "the uncontested lane." §2.5 targets "ADRs, runbooks, notes, specs." §1 defines technical goals G1–G6.

**Failure mechanism:** The plan establishes that its feature combination is different, but never establishes which users encounter sufficiently frequent, expensive markdown problems to install and maintain it. "What changed since Monday?" might be useful without being a reason to adopt another daemon. **Gap:** no customer interviews, design partners, prioritized user segment, or demand-validation gate. Four months later, the implementation works, but most prospective users consider their existing workflow adequate.

### F2 — "Token economy" saved reads while consuming the user's coding allowance. — **Critical**

**Evidence:** G3 targets "≥ 5× fewer source tokens read per answer." §4.1 says summarization "consumes the user's plan usage"; §4.3 starts concurrency at four workers and adds a reducer call. §11 includes cost per 1K documents and query-level A/B costs.

**Failure mechanism:** Users pay the indexing cost before receiving savings, and pay again when documents change. A corpus queried infrequently—or edited frequently—could never recover that cost. Budgets and AIMD constrain consumption; they do not establish positive value or protect interactive work from competition for allowance. **Gap:** no lifecycle break-even criterion combining backfill, updates, retries, and actual query frequency. Users disable the plugin after it consumes allowance they wanted for coding.

### F3 — Installation never became habitual agent usage. — **Critical**

**Evidence:** §2.4 explicitly acknowledges, "Agents ignore MCP tools they aren't reminded of." Its remedy is an **opt-in** `PreToolUse` nudge. §5 relies on a bundled skill prescribing search → cards → source sections.

**Failure mechanism:** The default installation does not establish that Claude consistently takes the intended retrieval path. Users could fund continuous indexing while Claude continues using ordinary tools. Conversely, rigidly following the multi-step path could add overhead to trivial reads. **Gap:** no acceptance metric for appropriate tool use in ordinary sessions, or for savings actually realized after installation. The plugin gets installed, barely participates, and is removed.

### F4 — The index discarded the details users needed to find. — **High**

**Evidence:** §5 runs lexical search over generated card fields and embeds "Cards, not raw chunks"; raw-chunk embedding is deferred. §4.4 budgets section cards at approximately 80–150 rendered tokens. §5 restricts whole-file reads to explicit requests or small files.

**Failure mechanism:** A rare configuration value, exception, error string, or qualifying sentence omitted from the card may become undiscoverable through both retrieval channels. Opening exact source lines only helps after the correct section is found. **Gap:** no raw-source lexical fallback or defined recovery path for missing or low-confidence results. After several "the answer was in the file" incidents, users return to grep and distrust the search-first instruction.

### F5 — The headline temporal promise exceeded the stored evidence. — **High**

**Evidence:** §2.4 promises answers to "what changed since Monday" and "is this ADR still current." §4.4 stores `change_history:[{at,sections_changed}]`; §4.5 combines timestamps and validity status, while `superseded_by` is deferred. §8 places supersession relationships in the later relationships phase.

**Failure mechanism:** Knowing that a section changed does not establish what its previous instructions were. A recent modification also does not establish that an ADR remains authoritative. **Gap:** no explicit historical content snapshots or diffs, and no clear rule for determining validity when documents lack explicit status. Users try the README's flagship questions and receive timestamps or inferred status where they expected a substantiated answer.

### F6 — "Always fresh" failed precisely while users were editing. — **High**

**Evidence:** §2.5 promises "Always fresh." §4.3 introduces debounce, queued model work, budgets, and pause/resume. §4.4 stores line ranges, while §5 exposes `mda_open(section_id)` as returning exact source lines. §6 includes a stale-card command that "should be empty."

**Failure mechanism:** During backlogs, rate limits, pauses, or ordinary processing latency, the source can differ from the indexed version. Inserting text above an unchanged section can also shift its line range without changing that section's content hash. **Gap:** no specified read-time hash check, stale-result fallback, or guarantee that unchanged sections' coordinates are refreshed. Users encounter outdated cards or incorrect excerpts during active work, undermining the reliability claim.

### F7 — Benchmarks validated the optimization instead of the user outcome. — **High**

**Evidence:** §2.4 proposes "choosing corpora large enough that the gains show." §11 uses 30 documents, 60 queries, recorded worker outputs, and A/B measurements of tokens, calls, latency, and cost.

**Failure mechanism:** Fewer source tokens can reflect successful retrieval—or missing relevant evidence. Recall and metadata grounding do not establish final-answer completeness, correctness, or successful task execution. Recorded outputs also cannot demonstrate live operational reliability. **Gap:** no explicit answer-quality parity gate, representative workload selection independent of expected gains, or incumbent search-tool comparison. Published savings look impressive, but users cannot reproduce useful improvements on their own repositories.

### F8 — The distribution plan produced a launch spike without a sustained adoption loop. — **High**

**Evidence:** §8 Phase 4 lists a benchmark page, demo GIF, launch post, and Show HN. §9 thoroughly covers installation and marketplace submission. §1 rules out telemetry; its success metrics remain technical.

**Failure mechanism:** Distribution and positioning are present, but installation mechanics do not explain how qualified users keep discovering the product, succeed with it, and recommend it. **Gap:** no ongoing recruitment channel, activation/retention targets, user-feedback cadence, or process for investigating abandonment. No telemetry is compatible with interviews and voluntary diagnostics, but neither is planned. Four months later, the team knows benchmark performance better than why users stopped using it.

### F9 — The no-key advantage remained dependent on an unresolved external decision. — **High**

**Evidence:** §4.1 requires written confirmation from Anthropic before public launch. §12 offers an API-key backend as the alternative. §4.2 labels the worker call shape "to be confirmed by the spike"; §12 acknowledges CLI flag drift.

**Failure mechanism:** This is an acknowledged dependency, not evidence of a policy violation. But if confirmation is unavailable, the alternative removes a central differentiator and changes onboarding and economics. Even with confirmation, the product depends on evolving CLI behavior for its core work. **Gap:** no separately validated adoption proposition for the API-key fallback. The project could launch in a form materially less attractive than the one its positioning assumes.

### F10 — First use delivered setup and indexing rather than a demonstrated win. — **High**

**Evidence:** G2 promises "start and forget." §9.5 requires a background binary download, `/mda start`, and three setup questions, then highlights showing the first cards. §12 adds lazy embedding downloads and manual bootstrap recovery. §7 targets search latency separately from model-processing latency.

**Failure mechanism:** "First cards available" is not the same as successfully answering the user's first question, especially while coverage is incomplete. Users must make model and budget decisions before seeing value; failed downloads introduce additional troubleshooting. **Gap:** no first-success workflow, incomplete-coverage behavior, or installation-to-useful-answer target. Prospective users abandon onboarding before experiencing the benefit.

### F11 — Scope and maintenance demands crowded out adoption work. — **Medium**

**Evidence:** §3 combines a watcher, parser, worker pool, validator, reducer, database, embeddings, MCP server, and CLI. §6 adds a broad command surface; §9 supports five platform targets. §8 schedules the engine and search phases in roughly four to five weeks, while §17 adds extensive documentation and review obligations.

**Failure mechanism:** This creates many interacting maintenance surfaces before demand is established. The plan names an owner but provides no staffing or post-launch support allocation. A plausible outcome is that installation failures, platform compatibility, and pipeline debugging consume the capacity needed to help early users succeed. The project ships considerable machinery but cannot rapidly resolve the adoption blockers users actually report.

> The top three root causes would be **unvalidated demand for the chosen differentiator, unproven net savings after indexing costs, and failure to turn installation into consistent agent usage**. Together, they explain a technically capable plugin that users neither need urgently nor benefit from reliably enough to keep.

## 3. Triage

| # | Severity | Decision | Reason | Where it landed in the plan |
|---|---|---|---|---|
| F1 | Critical | **Reject** | The owner is the target user (heavy daily Claude Code use on markdown-heavy repos) and knows the pain first-hand; a formal demand gate before the spike would cost weeks for information we already have. Design partners in Phase 4 remain as the external check. | — (design partners: §8 Phase 4) |
| F2 | Critical | **Partial** | Summarization runs on Haiku; its draw on the plan is small next to interactive Opus/Fable use, so a break-even *gate* is not warranted. The cheap part is accepted: make the net visible. | §6 `/mda cost` shows indexing tokens next to saved tokens |
| F3 | Critical | **Accept** | Agreed in full. The plan already called the nudge "the single most effective adoption lever" and then shipped it off. | §2.4 default-on nudge + index hit rate; §6 `/mda nudge`; §8 Phase 2; §12 new risk row |
| F4 | High | **Accept** | Cards are lossy by design; a raw-text lexical layer costs nothing at parse time and removes the failure entirely. | §5 raw-text FTS5 table, `raw=true` retry, Grep fallback in the skill; `pending` hits for unsummarized docs; §12 |
| F5 | High | **Accept** | Narrow the claim rather than build snapshots in v1. | §2.4 claim discipline: v1 promises "what changed since", not "is this still current" |
| F6 | High | **Accept** | Both mechanisms are small and remove a trust-destroying bug class. | §4.3 line-range refresh on every parse; `mda_open` read-time re-hash with `stale: true`; §5 tool table; §12 |
| F7 | High | **Accept** | Token savings without answer parity are meaningless. | §11 parity gate (Sonnet grader), corpora fixed before results, small-corpus results published |
| F8 | High | **Accept** | No telemetry stays; interviews, design partners and a voluntary diagnostics bundle replace it. | §8 Phase 4 adoption loop (activation/retention definitions, 5–8 design partners, abandonment log in `aha.md`); §6 `/mda diagnostics` |
| F9 | High | **Accept** | Resolve the dependency at the start, not the end. | §4.1 confirmation is a Phase 0 exit criterion; API-key-only pitch drafted in parallel; §8 Phase 0; §12 |
| F10 | High | **Accept** | "First cards" is not "first win". | §9.5 one confirmation with defaults, raw search available after parse, example query at end of `start`, install → first useful answer < 10 min |
| F11 | Medium | **Reject** | Owner decision: full v1 command surface and the §17 SDLC apparatus stay. Revisit at the Phase 1 retrospective if velocity suffers. | — |

## 4. Patterns → rules

- **"Opt-in" for an adoption lever is a decision to not have it.** Default-on with an off switch. (candidate for `.claude/rules/`)
- **Every LLM-derived layer needs a deterministic safety net underneath it** (raw FTS under cards; read-time hash under stored line ranges). Added to `CLAUDE.md` golden rules 4 and 5.
- **Never state a benchmark gain without an answer-quality parity check.** Goes into `docs/benchmarks.md` when created.

## 5. Follow-ups

- Re-run this review at the end of Phase 1 against the implementation, same prompt, pinned to the release SHA.
- F2 gets re-examined if design partners report plan-usage complaints.
