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
2. **L1 — choose sections from the hits.** Each hit carries `tldr` (or a raw `snippet` when the section has no card yet), `heading_path`, `line_start`/`line_end`, `token_estimate` (what `mda_open` will cost), `updated_at` with `updated_source`, and `section_id`. `updated_at` is the index's recorded time the section last changed, taken from git history or from the file's modification time; it is not a date the document declares, and not proof that the information is still current. The tldr tells you **where to look**; fetch the full card (`mda_card(section_id)` / `mda --json card <section_id>`) only when a hit is not enough to choose. Hits and cards are pointers, not sources.
3. **L2 — open the exact lines.** `mda_open(section_id)` / `mda --json open <section_id>` returns the source lines for that section only (typically 20–60 lines), with its `updated_at` and `updated_source`. Open the smallest set of sections that supports the answer and stop when the requested claims are supported. **Every claim about what the documents say comes from lines you opened**, never from memory of a tldr or a card. If the response has `stale: true`, the file changed after indexing; the lines you got are the current ones, but re-run the search if the answer looks off.
4. **Read the whole file** only when the user explicitly asks for it, or the card says the document is small (≲ 60 lines).

## The answer contract

- **Document content: file and lines next to each claim**, from lines you opened: `runbooks/deploy.md:11-13`.
- **Index facts are reported as the index's**: counts, events and times from `mda_status`, `mda_recent`, `mda_timeline` and `updated_at` are what the index reports; name them as such and do not attach a source-line citation to them.
- **Dates**: a date written in a document is cited to its lines. An index date is labelled separately, as "index last changed <date> (git or file time)". For a section you opened, use the open result's `updated_at` and `updated_source`; they supersede the search hit's. Never state a date that is in neither.
- **Nothing about a section you did not open**: not what it says, not what it omits, not what it implies. Say what you did not check instead of filling the gap.
- **No ranking or generalising beyond the lines**: one stated cause is not "the usual cause"; listed reasons are not ranked unless the page ranks them.

## Recovery rule

If the top hits don't contain the answer:

- retry the search with `raw=true` / `--raw` (lexical only, over the full section text: catches config values, error strings, identifiers that a summary would omit);
- then, and only then, fall back to `Grep` over the source files, and **say so** ("the index didn't surface this, so I grepped"). A silent miss is the one failure mode this workflow must never have.

After a grep fallback, read the relevant source lines and cite them like any other claim. If the opened lines and the fallback still do not answer the question, say what you checked and what remains unanswered; do not conclude that the whole corpus omits it.

Hits marked `pending` come from sections that are raw-searchable but not yet summarized; open them rather than waiting.

## What not to do

- Don't `Read` a markdown file whole to "check" a card. Open the section.
- Don't answer from a tldr or a card. Open the section and cite its lines.
- Don't turn `updated_at` into a claim about the content. "The index last changed this section in March" is a fact; "this is out of date" or "this is current" is an inference the lines must support.

## Time questions

- "What changed since Monday?" → `mda_timeline(since="…")` / `mda timeline --since …`, grouped by day, with renames and deletes.
- "What did we work on lately?" → `mda_recent(n=10)` / `mda recent`.
- "Is the index up to date?" → `mda_stale()` / `mda stale`: empty means every section has a card and every file matches its index.
