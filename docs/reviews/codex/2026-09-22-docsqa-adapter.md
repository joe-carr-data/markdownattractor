# Codex review — DocsQA-Repo adapter, coverage report, seeded split (PR #20, benchmark plan B0/B2)

| | |
|---|---|
| Date | 2026-09-22 |
| Scope | `crates/mda-core/src/eval/{mod,docsqa}.rs`, `run_docsqa` and `Args` in `crates/mda-cli/src/commands/eval.rs`, the DocsQA fixture and test in `crates/mda-cli/tests/engine_cli.rs`, `evals/README.md`, the DocsQA section of `docs/benchmarks.md`, `evals/results/docsqa/prisma/*`; context plan §0, §0a, §1, §2, §5 |
| Reviewer | Codex CLI 0.155.1 via the shared companion runtime, model `gpt-6-astra`, thread `01a0ca94-9a86-7283-ae27-fbd805660878`, read-only |
| Pinned to | `edb518b` (branch `feat/eval-docsqa`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 6 findings (2 High, 3 Medium, 1 Low): **6 accepted and fixed** on the same branch before merge; the three suggested test areas got tests. |

## 1. Packet

The `/codex-review` reviewer prompt with the north star, the rust rules, the plan's axis-A definition, the ingestion gate F1 (iv)–(v) and rule 0.2, the diff and the listed files. Five questions: (1) are the page-level metrics as the plan defines them, with off-by-one, empty or duplicate cases; (2) is the split deterministic, stratified and immune to order or renaming, and can the holdout leak; (3) does eligibility exclude exactly what F1 (iv)–(v) says; (4) does anything write outside `--out`, follow symlinks or leak paths; (5) missing tests.

## 2. Findings (Codex, condensed)

| # | Sev | Finding | Codex fix |
|---|---|---|---|
| F1 | High | `create_dir_all` and `std::fs::write` follow symlinks: an `out/results.json` symlink redirects the report into a checkout source file or outside `--out`. | Validate containment; refuse symlinks. |
| F2 | High | The "ingestion gate passed" claim only checks that labelled paths are in the store; plan §2 F1 (v) asks that the reference evidence be present in the indexed text. | A recorded per-question evidence-presence check; withhold the full claim until it passes. |
| F3 | Med | Search truncates to `fetch` sections before page deduplication: thirty sections of one page can hide the relevant page at page rank 2. | Fetch until ten distinct pages or exhaustion. |
| F4 | Med | `--split all` evaluates the holdout while the warning covers only explicit `holdout`. | Keep holdout out of ordinary runs; require an explicit opening option. |
| F5 | Med | Duplicate labels deflate nDCG: ranked `[A]` against `[A, A]` scores 0.61, not 1.0. | Deduplicate; ideal DCG over distinct pages. |
| F6 | Low | Reports serialise the canonicalised `root` and `data` (home paths). | Sanitise before serialising. |

Answers, condensed: (1) formulas right, deduplication first-occurrence, F3/F5 the caveats; (2) deterministic and stratified, integer arithmetic; ids are the hash input so renaming moves questions; holdout leaks through F4; no frozen split is re-validated on later runs; (3) exclusions match (iv); (v) unimplemented (F2); duplicate question rows would count twice; (4) the store is modified (cards, embeddings); F1; committed results were sanitised by hand; (5) page-ranking past the fetch depth, holdout isolation, output containment.

## 3. Triage

| # | Decision | What was done | Where |
|---|---|---|---|
| F1 | **Accept** | `report_dir` creates and canonicalises `--out` and refuses a directory inside the checkout; `write_report` refuses to write through anything that is not a plain file (symlinks included). Test plants a symlink from `out/results.json` to a source file and asserts the file is untouched. | `eval.rs`; `eval_docsqa_keeps_the_holdout_sealed_and_refuses_bad_output_targets` |
| F2 | **Accept** | `Question.anchors` from the dataset's `anchor_resolution` (page, canonical heading or page-level); `coverage` reports `anchors`, `anchors_found`, `questions_with_missing_anchor` and the ids, comparing headings after `normalize_heading` (case, backticks, Liquid tags, whitespace; the page title counts). Real corpora: Tailwind 96/96, Prisma 176/176, Supabase 47/48, GitHub Docs 183/222 (35 of the 39 misses are rendered Liquid includes and variants). The page now states F1 (v) with its numbers and the limitation instead of the bare "passed". | `docsqa.rs`, `benchmarks.md`; fixture anchors with a backticked heading, a page anchor, a title anchor and a missing one |
| F3 | **Accept** | `evaluate` doubles the fetch until ten distinct pages are in hand, the results run out, or `FETCH_CAP` (2000); `fetched` and `truncated` are recorded per question. Test: a 40-section page that hogs the list with `--fetch 4`. | `docsqa.rs`; `eval_docsqa_fetches_past_a_page_that_hogs_the_list` |
| F4 | **Accept** | `--split holdout` is refused and `--split all` skips holdout questions unless `--open-holdout` is passed (which warns). | `eval.rs`, `RunOptions.include_holdout`; test asserts no holdout row without the flag |
| F5 | **Accept** | Labels deduplicated at load; ideal DCG over distinct relevant pages. | `docsqa.rs`, `mod.rs` + unit test |
| F6 | **Accept** | `portable()` replaces the home directory with `~` in `data` and `root` before serialising. | `eval.rs`; test asserts no `/Users/` in `root` |

Not changed, with reason: id renaming moving questions across splits is by design (the id is the dataset's stable key; a renamed dataset is a new dataset and gets a new `FROZEN.md`); re-validating a frozen split on later runs is the `FROZEN.md` writer's job (B0 leftover).

## 4. Patterns → rules

- **A gate claim names its check**: "passed" is a number per criterion of the plan, with the misses classified, never a summary word.
- **Rank at the unit you publish**: when the metric is per page and the search is per section, fetch until the page list is complete before cutting.
- **Report files follow the state-file rules**: containment and no symlink following, even when the user chose the path.

## 5. Follow-ups

- GitHub Docs' rendered includes: consider indexing `data/reusables` and `data/variables` as support roots (the dataset's `sources.json` lists them) so an include's heading can be found; the page would still not carry it, so the honest fix is the disclosed number.
- `FROZEN.md` writer (B0) re-validates the split against the frozen file on every run.
- Partial-card bias in the fusion (found on Tailwind at 14% carded): a product question for the plan, not this adapter.
