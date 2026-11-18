# Codex review — naming a running daemon of another version (2026-11-18, PR #109)

Reviewer: Codex (requested `gpt-6.2-astra`; it reports "GPT-6 Codex", exact variant unavailable; CLI 0.167.0; fresh thread `01a1eca7-8c54-7642-92fa-4fed6e12a302`, read-only, static review, no test run, through the `codex:codex-rescue` companion; no nested `codex exec`). Pinned to `7a58f2c`. Triaged by Claude.

**Verdict:** "merge after fixes." Five findings (3 medium, 2 low). Found fine: the added JSON field breaks no consumer in the repository; a doctor warning still exits 0.

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | Medium | An older session talking to a newer daemon was told to `mda restart`, which would put the older version back; two sessions could restart each other's daemon back and forth. | **Fixed.** Versions are compared as numbers. An older daemon gets the restart command; a newer one gets "update the plugin or use a newer session; do not restart it from here". A version that cannot be read is treated like a newer one. `doctor` no longer offers a restart as the fix for a version difference. |
| 2 | Medium | The suggested `mda restart` resolves the root from the current folder and could restart another project. | **Fixed.** The command is printed with `--root` and the folder, single-quoted. |
| 3 | Medium | `mda status`, the command a user actually runs after an update, said nothing; the hook discards `start`'s output. | **Fixed** for the CLI: a note line and `daemon_version_note` in `--json`. **Not done:** the MCP tool `mda_status` (it returns the daemon's version as before, without the comparison); backlog. |
| 4 | Low | `start` compared against `daemon.json`, which can be gone or stale while a daemon answers. | **Fixed.** The live status's version is used, the file's only when the status cannot be read. |
| 5 | Low | Tests covered only the helper. | **Partly.** The unit test now covers older, newer, unreadable and a folder name with a quote. A command-level test needs two binaries of different versions answering one root, which the test setup does not have; not tested end to end, and the PR says so. |
