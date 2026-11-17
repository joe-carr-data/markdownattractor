# Codex review — the change unit for `mda check` (2026-11-17, PR #91)

Reviewer: Codex (`gpt-6-astra`, fresh thread `01a1eb52-6572-7543-bc6d-36e3a357237f`, read-only, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: `impact::{changes,terms}`, the `gittime` additions, `tests/impact_git.rs`.

**Verdict:** "Changes requested." Also: "I found no verified case where the unique-heading pairing attaches another heading path's earlier text as `before`."

**Coverage of this round:** the reviewer reported on git configuration, the hash-matching step and tokenisation. It gave no explicit findings on `lines_only_in`, on the rest of the git questions (option injection through the revision, `-z` parsing, blob size and binary data, submodules), on test gaps or on the Rust rules. Those were **unreviewed, not clean**; round 2 below covers them.

## Findings and triage, round 1 (4: 1 P1, 3 P2; all accepted and fixed in the PR)

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | P1 | `changed_since` inherits `core.fsmonitor`: a repository's configuration can make git start a program during the comparison (`git diff` against the working tree refreshes the index). Reproduced by the reviewer. | **Fixed.** Every git call (`base`) now passes `-c core.fsmonitor=false -c core.pager=cat -c diff.external= --no-pager`. Regression test with all three configured to touch a marker file; checked by hand that the hook runs without the override and not with it. The calls in released versions (`rev-parse`, `ls-tree`, `hash-object`, `log`, `blame <commit>`, `merge-base`) do not refresh the index, so they are not believed to have been exposed; that was reasoned, not tested. |
| 2 | P2 | Matching unchanged sections by hash alone loses identity when two parents hold identical text: removing `A/X` reported `B/X` deleted; an edited section whose old text was copied elsewhere became an addition. | **Fixed.** Unchanged sections match first on heading path and hash; a cross-path (moved) match is taken only when no section competes for either heading path. Tests for both inputs. |
| 3 | P2 | Whitespace tokenisation glued markdown syntax to values: `|timeout|30|seconds|` gave no `30 seconds`; `[guide](docs/v2.md)` gave `guide](docs/v2.md`. | **Fixed.** Table bars, brackets and parentheses separate tokens before classification. Tests. Done by a line rule, not by the markdown parser: the terms are search hints, not a parse (a missed term lowers recall, it cannot create a wrong finding). |
| 4 | P2 | Stripping backticks discarded short or common identifiers: `` `on` `` → `` `off` `` gave no terms. | **Fixed.** Code spans are split out first and every token inside one is an identifier, whatever its length or spelling; unbalanced backticks leave the line as text. Tests. |

## Round 2 (thread `01a1eb58-e619-7563-a213-f153aa152f6a`, on commit `99f44df`)

**Verdict:** "Changes requested." The reviewer could not build or run tests in its read-only sandbox; its git experiments used the installed git 2.39.5. It stated for each of the seven requested items whether it was examined.

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | P1 | `git diff` against the working tree runs the repository's clean and process filters; `--no-ext-diff` does not stop that. | **Fixed by removing the call.** `changed_since` no longer asks git to compare with the working tree: it hashes the working files raw in one `hash-object --no-filters --stdin-paths` call and compares with the commit's tree. Confirmed by experiment that the old call ran a clean filter and the new one does not; regression test with clean, smudge and process filters configured. |
| 2 | P1 | A missing object in a partial clone can be fetched on demand, through whatever transport the configuration names. | **Fixed.** Every git call passes `-c protocol.allow=never` (works on old gits) and `GIT_NO_LAZY_FETCH=1` (git ≥ 2.45). Test: a partial clone over `file://`; reading a missing blob is an error and no object arrives. |
| — | (from the coverage table) | `blame` applies a configured textconv driver by default; the call had no `--no-textconv`. `diff.external=` (empty) is an error on git 2.39, not "unset". | **Fixed.** `blame --no-textconv`; the `diff.external` and `core.pager` overrides are gone (no diff is run; `--no-pager` stays). **This one is in released code:** the dating pass of 0.2.0–0.2.4 runs `git blame`, so a textconv program named in the repository's own git configuration would run during dating. Confirmed by experiment and by the regression test, which fails when the flag is removed. A repository's `.git/config` is local and is not set by cloning, so the program is one already configured on the machine; it is still a program a background indexer has no business starting. In the changelog. |
| 3 | P2 | Double-backtick spans, escaped backticks, `fn()` and URLs with parentheses lost the replaced value. | **Fixed, with the parser.** Each changed line goes through comrak (`markdown::inline_pieces`): code spans and link destinations are what the parser says they are; a call such as `fn()` stays whole. Tests for each input. This is the project rule from the `.mdx` review (hand the line to the real parser). |
| 4 | P2 | Section matching was quadratic (about 12.5 million comparisons for 5,000 identical sections). | **Fixed.** Queues indexed by (heading path, hash), by hash, and by unique heading path; unmatched-path counts are maintained. Test: 20,000 sections compared with themselves. |
| 5 | P2 | An indentation-only change in code gave no changed lines (lines were trimmed before comparison). | **Fixed.** Lines are compared with their indentation and returned trimmed. Test. Order-only changes (two list items swapped) still give no changed lines; the section is still a change, it only yields no "replaced" terms. Accepted as is. |
| 6 | P2 | A changed path that is not UTF-8 disappeared silently. | **Accepted as a stated limit.** The comparison is now built on `Repo::tree`, which holds only regular files with UTF-8 paths, and on the indexed working files, whose paths are strings; such a file is not indexed by mda at all. The function's documentation says so. |
| 7 | P3 | The copied-text regression test could not collide (different heading text, so different hashes). | **Fixed.** The copy is `## X` under another parent; the test asserts the hashes are equal before checking the outcome. |

Other points from round 2:
- `resolve_commit`: **tightened.** The name is resolved to an object id first and that id is peeled to a commit, so a path ending in `^{commit}` cannot stand in for the check; `:/text` and `^{/text}` searches and input over 200 bytes are refused. Test.
- Statuses `T`/`U`, unmerged index, intent-to-add, submodule entries from `git diff`: **no longer applicable**, the function does not parse `git diff` output. A conflicted file is compared as the bytes on disk.
- An oversized blob (over the runner's 64 MiB cap) is an error, not a truncated success; binary data is returned as bytes. **Clean as reported.**
- Parser normalisation hides trailing-space changes (a markdown hard break). **Existing behaviour, accepted**: such a change alters no statement.
- The integration test helper runs git without a deadline, like `tests/git_times.rs`. **Deferred** (backlog: a shared bounded test helper).
- Not examined by either round: behaviour on git versions other than 2.39.5.
