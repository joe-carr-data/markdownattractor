# Codex review — which pairs `mda check` judges first (2026-11-18, PR #105)

Reviewer: Codex (requested `gpt-6.2-astra`; it reports "GPT-6 Codex", exact variant unavailable; CLI 0.167.0; fresh thread `01a1ec71-ec59-72d3-b16d-13f6a8e2591e`, read-only, source review, no test run, through the `codex:codex-rescue` companion; no nested `codex exec`). Pinned to `19abf6d`. Triaged by Claude.

**Verdict:** "merge after fixes." Five findings (1 high, 3 medium, 1 low). Found sound: the stable sort, the borrow in the queue filter, `weight`'s arithmetic, the raised output cap.

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | High | Removing a pair that repeats an earlier one could discard the only pair with a finding (two changes replacing the same wording with different new wording). | **Fixed.** A repeat is deferred behind every distinct pair, not removed: it is judged when the cap leaves room. Regression: the first pair `related`, the repeat a finding. |
| 2 | Medium | A change whose only pair was removed vanished from the coverage record. | **Fixed.** No pair is removed any more, so every pair has an outcome; `changes_without_judgment` counts changes that have candidates and got no answer, and the command prints it. |
| 3 | Medium | A global sort by strength let five changes take all 40 judgments. | **Fixed.** Rounds: every change gets one pair before any gets a second; a change's own pairs go strongest first, and within a round the changes whose next pair is strongest go first. The module docs say so. |
| 4 | Medium | Shared terms that are only new, or only context, weighed as much as replaced wording. | **Fixed.** Anything but replaced wording weighs a quarter. Not separately tested against an addition at a cap of one; the development half is the test of the weights. |
| 5 | Low | The new test accepted either of two tied pairs. | **Fixed.** It asserts the first in file order, and that the repeat comes last. |
