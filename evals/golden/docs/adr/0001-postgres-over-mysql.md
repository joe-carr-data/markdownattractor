# ADR 0001: Use PostgreSQL as the primary datastore

Date: 2024-03-11
Status: Accepted

## Context

The order service needs transactional writes, JSON columns for the flexible cart payload, and row-level locking for inventory reservations. The team has operated MySQL 5.7 for the legacy catalog since 2019, but the catalog has no transactional requirements.

## Decision

We use PostgreSQL 15 for every new service. Managed instances come from the platform team's RDS module; connection pooling goes through PgBouncer in transaction mode.

## Consequences

- Positive: JSONB indexes cover the cart payload queries without a schema change per field.
- Negative: two database engines in production until the catalog migrates (planned for Q1 2025).
- Follow-ups: write the migration runbook; add `pg_stat_statements` to the dashboards.
