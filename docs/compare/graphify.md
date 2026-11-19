# markdownattractor vs graphify, on your docs

[graphify](https://github.com/safishamsi/graphify) turns code, docs and media into a knowledge graph that Claude can query. markdownattractor does one thing: it keeps a live, section-level index of your markdown, so Claude answers from your runbooks, ADRs and specs and shows the exact lines it used and when each section last changed.

They overlap on one thing, markdown docs. On docs we measured both, side by side, on the same questions with the same answering model.

## The short version

| On markdown docs | markdownattractor | graphify |
|---|---|---|
| Source tokens Claude reads per answer, at the same answer quality | **3.18× fewer** | 1× |
| From an edit to Claude seeing it | **1.7 s, automatic** (a watcher) | 28.4 s, after you run `/graphify --update` |
| Right section in the top 5, four public doc sites | **0.41 to 0.70** | 0.02 to 0.10 |
| Index build on large doc sites (GitHub Docs, Prisma) | finishes | did not finish ($73.54 and $82.77 spent, no graph) |
| What every hit carries | file, heading path, line range, last changed (from git) | graph nodes and edges |
| Model needed to start | none: raw-text search works with no key | an LLM builds the graph |

## Retrieval on public docs

Share of test questions with the expected section in the top five (success@5), DocsQA-Repo test split, scored once:

| Docs site | Questions | markdownattractor (hybrid) | graphify (`query_graph`) |
|---|---|---|---|
| Tailwind CSS | 47 | **0.702** | 0.021 |
| GitHub Docs | 87 | **0.483** | 0.034 |
| Supabase | 21 | **0.429** | 0.095 |
| Prisma | 63 | **0.413** | 0.032 |

## Tokens per answer

On the golden corpus (32 files, 16 questions, 3 runs each, graded 0–6 by a model panel), with the answer contract: answer quality level with graphify, **3.18× fewer source tokens** read and 3.04× lower cost per answer.

## Freshness

Twenty seeded edits on the Prisma docs, one machine: markdownattractor made the edited section searchable in **1.7 s** (median) and its summary card in 8.4 s, with nothing to run. graphify's update took 28.4 s and has to be triggered.

## Use both

graphify covers code, and markdownattractor does not index code at all: it is markdown only, by design. If your project has both, keep graphify (or any code index) for the code and let markdownattractor handle the prose around it. Both are MCP servers; Claude Code uses them side by side.

```
/plugin marketplace add joe-carr-data/markdownattractor
/plugin install markdownattractor@markdownattractor
```

## Method

Every number on this page is on [the benchmarks page](../benchmarks.md), with its freeze, its gates and how to regenerate it: retrieval is T1, the build is T3, freshness is T4, tokens per answer are § The answer contract. graphify's arm used its own MCP server and the graph it built (with Haiku on three sites, where the Sonnet build did not finish; with Sonnet on Supabase).
