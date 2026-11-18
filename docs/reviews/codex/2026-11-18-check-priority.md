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

## Second pass (the delta `19abf6d..5b014b8`)

Reviewer: Codex (as above; fresh thread `01a1ec7b-eca4-7d91-9aba-7e543062be02`, read-only, scenarios traced through the code, no test run). **Verdict:** "merge after fixes." Seven findings (2 high, 5 medium). Each is fixed, with the reviewer's scenario as a test.

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | High | After a rename of capitals only (`db.user` to `db.User`), a candidate already using the new spelling passed every rule: the word sets were compared without capitals and came out empty. | **Fixed.** Words are compared as written; a word also counts whatever its capitals only when the change did not merely change capitals. `Call db.User` is dropped, `Call db.user` is a finding. |
| 2 | High | A phrase standing twice in the other section was taken at its first place, which could be the line that says the opposite. | **Fixed.** The first place is taken only when every place stands on identical lines; otherwise the quote is refused. |
| 3 | Medium | On the change's side, the same line text changed in one place and unchanged in another both "touched a changed line"; a multi-line quote was judged by its first line only. | **Fixed.** Exactly one place must touch a changed line, over all the lines it spans; two such places are refused. The changed lines carry no positions (`SectionChange` holds texts), so two identical lines cannot be told apart: refusing is the honest answer. |
| 4 | Medium | "A part of the after quote" was a substring test: `30 seconds` inside `130 seconds`. | **Fixed.** A run of whole words. |
| 5 | Medium | Punctuation alone (`Route B to A!`) made a candidate differ from the after quote. | **Fixed** by the same run-of-words comparison. |
| 6 | Medium | A change whose turn fell on a repeat lost the turn. | **Fixed.** The repeat is set aside and the change goes on to its next pair in the same round. |
| 7 | Medium | A failed call counted as a judgment in `changes_without_judgment`. | **Fixed.** Only pairs the model answered count. |
