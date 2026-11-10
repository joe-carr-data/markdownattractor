# Codex code review — git-aware times (2026-11-10)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: branch `feat/git-times`, commits `33df2f6`, `008b0e2`, `cf39bd8` (`crates/`), against ADR-0008 and design v3. Codex inspected the code, ran read-only git calls and an in-memory migration; it did not run the test suite.

**Verdict:** "Not ready. One High and five Medium findings need fixing first." (Eight findings counted below: 1 High, 6 Medium, 1 Low.)

## Findings and triage (all accepted and fixed)

| # | Sev | Finding | Fix |
|---|---|---|---|
| 1 | High | `gittime::run` joined its pipe threads without a deadline: a wrapper or descendant holding stdout or stderr open defeats the timeout, hanging `mda index` and daemon shutdown; the `try_wait` error path leaked the threads. | The deadline covers the whole call. Pipe threads are never joined: they hand their bytes over a channel read with the remaining time, so a held pipe ends in a timeout error. Tests: a call that outlives its timeout, a descendant holding the pipes after the child exits, both bounded. |
| 2 | Medium | The slice deadline started before discovery and the tree listing: a slow setup examined nothing, and the daemon recorded the commit and stopped; each slice restarted at the first path, so a slice of mismatches starved later documents. | The deadline never stops a call before its first document. A sweep carries a cursor (`DateLimits::start_after`, `DateReport::last_path`) and ends when a slice finds nothing after it. Test: documents behind two that cannot be dated yet are reached; an expired deadline still examines one. |
| 3 | Medium | `mda index` delegated to a running daemon skipped the restore and requeue that the inline command does. | The daemon's command handler restores file times under `time_source = "filesystem"` and requeues undated documents before waking the dating task. No dedicated test (the two store operations are tested; the handler wiring is not). |
| 4 | Medium | The v4 migration left `file_mtime` NULL although `updated_at` held exactly that time, so a later restore fell back to `first_seen_at`. | The migration backfills `file_mtime = updated_at`. The migration test asserts it. |
| 5 | Medium | A byte-only change (CRLF saved back as LF, trailing spaces) keeps the content hash, so an ineligible document was never examined again. | An unchanged-content sighting with a different modification time stores the new time and marks the document as waiting (`Store::note_file_seen`; a read first, so an unchanged file costs no write). Test: CRLF to LF becomes eligible and is dated; the same time again requeues nothing. |
| 6 | Medium | Save notifications bypassed the back-off for roots without a usable repository: a discovery per watcher batch. | The dating task keeps a no-git-before instant while the root is unusable and honours it for notifications and polls alike. No dedicated test. |
| 7 | Medium | Passing the output cap only discarded bytes; the child ran on until it exited or timed out, and stderr overflow was ignored. | A reader that passes its cap sets a flag and stops; the wait loop kills and reaps the child at once. Stderr has its own cap. Test: `yes` on stdout and on stderr is killed immediately. |
| 8 | Low | The "not a repository" tests assumed TMPDIR is outside every checkout. | Both tests return early when the temporary directory is inside a work tree. |

Also applied: the new fields on the serialisable store structs carry `#[serde(default)]`, so objects written before schema v4 still deserialise.

## Found while fixing

- The cursor of finding 2 introduced a bug of its own, caught by the daemon test: a commit landing mid-sweep marks documents the cursor has already passed, and the sweep ended with them waiting and the new commit recorded as seen. A sweep now restarts when a slice reads another commit than the previous slice.

## Verified by the reviewer as correct

Stale checks use `file_mtime`; the other consumers of `updated_at` (recent, search filters, timeline events, the hot set) behave as intended; dating writes are conditional `IMMEDIATE` transactions; marking and advancing the validated commit are atomic; a mismatch-only slice does not spin; stdin and both outputs are served concurrently; the git commands behave as assumed (empty prefix, `--literal-pathspecs` with `--follow`, subtree `ls-tree`, NUL-delimited paths, `--name-only` with `--diff-merges=separate`); line mapping, zero sections and front matter; the migration keeps tombstones; no dating on open, search, status, card or MCP; `MDA_NOW` and filesystem mode guard every git entry point. Not exercised: Windows discovery output and path forms (CI runs the suite on Windows).
