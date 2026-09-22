# ADR 0005: Feature flags through the config service, not environment variables

Date: 2024-09-23
Status: Accepted

## Context

Flags lived in environment variables, so turning a flag off meant a deploy. The checkout redesign in September 2024 needed a kill switch that worked in seconds.

## Decision

Flags are read from the config service with a 10-second cache. Every flag has an owner, a description and an expiry date; the CI job `flags-audit` fails when a flag is past its expiry.

## Consequences

- Positive: a flag flips in under ten seconds without a deploy.
- Negative: the config service is now on the request path; a stale cache is served if it is down.
