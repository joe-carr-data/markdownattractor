# Launch drafts (2026-11-18) — for the owner to edit and post

Status: **drafts**; the owner decided on 2026-11-18 to launch with the product of Phases A and B. Nothing here is posted; the owner alone posts (growth plan, Phase 2). Every number below is on `docs/benchmarks.md` with its method and limits; if a number changes there, it changes here. Nothing claims what did not pass a gate.

## What passed, and what did not (the source for every text below)

| | Result | Where |
|---|---|---|
| Passed | A first search with no credentials, from nothing installed, in 7 to 21 seconds on one machine (8 s on v0.2.9; limit 120 s) | plan 2026-11-v0.3, Phase B; `scripts/dev/first-run.sh` |
| Passed | Search without any model: the expected section is in the top 5 for 88 % of the golden questions (raw text only) | plan, Phase B |
| Passed | Save to searchable: raw text p50 1.7 s, card p50 8.4 s (one machine, 20 edits) | benchmarks, T4 |
| Passed, with limits | On the small golden corpus, against grep: +0.25 answer correctness, no saving in tokens read; against graphify: 3.18× fewer source tokens read | benchmarks, § The answer contract |
| Passed on one set, missed on the other | Every claim cited to opened lines: 100 % of answers grounded on the golden set (control 87.5 %); on the project's own docs one answer below the control, so the trust claim is not the headline | benchmarks, § The answer contract |
| Did not pass | No arm, mda included, passed the grounding gate on the four public documentation projects | benchmarks, § Where we lose |
| Did not pass, not shipped | Change-impact review: right on 51 % of its alerts against 87 % for an agent with file search and git; stopped | benchmarks, § Change-impact review |

## Show HN — title

> Show HN: Markdownattractor – Claude Code answers from your docs and cites the exact lines

Alternative, plainer:

> Show HN: A local index of your markdown for Claude Code, with line-level citations

## Show HN — first comment (draft)

I keep ADRs, runbooks and notes in markdown, and Claude Code answers questions about them by grepping and reading whole files. It works, but I could not tell which lines an answer came from or how old they were.

markdownattractor is a Claude Code plugin: one Rust binary that watches a folder, splits every markdown file into sections, and keeps a local index (SQLite full-text search, plus optional summaries and embeddings). Claude searches the index, opens only the lines it needs, and every claim in the answer carries the file, the line range, and when that section last changed (from git history when there is one).

What I can show with numbers, all on the benchmark page with the method:

- From nothing installed to a first useful search: 7 to 21 seconds, with no API key. Search runs on raw text; summaries are optional and use your own key or a local model.
- A saved file is searchable in about 2 seconds.
- On a small test corpus it answers more correctly than grep-and-read, at the same reading cost. On large public documentation sets it does not beat a good baseline on every axis, and the page says where.

What did not work, also on the page: I built a feature that, after you change one document, names other documents the change left out of date. I evaluated it on real commits from public documentation repositories against a plain agent with file search and git. The plain agent was right 87 % of the time; my feature 51 %, at twice the cost. It is not shipped.

It never writes to your files, has no telemetry, and the index stays on your machine. Apache-2.0.

Install, inside Claude Code:

    /plugin marketplace add joe-carr-data/markdownattractor
    /plugin install markdownattractor@markdownattractor

I would like to hear where the citations are wrong or the dates misleading: that is the part I most want to be right.

## X thread (draft, 5 posts)

1. Claude Code reads your docs by grepping and opening whole files. I wanted answers that say which lines they came from and how old those lines are. So I built a local index for markdown. [demo clip]
2. [the clip belongs here if post 1 carries the session image instead] One Rust binary watches the folder. Every section is searchable about 2 seconds after you save. No key needed to start: first useful search 7 to 21 seconds from nothing installed.
3. Every claim in an answer is cited to a file and a line range, with when that section last changed (from git history when there is one).
4. I benchmarked it against grep, qmd and graphify, with the gates written down before the runs. It does not win everywhere, and the page says where it loses.
5. One feature did not make it: warning you when a change leaves another doc out of date. A plain agent with search and git beat it, 87 % to 51 %. Not shipped; the numbers are public. [link]

