# Codex review — `.mdx` ingestion (PR #18, benchmark plan B0a)

| | |
|---|---|
| Date | 2026-09-22 |
| Scope | `crates/mda-core/src/markdown/{mod,tests}.rs`, `crates/mda-core/tests/fixtures/page.mdx`, `crates/mda-core/src/walk.rs`, `snippet_of`/`is_markup_only` in `crates/mda-core/src/search.rs`, the MDX CLI test, `scripts/nudge.sh`; context `docs/design/ingestion.md`, `design/search.md`, `CLAUDE.md`, `.claude/rules/rust.md` |
| Reviewer | Codex CLI 0.155.1 via the shared companion runtime, model `gpt-6-astra` (reports itself as GPT-6), thread `01a0ca73-bcc9-7770-b083-cd7abebbc986`, read-only. The subagent's first attempt spawned a nested `codex exec --sandbox read-only --ephemeral`, which fails to initialise here ("Operation not permitted"); the review was produced on the second turn by the companion thread itself. |
| Pinned to | `c41a075` (branch `feat/mdx-ingestion`, diff `cf4d8da...c41a075`) |
| Triaged by | Claude (Fable 5.1), same day |
| Outcome | 6 findings (2 High, 3 Medium, 1 Low): **6 accepted and fixed** on the same branch before merge. All three suggested test inputs added verbatim. |

## 1. Packet

The `/codex-review` reviewer prompt with the north star, the rust rules, `design/ingestion.md`, the "What a hit carries" section of `design/search.md`, the diff and the listed files in full, plus the note that section hashes drive re-summarisation. Five questions: (1) can the ESM rule or the HTML-block heading scan break the section invariants; (2) which existing `.md` inputs parse differently and what would it cost; (3) panics, loops, quadratic time in the new helpers; (4) is `block_type >= 6` the right gate and which real MDX constructs still lose or gain headings; (5) missing tests.

## 2. Findings (Codex, condensed; file:line against `c41a075`)

