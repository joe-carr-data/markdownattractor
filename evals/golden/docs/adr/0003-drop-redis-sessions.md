# ADR 0003: Move sessions from Redis to signed cookies

Date: 2025-06-18
Status: Accepted

## Context

Session lookups were 41% of Redis traffic and the Redis cluster failover on 2025-05-30 logged every user out. Sessions hold only a user id, a role and an expiry.

## Decision

Sessions become signed, encrypted cookies (`session_v2`) using the platform's key rotation service. Redis keeps caching and rate limiting only.

## Consequences

- Positive: a Redis outage no longer touches logged-in users.
- Negative: revoking one session needs the deny-list, checked on every request.
- Follow-ups: remove the `sessions` keyspace after the 90-day expiry window closes on 2025-09-16.
