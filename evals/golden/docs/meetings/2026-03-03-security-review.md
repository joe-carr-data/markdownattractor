# Security review — 2026-03-03

## Findings

- Cookie signing keys rotate every 30 days; the previous key stays valid for one rotation. Approved.
- Third-party API keys had no rotation schedule. Now every 180 days.
- The vault audit log retention was 30 days; raised to 90 days so an emergency rotation can trace reads.

## Open

- Enforce MFA for the vault UI by 2026-04-30.
