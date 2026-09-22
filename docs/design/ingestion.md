# Design — ingestion (what becomes a section, what becomes text, what is dropped)

As built for the benchmark plan's B0a (2026-09-22). This document describes *what the parser does* with the files the walker accepts; if it disagrees with `crates/mda-core/src/markdown/mod.rs` or `walk.rs`, the code wins and this file gets fixed.

## Files

The walker (`walk::discover`) accepts `.md`, `.markdown` and `.mdx` (any case), honours `.gitignore`, `.ignore`, `.markdownattractorignore` and the config's `ignore` patterns, skips hidden entries and never follows symlinks. Everything else is not a document. The daemon's `sync_path` and `mda index <file>` apply the same test.

## Section model (all three extensions)

A section starts at a heading and runs to the line before the next heading of any level; text before the first heading is a preamble section (level 0). The section text is **always the source lines of its range** (trailing whitespace stripped, `\r\n` folded), so the raw FTS index and `mda open` see exactly what is in the file. Nothing inside a section is rewritten or removed.

## What is excluded from sections

| Construct | Rule | Why |
|---|---|---|
| Front matter (`---` … `---` at the top; YAML, or TOML between the same fences) | Excluded; kept whole in `Document.frontmatter`. | Metadata, not prose. |
| Leading ESM block (MDX) | One or more `import` / `export` statements at column 0 right after front matter (blank lines allowed before), each running to the next blank line, are excluded like front matter. The preamble starts at the first line after the block. | They are component wiring, not content; carding them would waste a model call. |

An `import` or `export` line that appears after content (rare; six files in 1,711 real pages, all inside code fences) stays as text.

## What stays as text

JSX tags (`<Tabs>`, `<TabPanel label="…">`, `<Figure />`), JSX expressions (`{props.window ?? "30 minutes"}`, `{/* comment */}`), MDX component props spread over several lines, Liquid tags (`{% data variables.x %}`), HTML comments and any other markup. The card prompt treats section content as data, so markup in a section is noise for the summarizer, not an instruction; the raw index carries every identifier in it.

## Headings inside markup

CommonMark makes a raw HTML block of a line that is a complete tag on its own (`<Admonition type="note">`, `</TabPanel>`) and runs that block to the next blank line, so a heading glued to a tag without a blank line between (`</Admonition>\n## How it works`, or `<TabPanel …>\n  ### Config`) disappears into the block. The parser scans every HTML block of CommonMark type 6 or 7 (block-level and generic tags) for ATX heading lines (up to three spaces of indent, one to six `#`, then a space or the end of the line; closing `#`s stripped) and starts a section at each one, with the heading's plain text (inline code and emphasis stripped) in the heading path. Blocks of type 1–5 (`<script>`, `<pre>`, `<style>`, `<textarea>`, comments, processing instructions, declarations, CDATA) are not scanned: a `#` line inside them is content. A `#` line inside a code fence is never a heading, in either parser.

## Titles

`Document.title` is, in order: the first level-1 heading; else the front matter `title:` at column 0 (YAML `title:`, or TOML `title = "…"`; single or double quotes unwrapped, a trailing ` # comment` dropped, block scalars `|` and `>` ignored); else `export const title = "…"` from the leading ESM block. The title feeds the hit, the `recent` listing and the card embedding text; it is not a section.

## Checked on real corpora

`scripts/eval/mdx-accept` (kept in the session scratchpad; the numbers below are what it printed) ran `mda parse --json` over every file of the four DocsQA-Repo corpora at their pinned commits and compared, per file, the ATX heading lines outside code fences with the sections' start lines:

| Corpus (commit) | Files | Headings kept | Titled |
|---|---|---|---|
| Tailwind CSS `src/docs` (`bd868a3`), all `.mdx`, titles only in `export const title` | 197 | 1,328 / 1,328 | 197 / 197 |
| Prisma `apps/docs/content/docs` (`c4ac0e9`), all `.mdx`, imports on 278 pages | 685 | 9,499 / 9,538 | 685 / 685 |
| Supabase `apps/docs/content` (`6ea3567`), all `.mdx`, TOML front matter on the troubleshooting pages | 829 | 5,583 / 5,590 | 772 / 829 |
| GitHub Docs `content` (`c34e3dc`), all `.md` with Liquid | 3,740 | 21,029 / 21,083 | 3,738 / 3,740 |

The "missing" headings are the checker's, not the parser's: every one sits inside a fence opened with four backticks that the line-based checker closes at the first three-backtick line (Prisma's AI prompt pages and `guides/making-guides.mdx`, GitHub's Copilot tutorials), or inside TOML front matter comments (`troubleshooting/_template.mdx`). The 57 untitled Supabase files are `_partials/` included by other pages; the two GitHub Docs files have neither a title nor an H1. Before this change the same corpora gave 0 of 1,711 MDX pages a title and lost the glued headings (`realtime/authorization.mdx`, `native-mobile-deep-linking.mdx`, `storage/vector/local-development.mdx`).

## Not done

- MDX `import`/`export` in the middle of a document are text (see above).
- Components whose content lives in props (`<ApiTable rows={[…]} />`) are text: searchable raw, summarized as what they are.
- No JSX-aware stripping for the card input; if cards on component-heavy pages turn out poor in the DocsQA audit (benchmark plan rule 0.8), a "prose view" for the summarizer is the next step, and it would not change sections, hashes or line ranges.
