# Growth and launch plan (2026-11)

Date: 2026-11-10. Owner: Joe Carr. Status: proposed, awaiting the owner's go.

Sources: a Codex strategy pass (fresh thread, read-only, 2026-11-10) and an external research report the owner commissioned through Claude Desktop (2026-11-10, not committed; its key figures are restated below with their sources' names). Every figure in this plan is the research's, not a forecast of ours.

## Goal

Turn a released, rigorously benchmarked plugin with 0 stars and no users into a tool with real, returning users, and give it the best available chance of a breakout. The owner's target is 50,000 GitHub stars. Both reviews agree that 50K is a long-shot outcome over 12–24 months, not a 90-day plan. This plan is judged on users and on the funnel; stars are the lagging signal.

## What the evidence says (the short version)

- **Base rates.** The median Show HN gets 2 points. A successful one (258+ points) brings a median of about 509 stars on day 1, and 92% of the effect is over within 48 hours (Daniel King's analysis of 188,085 Show HN posts). An arXiv study of 138 AI repositories found an average of 121 stars in 24 hours and 289 in a week.
- **Comparables.** The solo, no-audience projects nearest to us reached 19K–38K stars over 17–28 months (Repomix, claude-code-router, ccusage). Each supports many agents or is about context and cost. graphify (≈145K) shipped within 48 hours of a Karpathy post: a catalyst we can prepare for but not schedule. qmd (≈31K) had a famous author.
- **Realistic checkpoints** (research's base case): 300–2,000 stars in 90 days with a well-run launch; 2K–10K in 12 months with multi-client support and steady releases.
- **Biggest levers:** support every MCP client, not only Claude Code; a 20–40 s demo at the top of the README; one concentrated launch window so stars land together and can reach GitHub Trending; the directories and registries; frequent releases with a post each.
- **Our honesty rule is a selling point.** Competitors lead with token multipliers that independent re-tests contradicted. We publish only what passed its gates.
- **Never** buy, trade or reward stars (GitHub's Acceptable Use Policies; the ICSE '26 fake-star study found a short-lived effect and a long-term liability).

## Positioning

- **Hook** (to test): "Claude answers from your ADRs and runbooks, and shows you the exact lines it used and when each section last changed. Local, no telemetry." (Amended 2026-11-10: "when they were written" is not true today; see the file-times task.)
- **Claims we may make:** answer quality on par with grep and qmd in preregistered benchmarks (scoped as on the page); save-to-searchable-summary p50 8.4 s (T4, passed); every hit carries file, line range and dates; local only; never writes to your files; raw text is searchable before any summary exists.
- **Claims we may not make:** any token or cost saving as a headline (no pair passed the gates); "nobody else does time"; anything about T5 temporal superiority. Source tokens read may appear only labelled in plain words as a reported, not-yet-gated observation.
- **How it runs:** your own Anthropic API key (the default) or a free local model. We do not market "runs on your Claude plan": the product's own warning says Anthropic's terms do not permit third-party tools to do that (ADR-0002).
- **Name:** keep "markdownattractor". The evidence says the hook matters more than the name, and renaming has costs.

## Who does what

The assistant prepares everything in the repository: code, docs, demo scripts, recordings it can produce, drafts of every post. **The owner alone posts, submits and sends**: Hacker News, X, Reddit, LinkedIn, creator messages, the Claude plugin directory (needs the owner's paid plan) and the awesome-claude-code issue form. No account action is taken on the owner's behalf.

## Tasks

### Phase 0 — Make the first visit convert (weeks 1–2, from 2026-11-16)

- [ ] **Launch blocker: four security advisories** published after the last merge fail `cargo deny` on `main` (2026-11-10): RUSTSEC-2026-0459 (the bundled SQLite in `libsqlite3-sys` 0.38.2; fixed in ≥ 0.39.0) and RUSTSEC-2026-0457/0461/0462 (`rmcp` 3.4.0; fixed in ≥ 3.9.0). Upgrade in its own PR with a Codex review; Dependabot #51 may cover part of it. No launch with a red dependency audit.
- [x] **Launch blocker (diagnosed 2026-11-10): a failing `markdownattractor` MCP server.** The installed plugin's server was connected all along; the failure was the repository's root `.mcp.json`, read as a *project* server whenever Claude Code runs inside a clone of this repository, where `${CLAUDE_PLUGIN_ROOT}` is unset. It hit contributors, not users. Fixed by declaring the server inline in `plugin.json` (PR #58), verified with a marketplace install from the checkout under an isolated config.
- [ ] Clean-machine install check (macOS and Linux) from the marketplace, timed to the first useful answer; target under 5 minutes, with raw search answering before cards finish.
- [x] README rewrite (PR #59, 2026-11-10): the hook first; the demo GIF; the verified install path and prerequisites; a comparison table against grep, qmd and graphify using only gated numbers; a link to the benchmark page. Remove "Not released yet", the "v0 design" badge, "40K tokens", "nobody else" and the sqlite-vec mention (ADR-0004 stores vectors in plain SQLite).
- [x] Marketplace description: drop "no API key"; say "your own API key or a free local model" (PR #58).
- [x] A public demo corpus: `scripts/demo/make-corpus.py DEST` copies the fictional golden set (never modifying it) and sets realistic file times (dated file names keep their date; runbooks days old, ADRs one to two years). mda reads file times, not git, so no history is needed. Three demo questions still to pick.
- [ ] Multi-client setup docs for the existing `mda mcp` server: Codex, Cursor, Gemini CLI, opencode, Claude Desktop. Each verified end to end on this machine before it is documented; the summarizer backend requirement stated plainly.
- [ ] Demo video 1, "Where did that answer come from?" (25–35 s, silent, captioned): a question, the hit with file, lines and dates, Claude opening only those lines, the install line. Recorded with VHS (to install) or a screen recorder; script and tape committed under `docs/demo/`.
- [ ] Demo video 2, "Edit, searchable in 8 s" (15–20 s, split screen with a timer; caption "p50 8.4 s in our benchmark").
- [ ] Owner: start commenting on HN and r/ClaudeAI now, so the accounts are not new at launch (HN currently restricts Show HN from new accounts; r/ClaudeAI reportedly needs karma for showcase posts).
- [ ] Funnel measurement without telemetry: a script that records GitHub traffic (views, unique visitors, referrers, clones), stars and release download counts daily to a local file, so each launch step can be read against the funnel.

### Phase 1 — Directories and first users (weeks 3–4)

- [ ] MCP Registry entry (`mcp-publisher`; namespace `io.github.joe-carr-data/…`; the registry hosts metadata only, so the Rust binary needs OCI or MCPB packaging — decide and build it).
- [ ] Owner: submit to the Claude plugin directory (claude.ai/directory/manage).
- [ ] Owner: 5–8 personal invitations to people with large, frequently edited markdown collections. The assistant drafts the message; watch at least three of them install. Success: five activate, three still use it after 14 days and can say why.
- [ ] **Product: times from git history** (needs an ADR; a schema change). mda dates documents and sections by filesystem time only (`pipeline::file_times`: mtime, birth time or first-seen). In a fresh clone every file starts at the clone's time, so "when it was written" and "changed 2 years ago" are unavailable on exactly the repositories users clone. Proposal (owner, 2026-11-10: "not everything is gonna be a git repo" — both sources): git when available, filesystem otherwise, decided per file. A folder that is not a git work tree keeps today's file times. Inside a work tree, a committed, unmodified file takes its created time from its first commit and each section's changed time from the last commit that touched its lines; an untracked file, or one with uncommitted edits, uses file times for what git cannot date (an edited section is "changed" at its mtime). Every stored time records its source (`git` or `filesystem`), as `created_at_source` already does, and hits can show it. This is what makes the time story true for real users; owner decision whether it ships before launch.
- [ ] **Search: the recency prior was never measured on realistic file ages.** Every benchmark corpus was a fresh checkout or copy, so all files had the same age and the prior was neutral. On the demo corpus with realistic ages, "why did we choose postgres" drops ADR 0001 from 2nd to 4th behind a 2-day-old runbook section (half-life 30 days; old sections keep at least half weight). Measure on a corpus with real ages before launch (T1's method with git-derived times, or the demo corpus with labelled questions); consider exempting decision records or lengthening the half-life.
- [x] (released 2026-11-10, PR #64 + tag; hook download and marketplace install verified) Release **v0.2.0** (not 0.1.2: new features and a schema migration make it a minor release; the version PR merges together with the tag, never before it) before any first user: installed v0.1.1 predates every change since 2026-09-22, including the leaner MCP search payload (seen 2026-11-10: the shipped server still returns scores and vector diagnostics). Then fix the largest onboarding obstacles first users hit, in v0.1.3 if needed.
- [ ] Owner: submit to awesome-claude-code through its issue form once 14 days of active development are visible (a description, not a pitch).
- [ ] A launch write-up for the blog or the repository: what it does, the demo, the benchmark method and what did not pass.

### Phase 2 — The launch window (week 5)

- [ ] Drafts ready: the Show HN title and first comment (backstory, demo, benchmark link, what did not pass), the X thread with video 1, the r/ClaudeAI "Built with Claude" post.
- [ ] Owner: Show HN near Monday 00:00 UTC; the X thread the same day; r/ClaudeAI 24 hours later. Answer every comment for 48 hours.
- [ ] Record the funnel for the window: HN points, visits, stars per day against Trending, installs (release downloads).

### Phase 3 — Sustain (weeks 6–13)

- [ ] Owner: r/rust (the Rust-binary angle) and r/ObsidianMD (the vault angle) on separate days, after reading each sidebar; a This Week in Rust Crate of the Week nomination.
- [ ] v0.2 within three weeks of launch with the launch feedback; a changelog post per release.
- [ ] Demo video 3, "Same question, three tools" (60–90 s): grep, qmd and markdownattractor on the public benchmark repository, gate status labelled on screen, ending with "rerun it yourself".
- [ ] Good-first-issues and a short contributor guide.
- [ ] Owner: 5–10 personal messages to Claude Code, MCP and Obsidian creators, each offering the demo corpus; no request to star.
- [ ] v0.3 with one integration users asked for (for example an Obsidian-vault guide); a second HN post only for a major change.
- [ ] Be ready for a catalyst: when a public conversation on agent memory, LLM knowledge bases or context engineering starts, the clearest working demo goes into it the same day.
- [ ] Evidence for the grounding decision: a real demo session on the golden set (2026-11-10) answered correctly with line citations but added one unsupported sentence about a page it never opened (it said ADR 0002 omits the 30-minute limit; line 12 states it). The same embellishment pattern T6 measured; the search-first skill should require opening a section before making a claim about it.
- [ ] Week 13 retrospective against the funnel; decide whether to pursue a token-savings claim (the grounding fix in the skill or card prompt, then a T6 public rerun under a new freeze).

## Paused

The owed benchmark reruns (T2, T4, T5, T6 public) and the rubric calibration are paused for this plan's duration, except the grounding fix, which is a week-13 decision. Pausing them changes no published number.

## Exit criteria (week 13, about 2026-02-15)

- [ ] A new user on a clean machine reaches a useful answer in under 5 minutes from the README alone.
- [ ] The README, marketplace text and every launch post make only claims listed under "Claims we may make".
- [ ] `mda mcp` documented and verified with at least three MCP clients besides Claude Code.
- [ ] Listed in the MCP Registry and submitted to the Claude plugin directory and awesome-claude-code.
- [ ] One launch window run, with its funnel recorded.
- [ ] At least five people outside the author used it for 14 days or more.
- [ ] A retrospective in `docs/` that reads stars, visits, installs and returning users against the research's base case (300–2,000 stars in 90 days) and states the next 90 days.

## Decision rule

Under 1K stars after a well-run launch: read the funnel before adding features. Few visits means distribution. Visits without stars means positioning. Installs without continued use means product value or friction.
