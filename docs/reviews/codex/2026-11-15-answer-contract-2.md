# Codex review — answer contract iteration 2 and the citation extractor, PR #74 (2026-11-15)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: `git diff main...feat/answer-contract-2`. Codex ran the old and new extractor against the raw historical answers and re-rendered the T6 report; it did not run the smoke suite.

**Verdict:** no High findings. "Fix the measurement reading and the rule 5 wording before merging. Regeneration of committed tables is safe."

## Findings and triage (10 items: 4 Medium, 5 Low, 1 without a finding)

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | Low | Regeneration is unaffected (T6 report re-rendered to the published sha256; all 2,214 recorded T2/T6 citation tokens extract identically), but the new extractor changes extraction for 59 of 1,002 historical T2 answers (0 of T6's 492), so a *regrade* could differ. | **Accepted.** Runbook §7.7 says so, with the counts. |
| 2 | Low | The citation grammar is deliberately limited and undocumented (`a.md:12:34`, `a.md:L5-L7` are not citations; line existence unchecked). No regex or placement defect. | **Accepted.** Documented in the code comment and the runbook; not extended. |
| 3 | Low | The smoke case leaves interactions untested. | **Accepted in part.** Nine extraction cases added (odd names, link destinations and titles, anchors, punctuation, colon-bearing names, the rejected forms). Not added: extraction-then-resolution cases on the resolver fixture; deferred to the counted run's harness change. |
| 4 | Medium | "Leave out … caveats" invites dropping caveats that matter (a conflict between documents, a safety condition, a limitation). | **Accepted.** Rule 5 takes Codex's wording in both places: omit background and related facts; keep required disclosures and caveats needed for accuracy or safe action, including relevant conflicts in opened sources and what remains unchecked or unanswered. This is not the wording iteration 2 measured; the counted run measures it. |
| 5 | Medium | "Set tighter than the grader's own noise" is not established: the regrade shows variability, not a noise threshold for mda, whose own regrade did not move. | **Accepted.** The plan now says both iterations miss the golden correctness condition, reports all four arms' regrade, and does not loosen the gate. |
| 6 | Medium | The explanation of lost points was stronger than the evidence. | **Accepted.** Labelled as an author's audit of the grader's notes; the truth of the extra details and the `mda_open`/`mda open` equivalence are stated as unverified readings. Blind adjudication is part of the counted run. |
| 7 | Low | "Restores docs correctness" and "about 10 % more tokens" overstate. | **Accepted.** Reworded: within the tolerance on docs, below it on golden, no overall quality improvement established; +12.1 % and +6.2 % median source tokens. |
| 8 | Medium | An mda-only counted rerun cannot isolate the contract's effect. | **Accepted.** The plan specifies the counted run before it is run: all four arms plus an old-contract mda control under one freeze, panel grading blind and interleaved on both mda arms, declared treatment of supported extra facts, index metadata and tool names, paired per-question analysis, gates fixed now, the reuse of development questions disclosed. Held-out questions: not added (no writer run planned in Phase A); disclosed instead. |
| 9 | Low | "Never judged" wrongly covered correctness grading. | **Accepted.** "Marked ungrounded without a model-based grounding judgment; correctness was still graded", in the plan and the runbook. |
| 10 | — | No further merge-blocking finding. | — |
