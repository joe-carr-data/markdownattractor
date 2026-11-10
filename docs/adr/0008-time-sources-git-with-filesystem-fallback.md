# ADR 0008: Time sources: git when it can be proven, the filesystem otherwise

Date: 2026-11-10
Status: Accepted

## Context
Every stored time came from the filesystem (modification time, birth time, or when a file was first indexed). In a fresh clone every document was "created" and "changed" at clone time, so "when did this section last change" was wrong on the repositories users clone. The owner asked for git times and required that folders without git keep working. Three Codex design passes (`docs/reviews/codex/2026-11-10-git-times-design.md`, `…-recency-and-git-times-v2.md`, `…-git-times-v3.md`; 31 findings) shaped the design; the plan is `docs/plans/2026-11-git-times.md`.

## Decision
We date a document and its sections from git only when a dating pass proves, at one pinned commit, that the bytes git describes are the bytes the index holds: the path is a regular file in the commit's tree, the file has no carriage return, the blob id of its bytes equals the tree's, and parsing those bytes gives the stored document hash and section ranges. Everything else keeps filesystem times: folders that are not repositories, shallow clones, untracked or modified files, files under filters or CRLF conversion, submodules, a missing `git`. Every time records its source (`git` or `filesystem`).

A git time is a commit's *authored* time: author time capped at committer time and at the clock. A document's created and changed times are the minimum and maximum over `git log --follow`; a section's changed time is the maximum over its lines in plain `git blame`. The pass runs after indexing, never on the save path, in the CLI after `mda index` and in the daemon on its own task (after the indexer changes something, and when a 30-second poll sees a new commit). It writes each document in one short conditional transaction. It never runs under `MDA_NOW` (ADR-0006) or with `time_source = "filesystem"`, when opening a store, on reads, or in the MCP server.

An edit puts the changed sections on the file's modification time until the file is committed; a section keeps its time across an edit only when its content hash is unique in the document before and after. Text that did not change keeps the date it was given, also when git can no longer vouch for the file (an uncommitted edit elsewhere in it, or a save that only changes line endings): the document's state then says `filesystem`, and its unchanged sections still carry their git times.

## Consequences
- Positive: on a cloned repository, hits carry the time their text was written; non-git folders and uncommitted work behave as before; the save-to-card path gains no git call; the replay is untouched.
- Negative: the first pass costs three git processes per eligible file (88 documents of this repository's `docs/` in 7.0 s on one machine); a large repository is dated in the background over minutes.
- Negative: a section changed only by deleting lines keeps its remaining lines' time, which can be older, and `--since` follows it; a rebase that rewrites author dates moves times; `git log --follow` can include an earlier file of the same name or a copy source.
- Negative: sections with duplicate content in one document fall back to the file time on any edit of that document until it is committed.
- Follow-ups: batch the history walks if large repositories need it; retry timers for failed documents (today: the next commit or the next `mda index`); kill the whole process group of a git call that times out (today the call returns at its deadline, and a descendant that holds a pipe keeps one reader thread until it lets go).

## Alternatives considered
- Committer time — rebases and cherry-picks reset it to the rewrite time.
- Git times only — breaks folders without git and uncommitted work.
- "Tracked and clean" as the eligibility test — filters and line-ending conversion make a clean file differ from its blob, and blame would describe other lines.
- Dating inside indexing — puts `git blame` before raw-searchability and on the save path (golden rule 5, G1).
- An endpoint diff to find changed files — misses an edit followed by a revert; the pass walks the commits in between, merges included.
