# Codex review — a first run needs no key (2026-11-16)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: PR #79 at its first commit (`git diff origin/main...HEAD`). Static: Codex ran no tests.

**Verdict:** "Changes requested. The daemon's missing-backend retry loop is addressed, but inline indexing and the gate leave substantive gaps." Wire compatibility of `LiveStatus.summaries_off` judged sound both ways; no environment divergence found between `mda start` and the detached daemon.

## Findings and triage (5, all P2, all accepted and fixed in the same PR)

| # | Finding | Triage |
|---|---|---|
| 1 | Inline `mda index` (no daemon) with pending sections and no key still fails: `backend_for`'s error is propagated, exit non-zero, and heading-only sections get no card. | **Accepted, fixed.** `index` gives heading-only sections their cards, prints the same note as `start`, reports `cards_off` in `--json` and exits 0. Test `index_without_a_daemon_or_a_key_succeeds_and_says_cards_are_off`. |
| 2 | The cards-off branch reads every pending section (with its text) on every idle poll, although without a backend they stay pending. | **Accepted, fixed.** The scan runs once at start and then only after the indexer's wake (new sections or a rescan); an idle poll only catches up vectors. Not benchmarked on a large root. |
| 3 | `first-run.sh` could pass on a local `target/release/mda` through the hook's contributor fallback while claiming a download. | **Accepted, fixed.** The hook runs from a temporary plugin layout holding only itself and `VERSION`; the installed binary's version is checked. |
| 4 | "No provider key reaches any child process" was not true: the corpus script was unwrapped and only four variables were removed. | **Accepted, fixed.** Every child runs under `env -i` with `PATH`, a temporary `HOME`/`TMPDIR` and the model directory. |
| 5 | The gate counted a hit on the file, not on the expected section. | **Accepted, fixed.** Each question names file and heading; both must match a hit in the top five. |

## Not covered by the review

Codex returned nothing on: the events and counters the cards-off branch does not publish (`RoundStarted`, `last_round_at`: none are, by design — no round runs); `doctor`'s final line when the key is missing and the backend is `local` or `claude-cli` (the "raw search, no cards" line prints only for the `api` backend without its key; other backends keep their own checks); the README figures (0.883 and 0.983 success@5 are `mda eval` on `evals/golden`, the second with recorded cards); further missing tests. Deferred test: the scan-once rule of finding 2 has no direct test (the daemon test covers the first scan and a later save).
