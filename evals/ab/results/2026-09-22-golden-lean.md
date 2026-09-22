# A/B parity run — golden corpus, lean MCP payload (B0b), 2026-09-22

**Exploratory, estimated measurement.** One run per arm, tokens estimated by the runner (chars/4 for source tokens, the CLI's `usage` for input tokens); the transcript-counted method and the repeated trials of the benchmark plan (rules 0.7 and 0.4) land with B0. Numbers recomputed from the raw `grades.jsonl` of both runs with conventional medians and the same denominator (all 12 questions), after the Codex review of PR #19 found the first version of this table quoted wrong "before" means and a parity-subset median next to an all-question one.

What changed between the two runs, all at once (so the difference cannot be attributed to the payload alone):

1. `mda_search` over MCP: eight hits with every CLI field → five hits, no ranking diagnostics, `snippet` only without a card.
2. The MCP `initialize` instructions and the search-first skill: "k=8" → "5 hits by default", "call mda_open on the section ids you need" → "on the one or two section ids you need".
3. The grader: Sonnet through the Messages API → Sonnet through `claude -p` with a JSON schema (owner decision, plan §0a.3). Same rubric text.
4. Both arms re-run the same day; the corpus, questions, answering model (Sonnet) and preamble are unchanged.

| | before (2026-09-22 morning) | after (lean payload) |
|---|---|---|
| Parity (index score ≥ baseline), per question | 12 of 12 | **11 of 12** (ab07: 4 vs 5) |
| Mean score 0–6, index / baseline | 5.67 / 5.58 | 5.50 / 5.42 |
| Median source tokens read, index (all 12) | 1,304.5 | **762.5** |
| Median source tokens read, baseline (all 12) | 297.5 | 245.5 |
| Median total input tokens, index (all 12) | 32,523 | 48,506.5 |
| Median total input tokens, baseline (all 12) | 37,986.5 | 37,833.5 |
| Mean tool calls, index / baseline | 1.5 / 2.4 | 2.8 / 2.5 |
| Mean wall-clock, index / baseline | 6.9 s / 8.2 s | 8.5 s / 9.7 s |
| Total cost (list-price equivalent), index / baseline | $0.269 / $0.215 | $0.366 / $0.202 |

Reading: the search result itself is now ≈ 400 tokens instead of ≈ 1,400, and the index arm's median source tokens per answer fell from 1,304.5 to 762.5. In the same run Claude opened sections on almost every question (2.8 calls) where it used to answer from the tldr half the time (1.5), so it took more turns and total input tokens, which count the whole context once per turn, went up by half. Whether the extra opens come from the smaller payload (less to answer from) or from the reworded instructions is not separable from this run. On this 450-line corpus the index arm still reads three times the baseline's source tokens; the first run's conclusion stands and the break-even size is a DocsQA (B2) question.

## Per-question table (after)

| id | baseline score | index score | parity | source tokens baseline | source tokens index | input tokens baseline | input tokens index | calls baseline | calls index |
|---|---|---|---|---|---|---|---|---|---|
| ab01 | 6/6 | 6/6 | yes | 234 | 810 | 37402 | 48537 | 2 | 4 |
| ab02 | 4/6 | 4/6 | yes | 232 | 532 | 50785 | 47816 | 3 | 2 |
| ab03 | 6/6 | 6/6 | yes | 226 | 964 | 37443 | 48607 | 2 | 3 |
| ab04 | 6/6 | 6/6 | yes | 158 | 571 | 37289 | 47926 | 2 | 2 |
| ab05 | 6/6 | 6/6 | yes | 130 | 676 | 37199 | 48243 | 2 | 3 |
| ab06 | 5/6 | 6/6 | yes | 669 | 667 | 39180 | 48098 | 2 | 3 |
| ab07 | 5/6 | 4/6 | **no** | 457 | 799 | 38224 | 48542 | 2 | 2 |
| ab08 | 6/6 | 6/6 | yes | 257 | 885 | 50444 | 48972 | 3 | 3 |
| ab09 | 6/6 | 6/6 | yes | 632 | 551 | 25703 | 47583 | 1 | 2 |
| ab10 | 5/6 | 6/6 | yes | 115 | 765 | 37157 | 48618 | 2 | 2 |
| ab11 | 6/6 | 6/6 | yes | 483 | 760 | 52780 | 48476 | 6 | 2 |
| ab12 | 4/6 | 4/6 | yes | 344 | 996 | 38398 | 83143 | 3 | 6 |

Parity gate: 11 of 12 questions at parity (0 with an ungraded run). On parity questions, median source tokens read: baseline 234 vs index 760; median total input tokens: baseline 37443 vs index 48476. Tokens here are the runner's estimate (chars/4 for source tokens; the CLI's usage for input tokens).
