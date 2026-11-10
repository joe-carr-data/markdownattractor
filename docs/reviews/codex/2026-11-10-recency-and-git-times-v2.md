# Codex review — the recency change (code) and git-times design v2 (2026-11-10)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: commit `9f7fa28` on `feat/recency-off` (part A), and "Design v2" in `docs/plans/2026-11-git-times.md` against the 15 findings of `2026-11-10-git-times-design.md` (part B). Codex did not run builds or tests.

## Part A — recency off by default. Verdict: "mergeable; correct the documentation findings"

Confirmed by the reviewer: CLI search, explain, MCP and the golden eval build their options through `for_config`; the DocsQA adapter overrides recency to zero on purpose; existing `config.toml` files load (`serde(default)`), unknown keys stay rejected; the clamp is as stated. `freeze.sh --check` on the historical freezes rejects any changed source under `crates/`, as already documented on the benchmarks page; neither CI nor `make check` runs it.

| # | Sev | Finding | Triage |
|---|---|---|---|
| A1 | Medium | ADR-0007 said the prior affected no published number; T5's replay sets historical file times and its MCP searches used the 30-day default. | **Applied.** The ADR and CHANGELOG now say: T1 ran without it; T2, T4 and T6 corpora had one age; T5's searches were recency-weighted, so fresh T5 runs can rank differently; regenerations are unaffected. |
| A2 | Low | README and `docs/design/commands.md` still described search as recency-weighted. | **Applied.** Both say the prior is optional and off by default. |

## Part B — git-times design v2. Verdict: "not ready to implement"

Of the 15 earlier findings: 4 resolved (6, 9, 12, 14), 11 partial. New findings, all accepted into design v3:

| # | Sev | Finding | Triage (design v3) |
|---|---|---|---|
| B3 | High | Endpoint diffs miss an edit followed by a revert: the bytes return, the history changed. | Freshness uses the commits between the last validated `HEAD` and the new one (`git log --name-only`), not an endpoint diff. |
| B4 | High | Validating database rows does not bind the git result to the indexed content; a concurrent checkout can mix revisions. | The pass resolves `HEAD` once to a commit id and uses it for every command; it reads the file's bytes once, requires their blob id to equal the pinned tree's, parses those bytes, and requires the parsed hash and section ranges to equal the stored rows, before blame and again inside the write transaction. |
| B5 | High | Byte equality does not prove line equality: the parser turns a lone CR into a line break, git does not. | Any file containing a CR byte falls back to filesystem times. |
| B6 | Medium | Occurrence ordinals are not stable identities for duplicate sections. | Upsert keeps a time by hash only when the hash occurs exactly once in both the old and the new document; otherwise the occurrences are treated as changed. |
| B7 | Medium | `ls-tree` and `diff`/`log` report paths in different coordinate systems; filename encoding for `--stdin-paths`. | One representation: repository-relative paths (`--full-name`), converted with the root's prefix from `rev-parse --show-prefix`; `--literal-pathspecs`; a path containing a newline falls back. |
| B8 | Medium | A pass that exhausts its budget has no guaranteed continuation; a discarded result is not requeued. | Documents carry a "needs dating" state; the thread keeps running slices until none is left, and a discarded result leaves the state set. |
| B9 | Medium | Per-document reachability checks repeat history walks; a transient timeout suppresses dating for ever. | One validated `HEAD` per store and one history walk per `HEAD` change; failures have an attempt count and a retry time, three attempts per (revision, blob). |
| B10 | Medium | A second connection does not guarantee saves never wait. | One short transaction per document; a test measures save latency while a pass writes. |
| B11 | Medium | Polling and fallback transitions incomplete (no git, not a repository, repository lost, shallow). | Discovery states and their retry intervals are specified; existing git dates are kept when the repository becomes unavailable. |
| B12 | Medium | Capping at committer time does not exclude future dates; the deletion-only limitation was dropped from v2. | A git time is also capped at the clock when it is recorded; the limitation is documented with the time filters. |