| # | Sev | Finding | Codex fix |
|---|---|---|---|
| F1 | High | The leading-ESM rule fires on ordinary markdown: `import duties apply.\n\nFurther guidance.\n` loses its first paragraph in a `.md` file; the rule checks no JavaScript syntax and applies to every extension, changing preamble text and hashes. | Make ESM exclusion conditional on MDX parsing. |
| F2 | High | The heading scan inside HTML blocks has no fence, comment or raw-element state: `<details>\n<summary>Example</summary>\n```sh\n# not a heading\n```\n</details>` makes an H1 section and a document title out of a shell comment. | Track fences (with delimiter lengths), comments and raw-text elements while scanning. |
| F3 | Med | An AST heading inside the excluded ESM block (`export const example = \`\n# fake\n\`;`) opens a section at line 2 because heading handling returns before the `body_start` check. | Apply the exclusion to headings too. |
| F4 | Med | Recovered headings mangle literal hashes and block-like text: `# C#` → `C`; `# ---` → empty (the content is reparsed as a thematic break). | Parse each recovered line as a heading with comrak and take its inline text. |
| F5 | Med | The multi-line tag filter in `snippet_of` drops prose: `<Note\n kind="tip">Important prose\nFollowing prose.` gives an empty snippet because the closing line contains `>` but does not end with it; any `<`-prefixed prose is treated as an unfinished tag. | Detect the actual tag terminator, keep what follows, only treat `<Tag`/`</Tag` as markup. |
| F6 | Low | Quoted title extraction truncates escaped titles: `export const title = "A \"quote\"";` → `A \`; YAML `'User''s guide'` → `User`; the test locked in the truncation. | Honour the quoting rules or decline the value. |

Answers, condensed: (1) no counterexample to the range/text invariants (static reading; F3 violates the exclusion but its section is well formed); (2) `.md` inputs that change: leading `import `/`export ` paragraphs (F1), ATX lines inside type-6/7 HTML blocks (legitimate headings inside `<details>` now split, false ones from embedded examples, F2), front matter titles where there was no H1 (title only, no hash change); (3) no small-input panic; quadratic heading-path copying and `Vec::contains` on fence languages pre-exist; `depth += 1` and `i as u32 + 1` overflow only on inputs of 2³² lines or characters; (4) `block_type >= 6` is a useful outer filter but not a correctness boundary without F2's state; `{}` expressions and Liquid branches are not parsed as code, which predates this PR; (5) the three inputs below.

## 3. Triage

| # | Decision | What was done | Where |
|---|---|---|---|
| F1 | **Accept** | `markdown::Flavor { Markdown, Mdx }`, chosen from the extension by `Flavor::of_path`; `parse_str` is plain markdown, `parse_str_as` takes the flavour, `parse_file` and the pipeline's two re-parse sites (`disk_state`, `open_section`) use the file's flavour so a `.mdx` page is hashed the same way every time. The ESM rule runs only for `Mdx` and only on real statements: `is_esm_statement` wants a binding form or ` from ` after `import`, and a declaration keyword, `default`, `{` or `*` after `export`; "import duties apply." is prose in both flavours. | `mod.rs`; tests `plain_markdown_prose_starting_with_import_or_export_is_not_esm`, `mdx_and_markdown_flavours_agree_outside_the_esm_block` |
| F2 | **Accept** | `RawScan` carries fence state (character and length; closed by at least that many of the same character and nothing else), `<!-- … -->`, and `<pre>`/`<script>`/`<style>`/`<textarea>` until their closing tag, across the lines of a scanned block. | `mod.rs::RawScan`; test `html_block_scan_ignores_fences_comments_and_pre` (Codex's input verbatim) |
| F3 | **Accept** | A heading node whose start line is inside the excluded block is skipped. | `visit_top_level`; test `heading_inside_an_esm_template_literal_is_not_a_heading` |
| F4 | **Accept** | `atx_heading` pre-checks the shape, then parses the trimmed line with comrak and takes the heading node's level and inline text; anything comrak does not see as a heading is not one. | `mod.rs::atx_heading`; test `recovered_headings_keep_literal_hashes_and_dashes` |
| F5 | **Accept** | A tag opened across lines ends at its first `>` and the rest of that line is prose; `opens_tag` requires `<` followed by a letter, `/` or `!`, so `a < b > c` is prose; `depth` uses `saturating_add`. | `search.rs`; test `snippet_skips_markup_only_lines` (extended) |
| F6 | **Accept** | `quoted_string` with backslash escapes (JavaScript, YAML double quotes) or doubled quotes (YAML single quotes); an unterminated string is `None`. | `mod.rs`; test `export_title_and_front_matter_title_parsing` |

Also from the answers: `u32::try_from(i)` with saturation in `leading_esm_block`. The pre-existing quadratic heading-path copying is a follow-up, not this PR.

## 4. Patterns → rules

- **A dialect is a property of the file, not of the text:** every re-parse of a stored document (staleness checks, `open`) must use the same flavour as the indexing parse, or hashes disagree and a page is forever "stale".
- **A line scan inside a block carries the block's raw-content state:** fences (with their length rule), comments and `<pre>`-like elements make a `#` line content, whatever the block type.
- **Reuse the real parser for the recovered piece:** a recovered heading line is parsed by comrak as a heading, not re-implemented (`# C#`, `# Title ##`).

## 5. Follow-ups

- Heading-path copying is Θ(N²) in the size of an H1 times the number of subsections (pre-existing); measure on GitHub Docs before deciding whether to intern heading text.
- `{}` expressions and Liquid `{% ifversion %}` branches are text in both flavours; headings inside a JS block comment `{/* … */}` can still become headings through the ordinary AST. Revisit if the DocsQA metadata audit shows it matters.
