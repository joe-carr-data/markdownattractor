# A/B parity run — golden corpus, lean MCP payload (B0b), 2026-09-22

Same protocol as `2026-09-22-golden.md` (12 questions, one run per arm, Sonnet answering through `claude -p`, Sonnet grading through `claude -p` with a JSON schema, `scripts/eval/ab.sh` + `scripts/eval/grade.sh`), run after the `mda_search` MCP view was cut to five hits without ranking diagnostics or snippets-when-carded. The corpus, the questions and the baseline arm are unchanged; the baseline was re-run the same day so both arms share the session. Tokens are the runner's estimate (chars/4 for source tokens, the CLI's `usage` for input tokens); the transcript-counted method of plan rule 0.7 lands with B0.

| | before (eight hits, full fields) | after (five hits, lean view) |
|---|---|---|
| Parity (index score ≥ baseline) | 12 of 12 | **11 of 12** (ab07: 4 vs 5) |
| Mean score 0–6, index / baseline | 5.42 / 5.42 | 5.50 / 5.42 |
| Median source tokens read, index | 1,305 | **760** |
| Median source tokens read, baseline | 341 | 234 |
| Median total input tokens, index | 32,548 | 48,476 |
| Mean tool calls, index | 1.6 | 2.8 |
| Total cost (list-price equivalent), index | $0.269 | $0.366 |

Reading: the payload change did what it was meant to (a search result is now ≈ 400 tokens instead of ≈ 1,400, and the median source tokens per answer fell 42%), but on this 450-line corpus Claude answered from the tldr alone in half the questions before and now opens one or two sections almost every time (2.8 calls), so total input tokens, which count the whole context once per turn, went up. Source tokens are still three times the baseline's here: the golden corpus is too small for the index to win, as the first run said. The lever that remains is the per-turn context cost (fewer turns, or a smaller system prompt), not the hit payload.

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
