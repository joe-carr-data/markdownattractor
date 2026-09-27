# Plan — M7: T4, the update-latency table (2026-09-27)

Milestone M7 of the execution plan (`2026-09-benchmark-execution.md` §4 T4, §5 M7): axis C on Prisma. One edit trigger (a dated sentence with a unique token appended to one section), twenty edits on twenty seeded sections; for every arm the time from the save to (a) raw-searchable (mda's raw search, qmd's BM25 search), (b) a card (mda only), (c) a correct grounded answer through the arm's own agent configuration (the comparison endpoint); 1 s polling, 300 s timeout per endpoint; every arm that needs an update trigger gets its documented one (`qmd update && qmd embed`; graphify's `/graphify <path> --update` skill flow through the login). Timeouts, fallback reads and the raw-search latency are published. Acceptance: complete, and mda's p50 save → card reported against G1 (< 15 s). Also in M7: the two T3 renderer fixes deferred from the 2026-09-26 publication check (`t3.sh`: MiB label, "reconstruction: attach + embed + score" column), and the Astra calibration item stays open (not a T4 dependency).

## Decisions carried in

- **Prisma's graphify arm is the Haiku-built graph** (`graphify-haiku`): the Sonnet build never completed on Prisma; the table says so and the arm is labelled.
- **Working copies, never the pinned checkouts**: every arm edits its own copy of the Prisma checkout under `$RUN/t4/<arm>/src` (mda's copy carries the frozen store; qmd's copy gets its own `qmd` index; graphify's copy is the haiku build's copy with its graph); the T1/T2 artifacts and the pinned checkout stay untouched, and the T4 freeze records the copies' provenance.
- **The edit**: `T4 note (<date>): <token>.` appended as a new paragraph at the end of a section's body, one section per edit, twenty sections chosen by seeded order (`sha256(seed ‖ section_id)`; this line originally said `blake3` — the harness always used SHA-256, corrected 2026-09-27 after the publication check) among sections with ≥ 200 characters of body, one per document; the token is `t4-<edit>-<8 hex>`, unique in the corpus (asserted before the edit).
- **Endpoints**: (a) raw-searchable = the token is returned by the arm's raw search (`mda search --raw`, `qmd search`) for the edited page; (b) card = the store has a `summarized` row for the edited section's new hash (mda only); (c) grounded answer = a headless `claude -p` session with the arm's launch configuration answers "What does the T4 note in <page> › <heading> say?" with the token in the answer and the page cited (resolved by the T2 citation rule). Each endpoint is timed from the save; polling every 1 s; 300 s timeout → a recorded timeout, never a missing row (rule 0.3).
- **Update triggers**: mda none (the daemon watches); qmd `qmd --index t4-qmd update && qmd --index t4-qmd embed` started right after the save, its duration recorded and included in every qmd endpoint; graphify `/graphify <copy> --update` through the login after the save, its wall-clock and cost recorded and included in its endpoint (its raw-search endpoint is n/a: graphify has no raw search); grep none (no index; endpoint (c) only).

## Tasks

- [x] **H1** `scripts/eval/t4.sh setup`: the copies (mda: files + `.markdownattractor/` store; qmd: files + `qmd --index t4-qmd collection add`, `update`, `embed`; graphify: the haiku build's copy), the daemon started on mda's copy (`mda start --root`, backend `claude-cli`, Haiku), the seeded edit plan (`t4/edits.jsonl`: edit, page, heading, section hash before, token) written before any edit.
- [x] **H2** `t4.sh run [edit-range]`: per edit, for every arm in turn: apply the edit to the arm's copy at a recorded wall-clock instant, fire the update trigger where the arm has one, poll the endpoints (1 s) to 300 s, write one row per (edit, arm, endpoint) with `t_save`, `t_ready`, latency, timed-out, and the poll count; resumable (a present row is never redone); the mda daemon's own events (`mda watch`) archived per edit.
- [x] **H3** `t4.sh table`: per arm and endpoint p50 / p90 / max, timeouts, the update trigger's own duration (qmd, graphify) and mda's raw-search latency; the G1 line (mda p50 save → card vs 15 s).
- [x] **H4** freeze: `freeze.sh --protocol final --table T4` (copies' provenance: the pinned commit, the store hash, the qmd index fingerprint, the graph hash; the edit plan's hash; the daemon's config; the harness hash; the models); preflight = the T2-style checks plus three activation probes for the answer endpoint per arm on the first three edits.
- [x] **H5** the deferred T3 renderer fixes in `t3.sh` (MiB; "reconstruction: attach + embed + score"), the T3 page regenerated, its hash in runbook §7.3 updated.
- [x] **D1** run: twenty edits, all arms; publish the table and its runbook §7.4 with hashes.
- [x] **Docs**: page section, runbook §7.4, STATUS, CHANGELOG, handoff, this plan ticked.

## Exit criteria

- [x] Twenty edits × every arm × every applicable endpoint has a row (a timeout is a row).
- [x] mda's p50 save → card is published against G1 (< 15 s) whichever way it falls.
- [x] The T4 freeze precedes the rows in git; the table regenerates from the archived rows; the pinned checkouts and the T1/T2 artifacts are unchanged (hashes checked).
- [x] Runbook §7.4 reproduces the setup and the run on a fresh machine.

## Outcome (2026-09-27)

80 rows, no timeouts, every answer correct. mda: raw-searchable p50 1.7 s, card p50 8.4 s (max 10.7 s; G1 met), grounded answer 17.9 s; qmd raw 6.8 s (trigger 6.0 s p50, 44.6 s max), answer 16.6 s; grep answer 7.4 s; graphify answer 37.3 s including its update flow (28.4 s, $0.80 for twenty), graph tool unused. Two lessons: an activation probe or an endpoint question must not name the page (the agents read the file instead of using their tool); an aborted attempt must be purged from every cache before the counted run (three cards came back from the store in under 2 s and were re-measured).

Follow-ups from the 2026-09-27 publication check (`docs/reviews/codex/2026-09-27-t4-page.md`): `t4.sh purge` must copy superseded rows and traces aside before deleting them; the plan's "blake3" wording for the edit order is SHA-256 in the harness (documented on the page); the renderer's quantile is the nearest-rank index, stated on the page.
