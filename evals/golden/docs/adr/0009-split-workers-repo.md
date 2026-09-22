# ADR 0009: Move the background workers back into their own repository

Date: 2026-02-10
Status: Accepted · supersedes ADR 0004

## Context

The workers team ships on a different cadence and their Python toolchain doubled the monorepo's CI time. Shared types are now published as a versioned package, which removes the drift problem ADR 0004 solved.

## Decision

The workers move to `platform/workers`. Shared types are consumed from the published `@acme/types` package pinned by minor version.

## Consequences

- Positive: monorepo CI drops from 18 to 7 minutes.
- Negative: a type change needs a package release before workers can use it.
