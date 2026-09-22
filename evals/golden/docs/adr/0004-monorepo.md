# ADR 0004: One repository for the web, API and workers

Date: 2023-11-02
Status: Superseded by ADR 0009

## Context

Three repositories meant three CI pipelines and a release train that took a week to coordinate. Shared types drifted between the API and the web client twice in October 2023.

## Decision

The web client, the API and the background workers move into one repository with a single CI pipeline and a shared `packages/types` module.

## Consequences

- Positive: one pull request changes a type and both its producers and consumers.
- Negative: CI runs take 18 minutes end to end; selective builds are a follow-up.
