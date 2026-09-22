# ADR 0002: Blue-green deployments for the API tier

Date: 2025-03-04
Status: Accepted

## Context

Rolling deploys of the API tier caused two incidents in February 2025 when a bad build served traffic for eleven minutes before the rollout finished. Health checks pass for a process that returns 500 on one endpoint.

## Decision

The API tier deploys blue-green behind the load balancer. `deployctl release` provisions the idle colour, runs the smoke suite against it, and flips traffic in one step. `deployctl rollback --to <sha>` flips back to the previous colour, which is kept warm for 30 minutes.

## Consequences

- Positive: rollback is one command and takes under 20 seconds.
- Negative: double the API capacity for 30 minutes after every release, about $340 per month.
- Follow-ups: extend the smoke suite to cover the checkout endpoint.
