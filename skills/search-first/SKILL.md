---
name: search-first
description: How to answer questions from a project's markdown (ADRs, runbooks, specs, notes) when a markdownattractor index exists — search the index, read a card, open only the lines you need, and fall back safely. Use before reading or grepping any .md file whole.
user-invocable: false
paths: "**/*.md, **/*.markdown, **/*.mdx"
---

# Search first, read less

A markdownattractor index exists for this project when `.markdownattractor/index.sqlite` is present at the project root. When it is, answer questions from markdown in this order. Each step is cheaper than the next; stop as soon as you have what you need.

The plugin also serves these as MCP tools (`mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_recent`, `mda_stale`, `mda_status`). Prefer the tool when it is available; the CLI form below is the same call.

1. **L0 — search.** `mda_search(query="<the question, as the user would type it>")` (5 hits; pass `k` only when the first five miss) or `"${CLAUDE_PLUGIN_ROOT}/scripts/mda" --json search "<question>" -k 5`. Add `since="7d"` (`--since 7d`) when the question is about recent changes, `path_prefix` (`--in <path>`) when the user named a folder. Hits are fused from cards, raw text and card vectors; `matched` says which. `partial: true` means no section matched every term and the hits are OR matches.
2. **L1 — read the card.** Each hit carries `tldr` (or a raw `snippet` when the section has no card yet), `heading_path`, `line_start`/`line_end`, `token_estimate` (what `mda_open` will cost), `updated_at`, and `section_id`. For most questions the tldr plus one card (`mda_card(section_id)` / `mda --json card <section_id>`) is enough to answer, and it tells you how old the information is. Say the date when it matters.
3. **L2 — open the exact lines.** `mda_open(section_id)` / `mda --json open <section_id>` returns the source lines for that section only (typically 20–60 lines). Quote from these, never from memory of the card. If the response has `stale: true`, the file changed after indexing; the lines you got are the current ones, but re-run the search if the answer looks off.
4. **Read the whole file** only when the user explicitly asks for it, or the card says the document is small (≲ 60 lines).

## Recovery rule

If the top hits don't contain the answer:

- retry the search with `raw=true` / `--raw` (lexical only, over the full section text: catches config values, error strings, identifiers that a summary would omit);
- then, and only then, fall back to `Grep` over the source files, and **say so** ("the index didn't surface this, so I grepped"). A silent miss is the one failure mode this workflow must never have.

Hits marked `pending` come from sections that are raw-searchable but not yet summarized; open them rather than waiting.

## What not to do

- Don't `Read` a markdown file whole to "check" a card. Open the section.
- Don't paraphrase dates or decisions from a card without the line range next to them.
- Don't ignore `updated_at`. "Deprecated in March" and "current as of last week" are different answers.

## Time questions

- "What changed since Monday?" → `mda_timeline(since="…")` / `mda timeline --since …`, grouped by day, with renames and deletes.
- "What did we work on lately?" → `mda_recent(n=10)` / `mda recent`.
- "Is the index up to date?" → `mda_stale()` / `mda stale`: empty means every section has a card and every file matches its index.
