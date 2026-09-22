# Design — search

As built in Phase 1 and extended in Phase 2 (2026-09-22, ADR-0004): two BM25 lists plus card vectors, fused.

## Query path

```
query ─► fts_escape ─► cards_fts (bm25) ─┐
                    ├► sections_raw_fts ─┤─► RRF ─► recency factor ─► filters ─► top-k
        embed(query) ─► VectorIndex (cosine) ─┘   (only when the model is on disk)
```

1. **Escape** (`store::fts_escape`): the user text is split on whitespace, each term is double-quoted with inner quotes doubled, and terms are joined with spaces (FTS5 implicit AND). No FTS5 syntax from the user ever reaches `MATCH`. A trailing `*` keeps prefix matching.
2. **Two BM25 lists**: `cards_fts` over `heading_path`, `tldr`, `summary`, `keywords`, `questions_answered`, `entities` with weights 3/3/1/1/2/1, and `sections_raw_fts` over `heading_path` (3) and the raw section text (1). Both use `unicode61 remove_diacritics 2`. `search --raw` skips the cards list.
3. **Vectors** (`search::vector_list`, ADR-0004): when embeddings are on, an embedder is given and its model is already on disk (`Embedder::ready`; a query never triggers a download), the query is embedded with `bge-small-en-v1.5` (quantised, 384 dimensions) and every card vector of that model is scored by dot product (`VectorIndex::top_k`, brute force from a contiguous `f32` buffer; ~15 MB and single-digit milliseconds at 10K sections). Vectors are keyed by section hash like the cards (so a moved or duplicated section costs nothing; the trade-off is that a hash shared by several documents carries one document's title and heading context, the same ambiguity the card itself has), made from `title + heading_path + tldr + summary + keywords + questions_answered` right after a card is attached (`Engine::embed_pending`, batches of 32), and rebuilt by `mda rebuild --embeddings`.
4. **Fusion** (`search::fuse`): reciprocal rank fusion with k = 60 over the three lists. A section in both lexical lists is `matched: both`; one found only by its vector is `matched: vector`; every hit carries `vector: bool` and `vector_score` (cosine).
5. **Recency** (`search::recency_factor`): score × (0.5 + 0.5·2^(−age/half-life)), half-life 30 days by default. Old but relevant sections are never buried below half weight; `0` disables it.
6. **Filters**: `--since`/`--until` on the section's `updated_at`, `--in <prefix>` on the relative path. Applied after fusion on the stored rows.
7. **OR fallback**: if the AND query returns nothing lexically, the same terms are retried joined with `OR` and hits are flagged `via_or_fallback`; the CLI prints "(showing partial matches)". The vector list is computed once and joins either way.

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

`mda explain <query>` prints the three lists the search used (cards, raw, vector, with the OR form noted when it applied) and the fused, recency-weighted result. `SearchOptions.vectors = false` or `--raw` skips the vector list.

## Measured (golden set, 32 docs / 117 sections / 60 queries, release build, Apple M3)

| Run | success@5 | MRR@5 | mean query |
|---|---|---|---|
| lexical, raw text only (no cards) | 0.883 | 0.747 | ≈ 2 ms |
| lexical, cards + raw | 0.900 | 0.777 | ≈ 3 ms |
| **hybrid, cards + raw + vectors** | **0.983** | **0.853** | see `docs/benchmarks.md` |

The seven lexical misses were all paraphrases ("undo the last release", "mass logout incident"); vectors recover six of them. Numbers and the one remaining miss are in `docs/benchmarks.md`.
