# Launch drafts — for the owner to edit and post

Status: **in progress** (LinkedIn posted 2026-11-22; the main window is 2026-11-30 to 2026-12-02). The owner alone posts. Every number is on `docs/benchmarks.md`, with its method; if one changes there, it changes here.

**Hacker News text is the owner's own.** HN's guidelines ban generated text and automated posting, so this file holds a fact sheet for HN, not prose, and Claude never fills in or submits an HN form. Reddit, X and LinkedIn drafts are starting points for the owner's voice.

## The calendar

| Date | Channel | What | Who |
|---|---|---|---|
| 2026-11-22 | LinkedIn | Posted, with the 54 s clip and the logo cover; link and install in the first comment | owner (done) |
| by 2026-11-29 | HN | Check the account can post a Show HN; if not, take part in threads genuinely until it can | owner |
| 2026-11-29 | Friends | Ask 10–15 Claude Code users to **try it and send feedback** (no star or vote request, no HN link) | owner |
| Mon 2026-11-30 | r/ClaudeCode | "Built with Claude" text post (draft below) | Claude drafts, owner posts |
| **Tue 2026-12-01** | **Show HN** | **8:30–9:30 ET (14:30–15:30 Barcelona)**, linking the repository; stay in the thread 6–8 hours, every reply by hand | **owner** |
| Tue 2026-12-01 | X | The thread (below), about two hours after HN; no HN link, no vote ask | owner |
| Wed 2026-12-02 | r/ClaudeAI | "Built with Claude" format (below) | owner |
| Thu 2026-12-03 | r/LocalLLaMA | The local angle (below) | owner |
| Fri 2026-12-04 | r/rust | The architecture post (below); This Week in Rust PR | owner / Claude |
| Tue 2026-12-08 | HN (regular story) | The benchmark write-up on its own URL, rewritten in the owner's words | owner |
| 2026-12-09 on | Lists, newsletters | awesome-claude-code web form (eligible: older than 14 days); claude.ai/directory/manage; newsletters | owner |

Daily numbers: `scripts/dev/funnel.sh` (stars, views, visitors, referrers; GitHub keeps traffic 14 days). Checkpoints: 150 stars on HN day, 500 by 2026-12-06, 1,000 by 2026-12-13.

## The two points every post leads with (owner, 2026-11-19)

1. **Indexed as it's written.** A Rust daemon indexes every markdown file the moment it is saved, including the docs, reports and plans Claude writes itself: searchable in about 2 seconds, no indexer to run.
2. **Never lost in stale docs.** Every section carries when it was created and last changed (git history, or file times). Claude sees the dates, so it tells the current runbook from last year's, and the new decision from the one it replaced.

Asked about links between documents: "next on the roadmap" (`supersedes`, `relates_to`).

## The numbers we use

| Claim | Figure | Where |
|---|---|---|
| Save to searchable | 1.7 s (median of 20 edits); summary card in 8.4 s | benchmarks, T4 |
| Install to first search hit, no API key | 8 s (7 to 21 s over runs, one machine) | `scripts/dev/first-run.sh` |
| Right section in the top 5 | 98 % on the golden questions (88 % with no model at all) | benchmarks, golden |
| Answers grounded in cited lines | 100 % on the golden set (87.5 % before the answer contract) | benchmarks, § The answer contract |
| Against graphify, on docs | 3.18× fewer source tokens at the same answer quality; edits live in 1.7 s against 28.4 s; top-5 retrieval 0.41–0.70 against 0.02–0.10 | `docs/compare/graphify.md` |
| Cost | $0.31 to summarise a 32-file docs folder; or no key at all; or your Claude subscription | README |

Not a headline: "5× fewer tokens" is the project's goal (G3), not a measurement. Never post it.

## Hacker News fact sheet (facts only; the owner writes the text)

**Titles: plain, under 80 characters, no superlatives.** Ideas to rewrite, not to paste:
- what it is: a live, time-aware index of your markdown for Claude Code, in Rust
- what it fixes: Claude Code answering from whichever version of a doc it finds first

**Facts for the first comment**
- Why: Claude Code writes much of the documentation now (plans, reports, ADRs, runbooks); after weeks the folder holds many versions, and grep-and-read answers from whichever file came first.
- How it works: one Rust binary watches the folder, splits markdown into sections, indexes the raw text at once (SQLite FTS5), adds local embeddings and, optionally, a ~200-token summary card per section; three ranked lists fused. Every hit carries file, heading path, line range and when the section was created and last changed (git history, or file times). Claude reads the card, then opens only the lines it needs. MCP server and CLI in the same binary.
- Numbers: the table above, each with its method on `docs/benchmarks.md`; gates were written before the runs and every number regenerates from committed artifacts.
- Privacy: local index, no telemetry, never writes to the user's files; network only for summaries (their key, their local model, or their own `claude -p`) and the one-time embedding-model download.
- Install: `/plugin marketplace add joe-carr-data/markdownattractor` then `/plugin install markdownattractor@markdownattractor`; `/mda start`.
- Limits to state plainly: markdown only (no code); summaries need a key, a local model or the Claude plan (search works without); one-machine timings; links between documents not yet.
- The ask: feedback on the questions their agent gets wrong.

