# Demo videos

Every video is a **real session on the public demo corpus**, recorded from a committed script. Nothing is staged or generated: a fabricated demo would undercut the product's one claim, that you can check where an answer came from. The model's wording differs between takes, so each take is watched before it is used.

## Setup (once)

```bash
brew install vhs                                   # also needs ffmpeg, claude (logged in), mda
scripts/demo/make-corpus.py ~/acme-docs            # the fictional corpus, with realistic file times
cd ~/acme-docs && mda index                        # cards cost about $0.30 with the api backend
```

## Video 1 — "Where did that answer come from?" (25–35 s, silent)

```bash
scripts/demo/record.sh ~/acme-docs video1.mp4 \
  "Why did we pick Postgres over MySQL, and when did that section last change? Three lines, with sources."
```

`record.sh` starts Claude Code off camera with only the markdownattractor server and its read-only tools allowed, types the question, and records the answer. The look is set in `scripts/demo/video1.tape.in`: MesloLGS NF at 20 pt, Catppuccin Mocha, a framed window with rounded corners on a violet-to-blue gradient, 1440×810.

What a good take shows (draft take, 2026-11-10: answered in 8 s after 3 tool calls):

1. The question.
2. "Called markdownattractor 3 times": no file was read whole.
3. Each line of the answer with its file and line range (`adr/0001-postgres-over-mysql.md:6-8`).
4. The date the section last changed, and where that date comes from.

Still to add in the edit: three captions (the question; "file, lines and date on every claim"; the install line as the end card) and a trim to under 30 s. For the final take, record in a short path such as `~/acme-docs` so the header reads cleanly, and check that the status line shows nothing personal.

## Video 2 — "Saved, then searchable" (16 s, silent) — recorded 2026-11-18 on v0.2.8

```bash
scripts/demo/make-corpus.py ~/acme-docs && (cd ~/acme-docs && mda start)   # no key needed
scripts/demo/record-fresh.sh ~/acme-docs fresh.mp4
```

One terminal, nothing off camera but the prompt setup: `mda search canary --raw` finds nothing; two `echo` lines append a "Canary" section to `runbooks/deploy.md`; the same search finds it with file, heading path and line range, marked "just now". No key is set, so the hit is raw text ("pending": no card yet). In a timed run of the same steps the section was found 1.0 s after the save (one machine, one run; the benchmark figure is T4's p50 of 1.7 s over 20 edits on Prisma). The README shows the session as text. The recording itself (an mp4 and a 960 px GIF) is not in the repository: the commit gate refuses binary files, so the owner keeps it and attaches it to posts. This is the launch demo (plan 2026-11-v0.3, Phase D): change-impact review did not pass, so the demo is the freshness loop.

The split-screen version with a visible timer and "p50 8.4 s" for the card is not recorded; it needs a key and adds little to this take.

## Video 3 — "Ask, edit, ask again" (45 s, silent, captioned) — recorded 2026-11-18 on v0.2.9 — the launch clip

```bash
scripts/demo/make-corpus.py ~/acme-docs && (cd ~/acme-docs && mda index && mda start)   # cards: about $0.30
scripts/demo/record-live.sh ~/acme-docs live.mp4
```

One window, two panes (tmux): Claude Code above, "a teammate's terminal" below.

1. Claude is asked how long the error rate is watched after a release. It answers in about 4 s after two tool calls: ten minutes, with `runbooks/deploy.md:5-9`.
2. In the lower pane a `sed` line changes "ten minutes" to "thirty minutes" in the runbook.
3. Claude is asked whether any runbook changed in the last hour. It names the section, the index's last-changed time and the new text, and says its earlier answer is out of date.

Five captions (rendered by `scripts/demo/captions.py`, overlaid at times that follow the tape's fixed pauses) are added after the take, then two cards: the install commands (3.5 s) and "How it works" (9 s: the pipeline from files to daemon to SQLite index to Claude Code, and the three ranked lists that are fused, with the measured 1.7 s and about 8 s); nothing in the terminal is edited, cut or sped up. The script puts the runbook back afterwards. The model is Sonnet (`DEMO_MODEL`), the default login; `DEMO_KEEP_RAW=path` keeps the take without captions and `DEMO_RAW=path` captions an earlier take again.

What the take does not show: the second question is a different one on purpose. Asked the same question twice, Claude answers from its own earlier answer without searching (`docs/aha.md`, 2026-11-18). The first video's recording (video 2) is kept as the clip for "works without a key". The mp4 and a 960 px GIF are with the owner, outside the repository (the commit gate refuses binary files).

## Demo questions

| # | Question | Shows | Status |
|---|---|---|---|
| 1 | Why did we pick Postgres over MySQL, and when did that section last change? | provenance: file, lines, date | recorded as a draft, 2026-11-10 |
| 2 | What changed in the deploy runbook in the last month? | time-aware search (`since`) | candidate, not yet run |
| 3 | Who do I page when the database fails over, and what is the first step? | an answer drawn from two documents | candidate, not yet run |

No video states a token-savings figure: that claim did not pass its gate (`docs/benchmarks.md`).
