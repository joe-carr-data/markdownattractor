# ADR 0009: Change-impact review — `mda check`, verdicts and findings

Date: 2026-11-17
Status: Proposed

## Context

The v0.3 plan (`docs/plans/2026-11-v0.3-clear-advantage.md`, Phase C) adds the one thing neither grep nor a graph of the folder does: after a change to one document, name the other documents it may have left out of date. The plan's Phase C design v1 holds the mechanics. This ADR fixes the parts that become public surface or storage: a command name, a vocabulary, a JSON shape, a schema version and a trait method. Trust (G6) ranks first: a wrong alert costs more than a missed one, and the product must not claim to know which document is right.

## Decision

We add a read-only review of changes with one command, `mda check [--since <revision>]`, built from two steps: candidates found without a model (lexical on the replaced wording, and links), then one schema-constrained judgment per pair by the configured summarization backend. A judgment is one of `possibly_outdated`, `possible_conflict`, `related`, `unrelated`, `abstain`; only the first two are alerts, and the judge abstains when a shared scope cannot be shown. An alert carries evidence from each snapshot it speaks about (before, after, candidate), located in the stored text and rendered as the exact source slice, or it is dropped. Output never states which side is correct and makes no authority claim. The command works on a fresh snapshot (revision pinned once, raw index synced first, files re-read before printing) and reports what it did not review. `Backend` gains one object-safe method for a schema-constrained call with a caller-supplied system prompt, whose result carries usage on every variant; all five implementations provide it. The first version stores nothing: no schema change, no cache. With no backend the command lists candidates, labelled as not judged, and exits 0. No new crate.

## Consequences

- Positive: a wrong alert needs both a wrong verdict and three passing quotes under a visible scope; spend is capped per run and always in the ledger; a first run with no key still gets candidates; nothing to migrate.
- Negative: one more model prompt to version and evaluate; a repeated check pays again until a cache exists; `--since` needs git; renamed files, renamed or moved headings and deletions are reported as not reviewed.
- Follow-ups, only if the Phase C gate passes: an MCP tool `mda_check`, review by the daemon after each save, stored findings and a judgment cache in a new schema version (keyed by a hash of the whole request, with document identity stored beside content hashes), entity and embedding candidates, a `supersedes` verdict with its own evidence of authority. A second ADR records those.

## Alternatives considered

- A whole-folder contradiction scan — quadratic in sections, no natural trigger, and the plan excludes it as a headline.
- One call per change with all candidates in it — cheaper, but a quote can be attributed to the wrong candidate.
- Verdicts decided by date ("newer wins") — a newer file time is not authority (ADR-0008 dates are authored times, not correctness).
- Judging with embeddings alone, no model — similarity finds the same subject, not a contradiction; it stays a candidate source.
- A `supersedes` verdict in the first version — it claims authority; two quotes that agree (before and candidate) do not show a replacement, so it waits for its own evidence rule.
- A new command per surface (`mda impact`, `mda conflicts`) — one verb, `check`, with flags.