## r/ClaudeAI — "Built with Claude" (draft)

Title: I built a Claude Code plugin that cites the exact lines of your markdown it answered from

Body: the first comment above, minus the Show HN framing, plus one paragraph on how it was built: the whole project was written with Claude Code, every change to the Rust code reviewed by a second model, and the benchmark runbook is in the repository so the numbers can be regenerated.

## Launch write-up — outline (for the repository or a blog)

1. The problem in one screen: a question, the grep-and-read session, what is missing (which lines, how old).
2. What it does: the session excerpt from the README.
3. How it works: sections, hash before summarise, raw text indexed first, cards optional, times from git.
4. The benchmark method in five lines: preregistered gates, frozen samples, a model panel in place of human graders, what that cannot show.
5. What passed (the table above), what did not, and the change-impact review story with its lesson: run the cheap baseline first.
6. What is next, without promises.

## The two points every post leads with (owner, 2026-11-19)

1. **Indexed as it's written.** A Rust daemon indexes every markdown file the moment it is saved, including the docs, reports and plans Claude writes itself: searchable in about 2 seconds, no indexer to run.
2. **Never lost in stale docs.** Every section carries when it was created and last changed (git history, or file times); Claude sees the dates, can search the last week and ask for a timeline, so it tells the current runbook from last year's and the new decision from the one it replaced.

(Not a claim: automatic "this supersedes that" links are not built; Claude infers it from the dates.)

## Launch tactics (Codex marketing pass, 2026-11-19)

- **Show HN:** title "Show HN: Markdownattractor lets Claude Code cite the runbook lines behind an answer". In the first comment, paste the real question, tool calls and cited answer (the README session), then the no-key install so readers can try it on their own docs.
- **X:** open with "My teammate just changed the runbook. Watch what Claude says when I ask what changed." Upload the full 54 s clip natively; end with the repo link and "Star it for your next on-call shift."
- **r/ClaudeAI ("Built with Claude"):** post the rollback example and invite readers to try a question their agent routinely gets wrong and share the hit; with permission, quote a reader's result in a follow-up.
- **The graphify crowd:** lead with "use it next to graphify: graphify for the code, markdownattractor for the docs", and link [the comparison page](../compare/graphify.md) when someone asks why (on docs: 3.18× fewer tokens, edits live in 1.7 s, success@5 0.41–0.70 against 0.02–0.10). No unsolicited messages to its stargazers; answer real questions in threads, and one open note to its maintainer suggesting a "works well with" mention.
- GitHub topics set 2026-11-19: claude-code, mcp, knowledge-graph, markdown, rag.
- README hook (live): "Edit the runbook. Ask Claude what changed." over the 36 s clip.

## Open before posting (owner)

- **The launch clip is the second recording** (2026-11-18, v0.2.9, 54 s, captioned, ending on a "How it works" card and a "Measured" card with the figures of the table above): Claude answers with its source, a teammate's terminal edits the runbook, Claude is asked what changed and reports the section, the time and the new text (`docs/demo/README.md`, video 3). The owner found the first clip had no hook (it looks like grep); it stays as the clip for the post about working without a key.
- The first demo clip is recorded (2026-11-18, v0.2.8): an mp4 for the X thread and a GIF, kept outside the repository (the commit gate refuses binary files; `docs/demo/README.md` says how to record it again); the README shows the same session as text. Watch it once before posting. It shows the freshness loop in a terminal, not a Claude session; post 3 of the thread (citations) has no clip and can use the README's session excerpt as an image.
- Account age and karma on HN and r/ClaudeAI (growth plan, Phase 0).
- Whether "more correct than grep on a small corpus" goes in the title line or only in the body (owner decision 2026-11-16 allows the claim with its limits).
