# ADR 0007: Recency prior off by default

Date: 2026-11-10
Status: Accepted

## Context
Search multiplied every score by a recency factor, 0.5 + 0.5·2^(−age/30 days), on the section's `updated_at`. Every benchmark corpus was a fresh checkout or copy, so all files had the same age and the prior had no effect on any published number; the published retrieval tables (T1, DocsQA) ran with it off anyway (`eval/docsqa.rs`). Measured on the golden set's 60 labelled queries with realistic file ages (`scripts/demo/make-corpus.py`), same cards, same binary (`docs/plans/2026-11-git-times.md`):

| Half-life | lexical raw success@5 / MRR | hybrid success@5 / MRR |
|---|---|---|
| off, or flat ages | 0.883 / 0.747 | 0.983 / 0.853 |
| 30 days, realistic ages | 0.550 / 0.379 | 0.900 / 0.629 |
| 365 days, realistic ages | 0.700 / 0.518 | 0.950 / 0.688 |

None of the 16 time-flagged queries gained. Git-derived times (planned) would give every cloned repository realistic ages and so switch the loss on for most users.

## Decision
We turn the recency prior off by default. A root can set `search_recency_half_life_days` in `config.toml` (clamped to 0–36,500; non-finite or negative values mean off) to restore a prior. Time questions are served by `--since`/`--until` filters and the timeline, not by ranking.

## Consequences
- Positive: ranking no longer depends on file age; the product default equals the configuration of the published retrieval tables.
- Negative: a corpus where "newer is more relevant" holds loses the boost unless the user sets a half-life. Fresh benchmark runs with a new binary (the owed reruns) rank without the prior; the published tables regenerate from committed artifacts and are unaffected.
- Follow-ups: consider content dates (`mentioned_dates`) or query-dependent recency for time questions.

## Alternatives considered
- A longer half-life (365 days) — still costs up to 18 points of success@5 on realistic ages.
- Recency only for queries that look temporal — no labelled evidence yet that it helps; revisit with a temporal query set.
