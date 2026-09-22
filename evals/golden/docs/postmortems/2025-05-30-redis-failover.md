# Postmortem — Redis failover logged everyone out (2025-05-30)

## Impact

Every logged-in customer was logged out at 14:03 UTC; login traffic tripled for 20 minutes.

## Cause

Sessions lived in Redis. The cluster failover promoted a replica that had lost the last 40 seconds of writes, and the session keyspace was not replicated with `wait` semantics.

## Follow-up

Sessions moved to signed cookies (ADR 0003, shipped 2025-11-05).
