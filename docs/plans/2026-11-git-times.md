# Git-aware times, and the recency prior off by default (2026-11)

Date: 2026-11-10. Owner go: "go git times" (2026-11-10), with the requirement "not everything is gonna be a git repo" — both sources. Parent plan: `docs/plans/2026-11-growth-launch.md` (Phase 0). Decision records: ADR-0007 (recency prior off, shipped first); the git-times ADR follows the reworked design.

## Goal

Make "when each section last changed" and "when this document was created" true on the repositories users clone, without breaking folders that are not git repositories, uncommitted work, the save-to-card latency (G1) or the historical replay (ADR-0006). First, stop the recency prior from burying old documents: git times would switch it on for every git user.

## Why the recency prior goes first (measured 2026-11-10)

The golden set's 60 labelled queries, same cards, same binary, only the file ages changed (`scripts/demo/make-corpus.py`: dated file names keep their date, runbooks days old, ADRs one to two years):

| Half-life | Ages | lexical raw success@5 / MRR | cards + raw | hybrid |
|---|---|---|---|---|
| 30 days (default) | flat | 0.883 / 0.747 | 0.900 / 0.777 | 0.983 / 0.853 |
| 30 days (default) | realistic | 0.550 / 0.379 | 0.783 / 0.520 | 0.900 / 0.629 |
| 365 days | realistic | 0.700 / 0.518 | 0.800 / 0.609 | 0.950 / 0.688 |
| off | realistic | 0.883 / 0.747 | 0.900 / 0.777 | 0.983 / 0.853 |

With the prior off, realistic ages score exactly as flat ages. None of the 16 time-flagged queries gains from it (they name a date or an event; filters serve "since Monday"). The published retrieval tables (T1, DocsQA) already ran with the prior off (`eval/docsqa.rs`: "a frozen corpus has no recent"), so the product default differed from the published configuration. These are development numbers on one small fictional corpus, not a published table.

## Design v1 (Codex design pass 2026-11-10: **rework**, 15 findings, `docs/reviews/codex/2026-11-10-git-times-design.md`; v2 below replaces it)

- **Sources, per file.** A root that is not inside a git work tree, a shallow clone, a missing `git` binary, `time_source = "filesystem"` in config, or `MDA_NOW` set: filesystem times exactly as today. Inside a full work tree, a file that is tracked and identical to `HEAD` gets git times; an untracked file, or one with staged or unstaged changes, keeps filesystem times.
- **Which git time.** Author time (when the change was written), not committer time (rebases and cherry-picks reset it). A document's `created_at` is the author time of the oldest commit in `git log --follow` for its path; its `updated_at` the newest. A section's `updated_at` is the newest author time over its lines in `git blame --line-porcelain`. Known limit, documented: a section changed only by deleting lines keeps the time of its remaining lines.
- **When.** A separate pass after indexing (`Engine::date_from_git`), never before a file is raw-searchable (golden rule 5) and never on the save path: a saved file is dirty until committed, so it keeps its filesystem time and costs no git call. The pass runs after `mda index` / `index_root` and after each daemon rescan; it dates only rows whose source is still `filesystem` and whose file is now clean and tracked, so a repository is dated once and a commit re-dates the sections it touched at the next rescan.
- **Edits.** A section whose hash is new in its document gets the file's mtime and source `filesystem`, as today; an unchanged section keeps its time and its source.
- **Storage.** Schema v4 adds `docs.created_source`, `docs.updated_source` and `sections.updated_source` (`'filesystem' | 'git'`, default `'filesystem'`, so existing rows stay valid and are re-dated by the first pass). `created_at_source` keeps its meaning for filesystem creation times. No table rebuild (a rebuild of `docs` would cascade-delete sections).
- **Child processes.** Every `git` call goes through one helper: no shell, `-C <root>`, `GIT_OPTIONAL_LOCKS=0`, stdout and stderr drained concurrently, a timeout, the child killed on timeout; a failure leaves the file on filesystem times and is logged, never fatal.
- **Surfacing.** The stored rows and `mda card` / `mda_card` carry the sources; `mda status` counts sections by source. The MCP search hit stays lean (unchanged).
- **Recency.** `search_recency_half_life_days` in `config.toml`, default `0` (off); any positive value restores a prior.

## Design v2 (after the Codex design pass; each line names the findings it answers)

