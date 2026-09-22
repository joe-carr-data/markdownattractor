# Database failover

## When to fail over

Fail over when the primary is unreachable for more than 60 seconds or replication lag exceeds 5 minutes with the primary saturated. Do not fail over for a single slow query; kill the query instead.

## Procedure

1. `pgctl promote --replica db-replica-1` promotes the replica. Takes 30 to 90 seconds.
2. Update the `DATABASE_PRIMARY` entry in the config service; PgBouncer reloads in under 10 seconds.
3. Announce in #incidents and open a follow-up to rebuild the old primary as a replica.

## After the failover

The old primary must be rebuilt from a fresh base backup before it rejoins; reusing its data directory caused the split-brain incident of 2024-11-08.
