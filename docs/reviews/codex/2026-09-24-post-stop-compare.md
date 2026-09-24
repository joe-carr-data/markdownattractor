# Codex review — `mda eval --compare` and `tune.sh explore` (`feat/bench-post-stop`)

| | |
|---|---|
| Date | 2026-09-24 |
| Scope | `crates/mda-core/src/eval/paired.rs` (new), `crates/mda-cli/src/commands/eval.rs` (`--compare`, `--run-name`, `--draws`), `scripts/eval/tune.sh` (`explore`, `explore-table`, `explore_invalid`), the post-stop wording in `evals/results/docsqa/TUNING.md` and `docs/benchmarks.md` |
| Reviewer | Codex CLI via the shared companion runtime, `gpt-6-astra`, thread `01a0d33e-9d1e-76f0-a52c-86036f887a80`, read-only, synchronous |
| Pinned to | `8479746` (branch head; base `main` at `28b4946`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 4 findings (2 Medium, 2 Low): **all accepted and fixed** before merge. Verdict was "mergeable after fixes". |

## 1. Findings (Codex, condensed; file:line against `8479746`)

| # | Sev | Finding | Fix |
|---|---|---|---|
| F1 | Med | The page and the ledger said c6/c7 "change no question"; they leave success@5 unchanged but move ranks below the cutoff (c6: `github-docs::126137` 14 → 9; c7: `github-docs::152845` unranked → 20). The causal explanation ("the first fetch already reaches ten pages") was also too broad: a baseline question fetched 60. | Wording narrowed to "unchanged per-question hybrid success@5" with the two rank moves cited; the explanation removed (`docs/benchmarks.md`, `TUNING.md`, handoff). |
| F2 | Med | `post-stop-*` was protected only through `explore`'s decisions: a fresh `post-stop-*` name routed through `trial` could be decided `keep` and adopted. | `trial` and `keep` refuse any `post-stop-*` name. |
| F3 | Low | A failed paired comparison deleted the completed observations, replaced the manifest with an invalid one and left the local decision file saying `screen-pass`/`screen-fail`. | The observations stay archived beside `compare.err`; the manifest's decision becomes `invalid-not-adopted` with the error; the local decision file is rewritten too (and on an evaluation failure as well). |
| F4 | Low | `percentile` used `round((N−1)·q)` instead of nearest rank `⌈N·q⌉`: at 5,000 draws the lower endpoint was the 126th draw, not the 125th. | Nearest rank implemented and unit-tested; the five `compare.json` regenerated (one endpoint moved: c4's objective lower bound −0.0200 → −0.0198; every other endpoint ties between the 125th and 126th draws). While regenerating, the joint draw turned out to depend on the order the projects were passed in; each project now has its own generator stream keyed by seed and label, tested order-independent, and the runbook says so. |

Also raised and applied: screening used the local `baseline.json` while pairing used the archived baseline results without checking their agreement → `explore` now refuses to run when the two objectives differ; "unrounded objective difference" describes the computation, the ledger prints four decimals → wording.

## 2. Answers (condensed)

- **(a)** The bootstrap is a correct within-project paired bootstrap; the joint resampling of the objective (one draw resamples every project, equal-weight mean of the per-project means) is right. The only issue was the percentile convention (F4).
- **(b)** `zone = u64::MAX − u64::MAX % n` with strict `v < zone` accepts exactly a multiple of `n` values: unbiased; `(M − n + 1) % n` would not be an equivalent substitute in this construction.
- **(c)** `read_hits` is consistent with the scorer: a missing external-arm question is still a row (`rank: null`) and counts in the denominator; the hybrid rows have no missing questions anyway.
- **(d)** The failure paths exit through the restoration trap; `explore_invalid` writes a manifest before the ledger row (no orphan rows). The empty-base check refuses a base moved by `keep`; it does not verify the stores' configs against the archived baseline (applying an empty base changes nothing) — accepted as a known limit, recorded here.
- **(e)** Settings did reach the runs: `apply_kv` skips `fetch`, `fetch_of` passes it as `--fetch`; `search_and_stopwords` is read by `for_config` and the lexical search. All four c6 archives record `fetch: 60`, all four c7 archives `and_stopwords: true`.
- **(f)** Post-stop timing, non-adoption, descriptive intervals and the multiple-trial limitation are disclosed; the retrieval-invariance claim was the one overclaim (F1).
