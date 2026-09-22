# Secrets rotation

## Schedule

Database passwords rotate every 90 days, API keys for third parties every 180 days, and the cookie signing key every 30 days through the key rotation service (two keys are valid at any time so sessions survive a rotation).

## Rotating a database password

1. `vault write db/rotate/orders` creates the new credential.
2. Restart PgBouncer pools: `pgbouncerctl reload`.
3. Verify with `deployctl smoke --suite db`.

## Emergency rotation

If a secret leaks, rotate immediately and open a SEV2. The audit log in the vault shows every read of the leaked secret for the last 90 days.
