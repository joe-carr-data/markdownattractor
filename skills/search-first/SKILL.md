---
name: search-first
description: How to answer questions from a project's markdown (ADRs, runbooks, specs, notes) when a markdownattractor index exists — search the index, read a card, open only the lines you need, and fall back safely. Use before reading or grepping any .md file whole.
user-invocable: false
paths: "**/*.md, **/*.markdown, **/*.mdx"
---

# Search first, read less

A markdownattractor index exists for this project when `.markdownattractor/index.sqlite` is present at the project root. When it is, answer questions from markdown in this order. Each step is cheaper than the next; stop as soon as you have what you need.

The plugin also serves these as MCP tools (`mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_recent`, `mda_stale`, `mda_status`). Prefer the tool when it is available; the CLI form below is the same call.

1. **L0 — search.** `mda_search(query="<the question, as the user would type it>")` (5 hits; pass `k` only when the first five miss) or `"${CLAUDE_PLUGIN_ROOT}/scripts/mda" --json search "<question>" -k 5`. Add `since="7d"` (`--since 7d`) when the question is about recent changes, `path_prefix` (`--in <path>`) when the user named a folder. Hits are fused from cards, raw text and card vectors; `matched` names the index that matched (`cards`, `raw`, `both`, or `vector` alone). `partial: true` means no section matched every term and the hits are OR matches.
2. **L1 — read the card.** Each hit carries `tldr` (or a raw `snippet` when the section has no card yet), `heading_path`, `line_start`/`line_end`, `token_estimate` (what `mda_open` will cost), `updated_at` with `updated_source` (`git`: the authored time of the last commit that touched those lines; `filesystem`: when the file was last saved), and `section_id`. The tldr and the card (`mda_card(section_id)` / `mda --json card <section_id>`) tell you **where to look** and how old the information is; they are pointers, not sources.
3. **L2 — open the exact lines.** `mda_open(section_id)` / `mda --json open <section_id>` returns the source lines for that section only (typically 20–60 lines), with its `updated_at` and `updated_source`. **Every claim in your answer comes from lines you opened**, never from memory of a tldr or a card. If the response has `stale: true`, the file changed after indexing; the lines you got are the current ones, but re-run the search if the answer looks off.
4. **Read the whole file** only when the user explicitly asks for it, or the card says the document is small (≲ 60 lines).

## The answer contract

- **File and lines next to each claim**: `runbooks/deploy.md:11-13`.
- **Dates**: when age matters, give `updated_at` as the index's "last changed" date and name its source (git or file time). Never present it as something the document says, and never state a date that is not in a tool result or in the opened lines.
- **Nothing about a section you did not open**: not what it says, not what it omits, not what it implies. Say what you did not check instead of filling the gap.
- **No ranking or generalising beyond the lines**: one stated cause is not "the usual cause"; listed reasons are not ranked unless the page ranks them.

## Recovery rule

If the top hits don't contain the answer:

- retry the search with `raw=true` / `--raw` (lexical only, over the full section text: catches config values, error strings, identifiers that a summary would omit);
- then, and only then, fall back to `Grep` over the source files, and **say so** ("the index didn't surface this, so I grepped"). A silent miss is the one failure mode this workflow must never have.

Hits marked `pending` come from sections that are raw-searchable but not yet summarized; open them rather than waiting.

## What not to do

- Don't `Read` a markdown file whole to "check" a card. Open the section.
- Don't answer from a tldr or a card. Open the section and cite its lines.
- Don't ignore `updated_at`. "Deprecated in March" and "current as of last week" are different answers.

## Time questions

- "What changed since Monday?" → `mda_timeline(since="…")` / `mda timeline --since …`, grouped by day, with renames and deletes.
- "What did we work on lately?" → `mda_recent(n=10)` / `mda recent`.
- "Is the index up to date?" → `mda_stale()` / `mda stale`: empty means every section has a card and every file matches its index.
