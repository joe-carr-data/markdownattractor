# Launch drafts — for the owner to edit and post

Status: **ready to post** (rewritten 2026-11-21 around the owner's two points). The owner alone posts. Every number is on `docs/benchmarks.md`, with its method; if one changes there, it changes here.

## The two points every post leads with (owner, 2026-11-19)

1. **Indexed as it's written.** A Rust daemon indexes every markdown file the moment it is saved, including the docs, reports and plans Claude writes itself: searchable in about 2 seconds, no indexer to run.
2. **Never lost in stale docs.** Every section carries when it was created and last changed (git history, or file times). Claude sees the dates, so it tells the current runbook from last year's, and the new decision from the one it replaced.

Let the dates do the work: "Claude sees which version is newer" lands harder than any feature name, and it is exactly what the clip shows. Asked about links between documents: "next on the roadmap" (`supersedes`, `relates_to`).

## The numbers we use

| Claim | Figure | Where |
|---|---|---|
| Save to searchable | 1.7 s (median of 20 edits); summary card in 8.4 s | benchmarks, T4 |
| Install to first search hit, no API key | 8 s (7 to 21 s over runs, one machine) | `scripts/dev/first-run.sh` |
| Right section in the top 5 | 98 % on the golden questions (88 % with no model at all) | benchmarks, golden |
| Answers grounded in cited lines | 100 % on the golden set (87.5 % before the answer contract) | benchmarks, § The answer contract |
| Against graphify, on docs | 3.18× fewer source tokens at the same answer quality; edits live in 1.7 s against 28.4 s; top-5 retrieval 0.41–0.70 against 0.02–0.10 | `docs/compare/graphify.md` |
| Cost | $0.31 to summarise a 32-file docs folder; or no key at all; or your Claude subscription | README |

## The clip

- README: 36 s cut (take 6, v0.2.10). Posts: the full 54 s clip, `mda-clip/markdownattractor-54s.mp4` on the owner's Desktop (not in the repository).
- What it shows: Claude answers "ten minutes" with `runbooks/deploy.md:5-9`; the runbook is saved with "thirty minutes" (badge: "saved by a teammate, or by Claude itself"); Claude is only asked "Can you double-check?" and finds the edit itself: "I was wrong. The runbook now says thirty minutes… the file changed between my two reads". Then the "How it works" and "Measured" cards.

## Show HN

**Title** (pick one):

> Show HN: Markdownattractor – Claude Code always knows which doc is the latest

> Show HN: A live, time-aware index of your markdown for Claude Code (Rust)

**First comment:**

Claude Code writes a lot of markdown for me: plans, reports, ADRs, runbook updates. After a few weeks the folder is full of versions, and when I ask a question Claude greps, reads a few files whole, and answers from whichever one it found first. It has no idea which is current.

markdownattractor fixes both halves:

- **Indexed as it's written.** One Rust binary watches the folder and splits every markdown file into sections. A save is searchable in 1.7 seconds, whether you wrote it, a teammate did, or Claude did. There is no indexer to run, ever.
- **Never lost in stale docs.** Every section knows when it was created and last changed, from git history (or file times). Claude gets the file, the line range and the date with every hit, so it can tell this week's runbook from last year's, and it cites the exact lines it used.

The clip in the README shows it: Claude answers, a teammate changes the runbook, and when I just ask "can you double-check?", Claude finds the edit on its own and corrects itself, with the new lines and the time of the change.

Numbers, with the method on the benchmark page:
- 8 seconds from install to the first search hit, with no API key.
- The right section in the top 5 for 98 % of test questions.
- Against graphify on docs: 3.18× fewer tokens read per answer at the same quality, and edits live in 1.7 s instead of 28 s.

Summaries are optional: your own API key ($0.31 for a 32-file docs folder), a free local model, or your Claude subscription. Local index, no telemetry, never writes to your files. Apache-2.0.

    /plugin marketplace add joe-carr-data/markdownattractor
    /plugin install markdownattractor@markdownattractor

I'd love to hear what you'd point it at first.

## X thread

1. Claude writes a ton of markdown: plans, reports, runbooks, ADRs. A month later it can't tell which version is current. I built markdownattractor so it always can. 🧵 [54 s clip]
2. ⚡ A Rust daemon indexes every markdown file the moment it's saved. Yours, your teammates', Claude's own. Searchable in 1.7 s. You never run an indexer.
3. 🕒 Every section knows when it last changed (from git). Claude sees the dates on every hit, so it picks this week's runbook over last year's, and cites the exact lines.
4. In the clip: a teammate changes the runbook. I only ask "can you double-check?" Claude finds the edit itself: "I was wrong. The runbook now says thirty minutes."
5. Measured: 8 s from install to first hit, no API key. 98 % right section in the top 5. 3.18× fewer tokens than graphify on docs. Free and local. Star it for your next on-call shift ⭐ github.com/joe-carr-data/markdownattractor

## r/ClaudeAI — "Built with Claude"

**Title:** I built a Claude Code plugin so Claude always knows which of your docs is the latest

**Body:** the Show HN first comment, then:

> How it was built: entirely with Claude Code, with every Rust change reviewed by a second model and the benchmarks preregistered in the repo so anyone can regenerate them.
>
> Try it on your own docs folder and tell me the question your agent usually gets wrong. I'll share the best ones.

## Launch write-up (outline, for the repository or a blog)

1. The pile: how much markdown Claude writes, and what happens a month later.
2. The clip, frame by frame: answer, edit, "can you double-check?", self-correction.
3. How it works: sections, hash before summarise, raw text indexed first, times from git, three ranked lists fused.
4. How we measured: gates written before the runs, a model panel as graders, every number regenerable.
5. The numbers (the table above).
6. Next: links between documents (`supersedes`, `relates_to`).

## Tactics

- **Show HN:** post Tuesday to Thursday, morning US Eastern; stay in the thread for the first two hours and answer every comment.
- **X:** upload the 54 s clip natively in post 1; pin the thread.
- **r/ClaudeAI:** the "Built with Claude" flair; invite readers' hardest questions and quote the good results in a follow-up, with permission.
- **The graphify crowd:** "use it next to graphify: graphify for the code, markdownattractor for the docs"; link `docs/compare/graphify.md` when someone asks why. Answer real questions in threads; one open note to its maintainer suggesting a "works well with" mention. No unsolicited messages to its stargazers.
- **Discoverability:** GitHub topics set (claude-code, mcp, knowledge-graph, markdown, rag); submit to the Claude Code plugin directory and the awesome-claude-code lists.

## Before posting (owner)

- Account age and karma on HN and r/ClaudeAI.
- Restart your Claude Code sessions so they run v0.2.10.
