# Codex review — dependency upgrade for four RUSTSEC advisories (2026-11-10)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: branch `fix/deps-advisories` (PR #57): workspace `rusqlite` 0.40 → 0.41 (`libsqlite3-sys` 0.39.1, bundled SQLite 3.54.1), `rmcp` and `rmcp-macros` 3.4.0 → 3.9.0, and the transitive `sha2` 0.11 stack (block-buffer, const-oid, crypto-common, digest, hybrid-array, typenum). Purpose: clear RUSTSEC-2026-0457, -0459, -0461, -0462.

**Verdict:** "Approve. No concrete regression in storage compatibility, MCP behaviour, dependency policy, or lockfile consistency." No findings.

## What the reviewer did (from its run log)

- Diffed `Cargo.toml` and `Cargo.lock`; read the store, the MCP server and the CLI MCP code.
- Diffed the `rmcp` 3.4.0 and 3.9.0 sources (including `model.rs` and the stdio transport) and read the 3.9.0 CHANGELOG. Noted that rmcp's latest protocol version becomes 2026-07-28 while initialize negotiation keeps the earlier behaviour; not raised as a finding.
- Checked the bundled SQLite version and file-format defines; noted that the store sets WAL mode, a 5 s busy timeout and IMMEDIATE write transactions.

## Triage

- No findings to triage.
- Depth caveat, recorded rather than dropped: Codex returned a summary verdict, not a per-question breakdown of the four questions it was asked (crate behaviour, on-disk compatibility, deny/ADR policy, lockfile coherence). Independent evidence on our side: `make check` green (335 tests, including the MCP end-to-end test through a real rmcp client; `cargo deny` advisories, bans, licenses and sources ok). No new direct crate, so no ADR.
