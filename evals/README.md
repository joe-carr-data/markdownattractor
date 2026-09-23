# Evals

`golden/` is the retrieval golden set: a small, fictional but realistic platform-team knowledge base (`docs/`, 30 documents: ADRs, runbooks, changelogs, meeting notes, specs, onboarding, postmortems) and `queries.jsonl` (60 queries, each with the sections that answer it; half are literal, half paraphrased, a third temporal). `cards.json`, when present, holds recorded section cards keyed by section hash so the hybrid run needs no model at eval time.

```
mda eval --golden evals/golden -k 5            # lexical run (+ hybrid when cards.json exists)
mda eval --golden evals/golden --record        # summarize the corpus with the configured backend, write cards.json, then evaluate
```

Numbers live in `docs/benchmarks.md`. The corpus is fixed: do not edit documents to make a query pass; add a query instead and report it.

`spike/` holds the Phase 0 measurement scripts.

## DocsQA-Repo adapter (`mda eval --dataset docsqa`)

The benchmark plan's primary dataset (`PowderXu/docsqa-data`, schema v3: 467 real community questions over four documentation repositories pinned to commits, sparse page-level labels). We index the repository *source* at the pinned commit, not the dataset's rendered text, so every label (`qrel_ids` → `corpus.jsonl` → `repository_source_path`) is looked up in our store by relative path. The adapter reports coverage before any number (plan §2 F1), assigns every question of a project to a seeded split (rule 0.2: 30% dev, 55% test, 15% sealed holdout; seed 20260922; `blake3(seed ‖ question_id)` order, stratified per project), and scores at **page** granularity: a section list is deduplicated by path in rank order, then success@5, MRR@5 and nDCG@10 (binary gains) against the labelled pages.

```
git clone https://github.com/PowderXu/docsqa-data && gunzip -k docsqa-data/data/corpus.jsonl.gz
# one sparse checkout per project at the pinned commit (sources.json), e.g. prisma/web@c4ac0e9 apps/docs/content/docs
mda index --no-summarize <checkout>                     # raw index; add --cards <file> below for the carded runs
mda eval --dataset docsqa --data docsqa-data --project prisma --root <checkout> --split dev --out evals/results/docsqa/prisma
```

Rows: `lexical (raw only)`; with cards attached (`--cards`, recorded per corpus and `mda` version under `evals/results/docsqa/`) also `lexical (cards + raw)` and, when the embedding model is on disk, `hybrid`. `--out` writes `coverage.json`, `split.json` (every question's split, so the sealed holdout is visible and never touched) and `results.json` (per-question ranks and the top pages). Excluded and stated: questions whose reference evidence is image-derived text (`image_text_evidence_used`), and questions with a label we cannot map or did not index. `requires_multimodal_judgment` is counted for the answer-quality axis. `--split holdout` is refused by convention until 1.0 (it prints a warning; rule 0.2).

## A/B parity protocol (`ab/`)

Plan §11: the same question through headless `claude -p`, once **without** the index (Claude has `Read`, `Grep`, `Glob` over the corpus) and once **with** it (the same tools plus the `mda` MCP server and the search-first rules), N runs each; every answer is graded against a reference by Sonnet (correctness 0–3, completeness 0–3); token savings are only reported for questions where the with-index score is at least the baseline's.

```
mda index <corpus>                                    # cards + vectors first; the daemon may run
scripts/eval/ab.sh <corpus> evals/ab/questions.jsonl <out> [runs=1] [model=sonnet]
scripts/eval/grade.sh evals/ab/questions.jsonl <out>  # Sonnet through `claude -p` (no API key); writes <out>/parity.md
```

`ab/questions.jsonl` holds one question per line with a reference answer written from the golden corpus and the sections it comes from. Two numbers per run: **source tokens** (the size of everything the tools returned: file contents in the baseline, cards and sections with the index; the quantity G3 is about) and **total input tokens** (which also count the system prompt and the tool schemas, so the MCP arm starts with a fixed overhead). Every model call in the protocol, the two arms and the grader, goes through the owner's own Claude Code login (`claude -p`), never an API key (benchmark plan §0a.3); that is the owner's ordinary use of their own account and is not how the product summarizes anything for users (ADR-0002). A run with no grade is shown as **ungraded** and never counted as parity.

Small corpora are reported even when the index loses. On the golden corpus (32 files, 450 lines) it does: one `Grep` and one `Read` cost fewer source tokens than eight hits. See `docs/benchmarks.md`.
