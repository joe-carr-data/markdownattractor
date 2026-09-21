---
title: Deploy runbook
status: current
owner: platform
---

# Deploy runbook

Everything about shipping the API. Decided in [ADR-0007](../adr/0007-blue-green.md).

## Deploy

We deploy with blue-green since March 2026. Check [status](https://status.example.com) first.

1. `deployctl plan --env prod`
2. `deployctl apply --env prod`
3. Watch the dashboard for 10 minutes.

### Rollback

If error rate exceeds 2% within 10 minutes, roll back:

```bash
deployctl rollback --to <previous-sha>
deployctl verify --env prod
```

Rollback is safe because the previous colour keeps serving until `apply` completes.

#### Rollback of a database migration

Migrations are forward-only. Do **not** roll back the schema; deploy a fix instead.
See [contacts](#contacts) if unsure.

## Contacts

| Role | Person |
|---|---|
| On-call | see PagerDuty |
| Owner | platform team |



