# Failover postmortem — 2024-11-12

## Summary

The database failover of 2024-11-08 produced a split-brain for 14 minutes: the old primary came back and accepted writes because its data directory was reused instead of rebuilt.

## Timeline

- 02:14 primary unreachable; 02:16 replica promoted.
- 02:31 old primary restarted by the autoscaler and accepted writes.
- 02:45 old primary fenced.

## Actions

- Rebuild a failed primary from a base backup, never reuse its data directory (runbook updated).
- Fence the old primary automatically during promotion (owner: Marcus).
