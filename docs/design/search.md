# Design — search

As built in Phase 1 (2026-09-22). Vectors are Phase 2 and slot in as a third ranked list.

## Query path

```
query ─► fts_escape ─► cards_fts (bm25) ─┐
                    └► sections_raw_fts ─┤─► RRF ─► recency factor ─► filters ─► top-k
```

1. **Escape** (`store::fts_escape`): the user text is split on whitespace, each term is double-quoted with inner quotes doubled, and terms are joined with spaces (FTS5 implicit AND). No FTS5 syntax from the user ever reaches `MATCH`. A trailing `*` keeps prefix matching.
2. **Two BM25 lists**: `cards_fts` over `heading_path`, `tldr`, `summary`, `keywords`, `questions_answered`, `entities` with weights 3/3/1/1/2/1, and `sections_raw_fts` over `heading_path` (3) and the raw section text (1). Both use `unicode61 remove_diacritics 2`. `search --raw` skips the cards list.
3. **Fusion** (`search::run`): reciprocal rank fusion with k = 60. A section in both lists gets both contributions and is marked `matched: both`.
4. **Recency** (`search::recency_factor`): score × (0.5 + 0.5·2^(−age/half-life)), half-life 30 days by default. Old but relevant sections are never buried below half weight; `0` disables it.
5. **Filters**: `--since`/`--until` on the section's `updated_at`, `--in <prefix>` on the relative path. Applied after fusion on the stored rows.
6. **OR fallback**: if the AND query returns nothing, the same terms are retried joined with `OR` and hits are flagged `via_or_fallback`; the CLI prints "(showing partial matches)".

## What a hit carries

`section_id`, `rel_path`, `heading_path`, `line_start`/`line_end`, `token_estimate`, `tldr` (when a card exists), `snippet` (first ~200 chars of the body, whitespace-collapsed), `score`, `matched` (cards / raw / both), `pending` (no card yet), `updated_at`. The CLI prints the exact `mda open <section_id>` to run next.

## Reading source: `mda open`

`pipeline::Engine::open_section` re-parses the file at read time and compares the section's hash with the stored one. If they match, the stored line range is returned. If not, the file is re-indexed on the spot, the section is re-found (by hash, then heading path, then index), and the *current* lines are returned with `stale: true`. Claude never receives an excerpt that does not match what is on disk.

## Time semantics

- `updated_at` on a section is the file's mtime when that content was last seen new for that document.
- Filters and the recency prior use `updated_at`. Content-time (dates *inside* the text) is stored on cards as `mentioned_dates` and is not yet a ranking signal.

## Tuning knobs

| Knob | Where | Default |
|---|---|---|
| `k` | `SearchOptions.k`, `-k` | 8 |
| recency half-life | `SearchOptions.recency_half_life_days` | 30 |
| OR fallback | `SearchOptions.or_fallback` | on |
| fetch depth per list | `k × 4`, min 16 | |
| bm25 weights | `store/mod.rs` | see above |

`search::explain` returns the two raw ranked lists for a query; `mda explain` (planned) will print them with the fused score.
