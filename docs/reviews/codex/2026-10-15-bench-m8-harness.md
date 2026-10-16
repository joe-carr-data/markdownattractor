# Codex review — the T5 harness, the clock seam and the replay analysis (`feat/bench-m8`)

| | |
|---|---|
| Date | 2026-10-15 |
| Scope | `crates/mda-core/src/clock.rs` and its call sites; `crates/mda-core/src/eval/replay.rs` and `mda eval --replay-analysis`; `scripts/eval/t5.sh`, the `git`/`mda-index` arms and `ARM_WITH_GIT` in `lib.sh`, the T5 block of `freeze.sh`, the `t4.sh purge` backup; the committed T5 inputs (`range.json`, `steps.jsonl`, `questions.jsonl`); runbook §7.5; ADR-0006 |
| Reviewer | Codex CLI via the shared companion runtime, `gpt-6-astra`, thread `01a13e8e-5674-7933-b8d1-542218e5556d`, read-only, synchronous |
| Pinned to | `c404628` (branch `feat/bench-m8`, base `main` at `6ffa140`) |
| Triaged by | Claude (Fable 5.1), same day, before the T5 freeze |
| Outcome | 16 findings (10 High, 6 Medium): **16 accepted, 16 applied** before the freeze; the Tailwind questions regenerated; the dry-run replay re-validated with the two-way validator. Verdict as given: "Not ready to freeze; fix these issues, regenerate the questions, and verify a fresh replay plus failure-case scoring before freezing" — followed. |

## Findings and what changed

| # | Sev | Finding | Applied |
|---|---|---|---|
| 1 | High | A fresh replay exited before its first step: the record glob reached `jq` literally and `pipefail` made the empty assignment fatal. | Records are iterated with a guarded `for`; an empty directory is the normal first run. |
| 2 | High | Validation could pass an empty or incomplete store: only existing rows were checked; missing documents, sections and tombstones were never mismatches. | Two-way validation from the step trees: a live document for every indexable markdown file of the final tree (config ignore rules applied) and no other; its sections equal `mda parse` of the file; a tombstone for every file present earlier and absent at the end and no other; `passed` requires `docs_checked == expected_documents > 0`. |
| 3 | High | Expectations came from per-commit events flattened per day (same-day add+delete, rename chains); the fallback hid missing first-seen expectations. | Existence, first observation, modification and deletion are derived from consecutive step trees (`ls-tree`, `diff` step n−1 → n); first-step flattening, renames (delete + add) and re-additions are explicit. |
| 4 | High | Section ground truth compared against the day's last commit's parent, not the previous step's tree. | The step list records the tree diff against the previous step (`diff_base`); the section differ parses the blob at step n−1 and step n. Questions regenerated for both projects. |
| 5 | High | The page universe was the final corpus list: deleted or renamed-away pages could be right answers scored as false positives; `deleted_on` never cleared. | The universe is every markdown file under a content root present at any step; the question text states that pages deleted later count and asks for the path the page had; the grader resolves answers against that historical universe. |
| 6 | High | Counted runs did not enforce the freeze or the preflight; a dry-run replay could become the counted one. | `replay` requires `freeze.sh --check --table T5` (or `T5_DRY_RUN=1`, whose records carry `frozen_sha256: dry-run`) and refuses records from another freeze or binary; `run` requires the freeze check, a passing preflight bound to the current store, and a validation bound to the store and step list; `T5_SMOKE` runs live in `runs-smoke/`. |
| 7 | High | The freeze hashed the committed step lists while the replay read `$RUN` copies; archived graphs while serving copies; the wrong qmd indexes; fixed run parameters despite env overrides. | Every phase reads the committed `steps.jsonl` (`steps_file`); `arms` and the preflight require the served graph's hash to equal the archived one; the `t5-qmd-<project>` fingerprint is recorded in the preflight report and the freeze's Runtime; the run parameters are constants for counted runs (`T5_*` overrides die unless `T5_SMOKE`). |
| 8 | High | Retries overwrote the trace; effort came from the last attempt; the published cost ignored failed attempts. | Every attempt keeps its own trace (`.a1`, `.a2`); the scored row takes the last attempt's answer and sums tool calls, source tokens and cost over all attempts (`cost_usd_last_attempt` beside it). |
| 9 | High | A malformed trace left no row; `wait` ignored worker status. | `attempt_of` never fails (a truncated trace yields `parsed: false`); row assembly has a fallback error row; `run` exits 3 when any manifest cell lacks a row; `grade` refuses an incomplete run set. |
| 10 | High | Two arms failing everything would have a zero correctness difference and could earn the effort claim; effort intervals could be over a different sample than correctness. | `replay.rs`: the claim additionally requires an effort value for every question of both arms (`n` equal across the three intervals) and at least one correct mda answer; two tests added; the criterion text on the page and in the freeze says so (clarified before the freeze). |
| 11 | Med | Section grading iterated a Python set (nondeterministic) and accepted invented prefixes and ambiguous suffixes. | Answers are resolved against the page's heading paths at the final checkout: exact, or the unique path ending in the answer; sorted, deterministic. |
| 12 | Med | `file_times` fell back to the wall clock; the embedding rebuild ran without the override. | The fallback goes through `clock::now`; `rebuild --embeddings` runs under `MDA_NOW` = the final step's date. |
| 13 | Med | `mda index` under the override delegated to a running daemon, which stamps its own clock. | `index` refuses delegation when the override is active (clear error naming `mda stop`). |
| 14 | Med | Exhausted summarisation rounds still marked a step complete. | A step record is written only when nothing is pending and nothing failed; otherwise the replay dies and the step is retried on the next run. |
| 15 | Med | `arm_tool_used` counted failed calls; mda's regex ignored the temporal tools. | Activation requires a successful matching call; mda and mda-index match `mda_search|mda_timeline|mda_recent`; denied Bash attempts stay in the effort count. |
| 16 | Med | The table aggregated present grade rows; the analysis counts missing runs as failures. | The table renders per-arm numbers from `analysis.json` (manifest-complete) and shows `failed/total` runs. |

Also from the review's verdict: the dry-run Tailwind replay is re-validated with the two-way validator before the freeze, and the counted replays start only under the freeze (`T5_DRY_RUN` is never set for them).

## After the review (same day)

Two further changes before any answer row, outside the review's scope and recorded here for completeness: `t5.sh replay` refuses a reused working copy (file birth times older than the clone: macOS moves a birth time down to an earlier mtime and never back, so a second replay in the same clone inherits the first one's `created_at`; found on the second dry run); and the declared accepted-probe-failure mechanism for graphify's activation probes under equal evidence (plan amendment of 2026-10-15), which required one freeze rewrite after the Tailwind replay and before any answer run.
