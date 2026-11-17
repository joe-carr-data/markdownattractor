# Codex review — the change unit for `mda check` (2026-11-17, PR #91)

Reviewer: Codex (`gpt-6-astra`, fresh thread `01a1eb52-6572-7543-bc6d-36e3a357237f`, read-only, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: `impact::{changes,terms}`, the `gittime` additions, `tests/impact_git.rs`.

**Verdict:** "Changes requested." Also: "I found no verified case where the unique-heading pairing attaches another heading path's earlier text as `before`."

**Coverage of this round:** the reviewer reported on git configuration, the hash-matching step and tokenisation. It gave no explicit findings on `lines_only_in`, on the rest of the git questions (option injection through the revision, `-z` parsing, blob size and binary data, submodules), on test gaps or on the Rust rules. Those are **unreviewed, not clean**; a second round on the fixed PR asks for them (below).

## Findings and triage, round 1 (4: 1 P1, 3 P2; all accepted and fixed in the PR)

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | P1 | `changed_since` inherits `core.fsmonitor`: a repository's configuration can make git start a program during the comparison (`git diff` against the working tree refreshes the index). Reproduced by the reviewer. | **Fixed.** Every git call (`base`) now passes `-c core.fsmonitor=false -c core.pager=cat -c diff.external= --no-pager`. Regression test with all three configured to touch a marker file; checked by hand that the hook runs without the override and not with it. The calls in released versions (`rev-parse`, `ls-tree`, `hash-object`, `log`, `blame <commit>`, `merge-base`) do not refresh the index, so they are not believed to have been exposed; that was reasoned, not tested. |
| 2 | P2 | Matching unchanged sections by hash alone loses identity when two parents hold identical text: removing `A/X` reported `B/X` deleted; an edited section whose old text was copied elsewhere became an addition. | **Fixed.** Unchanged sections match first on heading path and hash; a cross-path (moved) match is taken only when no section competes for either heading path. Tests for both inputs. |
| 3 | P2 | Whitespace tokenisation glued markdown syntax to values: `|timeout|30|seconds|` gave no `30 seconds`; `[guide](docs/v2.md)` gave `guide](docs/v2.md`. | **Fixed.** Table bars, brackets and parentheses separate tokens before classification. Tests. Done by a line rule, not by the markdown parser: the terms are search hints, not a parse (a missed term lowers recall, it cannot create a wrong finding). |
| 4 | P2 | Stripping backticks discarded short or common identifiers: `` `on` `` → `` `off` `` gave no terms. | **Fixed.** Code spans are split out first and every token inside one is an identifier, whatever its length or spelling; unbalanced backticks leave the line as text. Tests. |