- **Eligibility (F4, F11).** One `git ls-tree -r -z HEAD -- .` from the root gives the committed blob of every tracked path relative to the root; one `git hash-object --no-filters --stdin-paths` gives the working blob of each indexed file. A file is git-dated only when the two blobs are equal (byte-identical, so blame's lines are the indexed lines). Everything else uses filesystem times: untracked, modified or staged files, files under smudge/clean filters or CRLF conversion, submodules and nested repositories (not in the root repository's tree), shallow clones, no `git`, `MDA_NOW` set, `time_source = "filesystem"`. Only present, indexed, `safe_join`-validated files are dated; `git` paths never add documents.
- **Meaning of a git time (F8, F9).** *Authored* time: a commit's author time capped at its committer time (no future dates). A document's created time is the minimum and its changed time the maximum over the commits of `git log --follow` for its path; a section's changed time is the maximum over its lines in plain `git blame --line-porcelain` (no `-M`/`-C`), using blame's final-file line numbers against the parser's one-based inclusive ranges. Timeline events stay observation history and are never backdated.
- **Occurrences (F5).** A dating pass dates every section of an eligible document, so incremental results equal a clean rebuild. Upsert keeps an unchanged section's time by (hash, occurrence number), not by hash alone; dates never move between documents.
- **Freshness (F1).** Each document records `dated_rev` and `dated_blob`. A pass re-dates a git-dated document when `HEAD` differs from `dated_rev` and either `git diff --name-only -z <dated_rev> HEAD` names the path or `dated_rev` is no longer reachable (rewritten history).
- **Writes (F2).** Results are computed outside any transaction and applied per document in a short `IMMEDIATE` transaction that checks the content hash, the section ids, hashes and line ranges, and the dating state read at the start; a mismatch discards the result (the next pass retries).
- **Scheduling (F3, F6, F13).** `mda index` runs a pass after indexing. The daemon runs passes on their own thread with their own store connection: after the initial index, when a single-file index queues a file, and when a poll of `git rev-parse HEAD` (every 30 s) sees it change. A pass has a time budget (20 s in the daemon), stops on cancellation and resumes next time; failures are remembered per (path, revision, blob) and not retried until one of them changes. Saves keep their filesystem time and never wait for git.
- **Transitions (F7).** Filesystem to git when a file becomes eligible (after a commit). Git to filesystem for the sections an edit changes (upsert), the rest keep their git times until the next pass. A document's created time moves only from filesystem to git, never back, except when history is rewritten (F1). A rename detected by the daemon inherits created time and its source; after the commit, `git log --follow` decides. `time_source = "filesystem"` makes the next pass restore file times on git-dated rows. `first_seen_at` never changes.
- **Storage (F14).** Schema v4: `docs.created_source`, `docs.updated_source`, `sections.updated_source` (`NOT NULL DEFAULT 'filesystem'`, checked), `docs.dated_rev`, `docs.dated_blob`, and a `git_date_failures` table. `created_at_source` applies only when `created_source = 'filesystem'`.
- **Benchmarks (F12).** No dating when opening the store, on reads or in `mda mcp`. Fresh agent runs with a new binary differ from the published ones through ADR-0007 already; regenerations from committed artifacts are unaffected.

## Tasks

- [x] Recency: config key, default 0, wired into CLI and MCP search; `docs/design/search.md`; a test that the default is off (shipped separately, ADR-0007).
- [x] ADR-0007 (recency). - [ ] The git-times ADR, after design v2.
- [ ] `gittime` module: discover (work tree, prefix, shallow), status of paths (tracked and clean), per-file log times (`--follow`), per-line blame times; one child-process helper with a timeout.
- [ ] Schema v4 and store methods: carry sources on upsert; `redate_from_git`; rows needing a date.
- [ ] `Engine::date_from_git` after `index_root`; daemon rescan; `time_source` config; off under `MDA_NOW`.
- [ ] Surfacing: card, status.
- [ ] Tests (temporary repositories with fixed author and committer dates): not a repo, git missing, shallow clone, clean tracked file (document and per-section times), untracked, modified and staged files, a CRLF or filtered checkout falls back, a submodule falls back, rename followed, a root below the top level, paths with spaces and unicode, an edit then a commit re-dated after a `HEAD` change, an amended commit re-dated, duplicate sections in one file, front matter and MDX imports, a heading-only section, a future author date capped, a concurrent re-index discards the stale result, `MDA_NOW` (subprocess, full clone) means no git call, `time_source = "filesystem"` restores file times, migration of a populated v3 store, save-to-raw latency unchanged while a pass runs.
- [ ] Measure the pass on this repository's `docs/` (73 files) and on a Prisma copy; record in `docs/benchmarks.md` as development numbers.
- [ ] Docs: `docs/design/search.md` (times), README provenance line and hook, CHANGELOG, STATUS, aha.
- [x] Codex design pass on v1 (rework, 15 findings, all accepted into v2).
- [ ] Codex pass on v2 before code; review of the implementation PR before merge.

## Exit criteria

- [ ] On a full clone, `mda card` for a committed section shows its blame author time and source `git`; a non-git folder shows source `filesystem` with today's behaviour.
- [ ] An uncommitted edit never triggers a git call on the save path; G1's save-to-card path is unchanged.
- [ ] `MDA_NOW` set means no git call at all (tested).
- [ ] The golden eval with realistic ages equals the flat-age result under the new default.
- [ ] `make check` green; Codex review triaged in `docs/reviews/codex/`.
