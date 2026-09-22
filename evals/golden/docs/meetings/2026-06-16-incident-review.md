# Incident review — 2026-06-16

## What happened

On 2026-06-12 between 09:12 and 09:41 UTC the checkout returned 500 for customers paying with SEPA. A config service outage made the payment provider flag read as `off`, which disabled SEPA for everyone.

## Root cause

The flag cache serves stale values when the config service is down, but the SEPA flag had never been read before the outage on the two newest pods, so their cache was empty and defaulted to `off`.

## Actions

- Warm the flag cache on pod start (owner: Priya, due 2026-06-30).
- Default unknown flags to their last known value from the shared cache instead of `off`.
