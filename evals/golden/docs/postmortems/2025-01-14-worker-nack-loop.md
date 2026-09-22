# Postmortem — worker nack loop (2025-01-14)

## Impact

Order confirmations were delayed by up to 50 minutes for 3,100 orders.

## Cause

A worker deploy shipped a consumer that raised on a new event field and nacked the message, which was redelivered immediately, forever.

## Follow-up

- Consumers now dead-letter a message after 5 failed attempts.
- `queuectl pause` was added to stop a consumer group without redeploying.
