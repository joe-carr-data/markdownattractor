# Codex review — benchmark M3: the tuning knobs and the greedy loop (`feat/bench-m3`)

| | |
|---|---|
| Date | 2026-09-23 |
| Scope | `crates/mda-core/src/{config,embed,search,pipeline}.rs`, `crates/mda-core/src/store/mod.rs` (`search_cards_weighted`), `crates/mda-core/src/eval/docsqa.rs` (`RunOptions.search`), the call sites in `crates/mda-cli` and `mcp.rs`, `scripts/eval/tune.sh`, the loop driver; context execution plan §3, strategy rule 0.2, `docs/design/search.md` |
| Reviewer | Codex CLI via the shared companion runtime, `gpt-6-astra`, thread `01a0cfd1-00fe-70f0-a134-d3b52c121eb3`, read-only, synchronous |
| Pinned to | `f7fa54e` (base `main` at `c294d03`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 8 findings (3 High, 5 Medium): **8 accepted and fixed** before the loop ran its second candidate (the first candidate's re-embedding was kept; the loop was restarted on the fixed script). |

## 1. Findings (Codex, condensed; file:line against `f7fa54e`)

| # | Sev | Finding | Codex fix |
|---|---|---|---|
| F1 | High | A failed or interrupted trial leaves the candidate's settings installed in the four stores (restoration only after a successful evaluation); the driver logged failures and continued on an unaccepted base. | Snapshot and restoration trap before mutation; abort the driver on failures. |
| F2 | High | `FETCH=60` changed the evaluation and the ledger text but was not persisted by `keep`, so the next trial compared against a winner evaluated at a fetch depth it did not use. | Persist the complete effective base, adapter parameters included. |
| F3 | High | Candidate 2 (`with-entities`) reverses candidate 1 (`questions-first`) instead of adding one change, against greedy forward selection. | A combined variant with its own vector identity; candidate 2 appends entities to the current winner. |
| F4 | Med | `mda status` reported the unsuffixed model's vectors: a fully embedded v1 store switched to a variant reported full coverage before any variant vector existed. | Share the identity computation between `check()` and `embedder_for()`. |
| F5 | Med | Validation accepted positive infinity (zero contributions or infinite scores); the BM25 weight was formatted into SQL with three decimals (silent rounding, `NULL` for absurd values). | Finite bounds; bind the weight. |
| F6 | Med | The objective took `.runs[-1]` without checking its name: with embeddings off or the model missing, "hybrid tuning" would silently tune the lexical row. | Select the named hybrid row and require it on every project. |
| F7 | Med | `keep` accepted a discarded or stale candidate (two files existing was the only check). | A structured decision and a base fingerprint, verified before adoption. |
| F8 | Med | The ledger could not substantiate a reproduction: rounded scores and checkout HEAD only, raw reports uncommitted and overwritten on name reuse; the runbook called a fresh execution "regeneration, exact". | Archive immutable trial manifests and per-project reports bound to the binary and input hashes; regenerate rows from them. |

Answers, condensed: the AND/OR split is right (stop-words touch only the AND form, all-stop-word queries stay); `Matched` labels list membership, so a zero-weighted raw hit stays labelled with a zero score; the variant identity isolates vectors on every factory-wired path, `status` being the exception; `embed_pending` takes the text variant from the config and the identity from the embedder, so a hand-built mismatched embedder could mislabel (noted); the loop's arithmetic is right if all four rows are hybrid; omitting candidate 8 is reasonable and should be recorded; the stop counter should count only objective failures; `--fetch 30 → 60` is a fair reading of the candidate but an adapter parameter, not a product default; no test/holdout scoring found.

## 2. Triage

| # | Decision | What was done | Where |
|---|---|---|---|
| F1 | **Accept** | `snapshot` copies the four configs before any mutation and `restore` runs on every exit path (trap); every failure dies non-zero; the driver stops on any non-zero step. | `tune.sh`, `tune-all.sh` |
| F2 | **Accept** | The base is a persisted `key=value` file including `fetch=<n>`; every trial applies the base then its change; `keep` merges the trial's pairs into the base (later values win) and prints the fingerprint. | `tune.sh` |
| F3 | **Accept** | `EmbedText::QuestionsFirstWithEntities` (`+questions-first+with-entities`), `questions_first()` / `with_entities()` predicates; the driver runs candidate 2 as `questions-first-with-entities` when candidate 1 was kept, `with-entities` otherwise. | `config.rs`, `embed.rs`, driver |
| F4 | **Accept** | `embed::check` computes the same identity as `embedder_for` (base model + variant suffix); `status` uses it; test asserts they agree. | `embed.rs`, `status.rs` |
| F5 | **Accept** | Validation requires finite `search_rrf_k` in 1..=1e6 and weights in 0..=100; `SearchOptions::for_config` and `fuse` clamp non-finite values built by hand; the cards weight is bound as a parameter (`?3`), never formatted. Tests. | `config.rs`, `search.rs`, `store/mod.rs` |
| F6 | **Accept** | `run_all` requires the row named `hybrid (cards + raw + vectors)` with questions > 0 and complete card coverage on every project, and records the embedding model and effective search settings in the summary. | `tune.sh` |
| F7 | **Accept** | Each trial writes `<name>.decision.json` (`keep` / `discard` / `discard-guardrail`, base fingerprint); `keep` refuses anything but a `keep` decided on the current base. | `tune.sh` |
| F8 | **Accept** | Each trial is archived under `evals/results/docsqa/tuning/<trial>/` (manifest: hypothesis change, base before, code SHA, binary sha256, `FROZEN.md` and cards hashes, summary with denominators, decision; the four `results.json`); trial names are immutable; the ledger rows come from those files; the runbook §4d distinguishes regeneration (metrics from the archived page lists through `--arm-output`) from a rerun of the search. | `tune.sh`, runbook §4d |

Also from the answers: the stop rule counts only objective failures (a guardrail-only discard does not count); candidate 8's omission is written in the ledger header; candidate 6 is labelled an adapter parameter in the ledger.

## 3. Patterns → rules

- **Mutate only under a restoration trap**: a script that edits shared state snapshots it first and restores on every exit path.
- **A greedy step adds one change to the winner**: a candidate that replaces an earlier change is a different candidate and needs a combined configuration.
- **The identity of a derived artifact is computed in one place** (`check` and `embedder_for` agree), and numeric knobs are bound, bounded and finite.

## 4. Follow-ups

- Shell tests with a fake evaluator for restoration, fetch persistence, stale `keep`, missing hybrid rows and stopping (M5 failure matrix).
- Encode the text-variant ↔ identity relationship in `embed_pending` (take the variant from the embedder rather than the config) so a hand-built embedder cannot mislabel vectors.
