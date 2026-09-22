# Evals

`golden/` is the retrieval golden set: a small, fictional but realistic platform-team knowledge base (`docs/`, 30 documents: ADRs, runbooks, changelogs, meeting notes, specs, onboarding, postmortems) and `queries.jsonl` (60 queries, each with the sections that answer it; half are literal, half paraphrased, a third temporal). `cards.json`, when present, holds recorded section cards keyed by section hash so the hybrid run needs no model at eval time.

```
mda eval --golden evals/golden -k 5            # lexical run (+ hybrid when cards.json exists)
mda eval --golden evals/golden --record        # summarize the corpus with the configured backend, write cards.json, then evaluate
```

Numbers live in `docs/benchmarks.md`. The corpus is fixed: do not edit documents to make a query pass; add a query instead and report it.

`spike/` holds the Phase 0 measurement scripts.
