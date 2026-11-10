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

## Video 2 — "Edit, searchable in 8 s" (15–20 s, split screen) — not recorded yet

Left pane: an editor saving a new section in `runbooks/deploy.md`. Right pane: `mda watch`, then `mda search` finding the new text. A timer between save and hit. Caption: "p50 8.4 s in our benchmark" (T4; the only speed claim that passed its gate).

## Demo questions

| # | Question | Shows | Status |
|---|---|---|---|
| 1 | Why did we pick Postgres over MySQL, and when did that section last change? | provenance: file, lines, date | recorded as a draft, 2026-11-10 |
| 2 | What changed in the deploy runbook in the last month? | time-aware search (`since`) | candidate, not yet run |
| 3 | Who do I page when the database fails over, and what is the first step? | an answer drawn from two documents | candidate, not yet run |

No video states a token-savings figure: that claim did not pass its gate (`docs/benchmarks.md`).
