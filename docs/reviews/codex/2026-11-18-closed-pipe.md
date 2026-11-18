# Codex review — a closed output pipe is a quiet exit (2026-11-18, PR #108)

Reviewer: Codex (requested `gpt-6.2-astra`; it reports "GPT-6 (Codex)"; CLI 0.167.0; fresh thread `01a1ec9e-a6da-7ea0-b26d-4729bc7a2ef6`, read-only, with a pipe check against the existing binaries, through the `codex:codex-rescue` companion; no nested `codex exec`). Pinned to `e2495ca`. Triaged by Claude.

**Verdict:** "merge after fixes." Three findings (all medium). Found clean: both JSON helpers go through `println!`; no other stdout write path; the detached daemon's output goes to a file; the MCP server writes through its transport, not `println!`.

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | Medium | `process::exit` in the hook skips destructors: `mda eval` leaves its temporary corpus copy when its report meets a closed pipe. | **Accepted as is, documented.** The released binary is built with `panic = "abort"`, which never ran destructors on this panic either, so nothing changes for users; a debug build now behaves like the release. The reviewer's preferred fix (a fallible print helper at every print site, with the status chosen in `main`) is the right design and a large mechanical change; backlog. |
| 2 | Medium | The matcher missed the other Windows code for a pipe whose reader is gone (109). The text is not a stable contract. | **Fixed** for 109. The text match stays (no `libc`, no `unsafe`); the Unix message is pinned by the test, the Windows ones are untested. 141 is a Unix convention; on Windows it is simply a non-zero status without panic text. |
| 3 | Medium | The test could pass with the hook removed: the reader was dropped after the spawn and both 0 and 141 were accepted. | **Fixed.** The reader is closed before the command starts (`std::io::pipe`), the command always prints (`schema section`), and the test asserts status 141 and an empty stderr. |
