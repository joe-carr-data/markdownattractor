# On-call runbook

## Paging

PagerDuty pages the primary on-call for any SEV1 or SEV2. The secondary is paged after 10 minutes without acknowledgement. Escalate to the engineering manager for a SEV1 that lasts more than 30 minutes.

## Severity levels

- SEV1: checkout or login unavailable for any customer.
- SEV2: a degraded feature with a workaround, or error rate above 2% on any tier.
- SEV3: a single customer affected, or an internal tool down.

## Handover

The weekly handover is Monday 10:00 UTC. The outgoing on-call posts open incidents, silenced alerts and their expiry, and any flag that was flipped during the week.
