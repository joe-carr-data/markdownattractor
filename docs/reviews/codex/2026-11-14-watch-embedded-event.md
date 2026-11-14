# Codex review — `mda watch` and the embedding event (2026-11-14)

Reviewer: Codex (`gpt-6-astra`, fresh read-only thread, through the `codex:codex-rescue` companion; no nested `codex exec`). Subject: PR #68, `DaemonEvent::Embedded.ms` from `u128` to `u64`. Static review; Codex ran no tests.

**Verdict:** "No correctness findings introduced by PR #68. The diagnosis and conversion are correct." The diagnosis was confirmed in serde's source: the derived decoder of an internally tagged enum buffers into `Content`, which has no 128-bit integer. The only other 128-bit field that is serialized, `EmbedReport.ms`, is never read back through a tagged response.

## Findings and triage (2, both pre-existing; neither is in this change)

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | Medium | The socket reader applies the 64 KiB request limit to responses too, while the writer has no limit; `IndexReport.errors` is unbounded, so a delegated `mda index` with very many per-file errors could receive a valid response it refuses to read. | **Deferred** to `docs/backlog.md` B-0003. Not observed; needs its own change (a response limit, a bounded error list, an oversized-response test). |
| 2 | Low | `cost_usd` fields are plain `f64`; a NaN or infinite cost would be written as `null` and fail to read back. Codex calls the production occurrence hypothetical. | **Deferred** to B-0003 with finding 1. |

## Notes acted on

- Test strengthened as suggested: the exact JSON of the event is asserted, and `ms: u64::MAX` (where the daemon saturates) round-trips.
- Compatibility, as Codex states it: the JSON is the same, so a 0.2.0 daemon works with a fixed `mda watch`; a fixed daemon with a 0.2.0 `mda watch` still fails, because the old reader asks for a `u128`. Both live in one binary, so an upgrade fixes both.
- Not done: a round-trip test of every event variant (worthwhile, not required for this fix); listed in B-0003.