**Likely questions, with the facts**
- *vs qmd?* qmd is good local search. Same answering setup, golden set and `docs/`: answer quality level; mda's median source tokens lower (1,430 vs 1,908; 2,068 vs 3,831) but not every gate passed, so no savings are claimed against qmd. Differences: per-section dates, line-range citations, live watcher, summary cards. T1 retrieval: hybrid at or above qmd full on all four sites, intervals include zero.
- *vs grep?* Golden set: +0.25 answer correctness with the answer contract; grep reads fewer tokens on the 32-file golden set.
- *vs graphify?* `docs/compare/graphify.md`; use both: graphify for code, mda for the docs.
- *API key cost?* $0.31 for 32 files with Haiku; an edit redoes one section; free local model or no key at all.
- *Subscription terms?* ADR-0010, quoting Anthropic's page; opt-in, counts against plan limits.
- *Why Rust?* One binary, no runtime, the watcher and the index in one process.

## LinkedIn (posted 2026-11-22, as published)

> Claude Code writes a lot of my documentation now: plans, reports, runbooks, decision records. A few weeks in, the folder is full of versions, and Claude has no idea which one is current. It answers from whichever file it finds first.
>
> So I built markdownattractor, a free, open-source plugin for Claude Code:
>
> ⚡ Indexed as it's written. A small Rust program watches your docs folder. Every markdown file is searchable about 2 seconds after it's saved, whether you wrote it, a teammate did, or Claude did. Nothing to re-run.
>
> 🕒 Never lost in stale docs. Every section carries when it was created and last changed, read from git history (or file times when there's no git). Claude sees those dates on every hit, so it tells this week's runbook from last year's, and cites the exact lines it used.
>
> In the clip, Claude answers a question, a teammate edits the runbook, and when I just ask "can you double-check?", Claude finds the change on its own and corrects itself.
>
> It runs locally, with no telemetry. It works with no API key at all, or with your own key or your Claude subscription.
>
> If your agents write docs as fast as mine do, this is the piece that keeps them honest.
>
> 🔗 Link in the first comment.

First comment: the repository link and the two install lines.

## X thread (Tue 2026-12-01, about two hours after HN)

1. Claude writes a ton of markdown: plans, reports, runbooks, ADRs. A month later it can't tell which version is current. I built markdownattractor so it always can. 🧵 [54 s clip]
2. ⚡ A Rust daemon indexes every markdown file the moment it's saved. Yours, your teammates', Claude's own. Searchable in 1.7 s. You never run an indexer.
3. 🕒 Every section knows when it was created and last changed (from git, or file times). Claude sees the dates on every hit, so it picks this week's runbook over last year's, and cites the exact lines.
4. In the clip: a teammate changes the runbook. I only ask "can you double-check?" Claude finds the edit itself: "I was wrong. The runbook now says thirty minutes."
5. Measured: 8 s from install to first hit, no API key. 98 % right section in the top 5. 3.18× fewer tokens than graphify on docs. Free and local. github.com/joe-carr-data/markdownattractor

## r/ClaudeCode (Mon 2026-11-30, "Built with Claude", text post)

**Title:** I built a plugin so Claude Code always knows which of your docs is the latest

**Body:** First line: "I built this, it's free and open source." Then: the problem (the pile of versions Claude writes), the two points, the clip, three numbers (1.7 s, 98 %, 8 s with no key), the install lines, the limits (markdown only; summaries need a key, a local model or the plan), and one question: "what question does your agent get wrong on your own docs?"

## r/ClaudeAI (Wed 2026-12-02, "Built with Claude" flair)

The flair asks for what you built, how you built it, a demo and at least one prompt you used. **What:** the two points. **How:** built entirely with Claude Code; every Rust change reviewed by a second model; benchmarks preregistered in the repository. **Demo:** the 54 s clip. **Prompt:** "Can you double-check?" (the clip's second question), and the one the owner used most while building.

## r/LocalLLaMA (Thu 2026-12-03)

Angle: runs fully local. Raw-text search with no model at all (88 % right section in the top 5 on the golden set); local embeddings; summary cards from a llama.cpp server (`/mda backend local`, `docs/guides/local-model.md`); no telemetry; never writes to your files. Lead with the local setup, not with Claude.

## r/rust (Fri 2026-12-04)

Technical, not marketing: one binary as watcher, index, MCP server and CLI; `notify` events treated as hints and checked against the filesystem; hash-before-summarise so unchanged sections never reach a model; SQLite FTS5 + local embeddings (`fastembed`) + time, fused; per-root daemon with an OS file lock. Ask for review of the watcher and the differ. Same week: a This Week in Rust PR with the post.

## Launch write-up (2026-12-08, its own URL)

1. The pile: how much markdown Claude writes, and what happens a month later.
2. The clip, frame by frame: answer, edit, "can you double-check?", self-correction.
3. How it works: sections, hash before summarise, raw text indexed first, times from git, three ranked lists fused.
4. How we measured: gates written before the runs, a model panel as graders, every number regenerable.
5. The numbers (the table above).
6. Next: links between documents (`supersedes`, `relates_to`).

## Rules for every channel

- Never ask for upvotes, on any site, from anyone; never share the HN link with a call to action; no second accounts; no identical cross-posts.
- One modest star line at the end of a GitHub-facing text at most.
- The graphify crowd: "graphify for the code, markdownattractor for the docs"; answer real questions; no unsolicited messages to its stargazers.

## Before posting (owner)

- HN account able to post a Show HN.
- Read each subreddit's sidebar the day before (rules change).
- Restart your Claude Code sessions so they run the latest release.
