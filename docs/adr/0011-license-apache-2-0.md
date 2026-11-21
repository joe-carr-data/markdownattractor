# ADR-0011 — The licence stays Apache-2.0

Status: **Accepted** · 2026-11-22 · Owner decision

## Context

`LICENSE` has been Apache-2.0 since the first commit (2026-09-21, `docs/project-plan.md` § repository layout), with no recorded reason. On 2026-11-22, after the launch posts began, the owner asked why Apache-2.0 and not MIT. The alternatives were MIT, and the Rust convention "MIT OR Apache-2.0" (Rust itself and most popular crates). At this point only the owner and Claude have committed to the repository, so a change would still be simple; after outside contributions it would need their agreement.

## Decision

We keep Apache-2.0 as the only licence.

## Consequences

- Positive: an explicit patent grant from every contributor, with termination for anyone who sues over patents in the work; contributions are under the same terms by default (section 5), with no separate contributor agreement; no rights to the name or logo (section 6), so a fork cannot pass as markdownattractor; a licence company legal teams approve routinely. All our dependencies are compatible (`deny.toml`).
- Negative: longer and less familiar than MIT; it cannot be combined with GPLv2-only code; some Rust users expect the dual licence.
- Follow-ups: none. Revisit only by a new ADR; once outside contributions land, a relicence needs their contributors' consent.

## Alternatives considered

- MIT — shorter and better known, but no patent grant and no trademark clause; neither licence affects adoption or stars.
- MIT OR Apache-2.0 (the Rust convention) — maximally permissive for users; the owner preferred one licence.
