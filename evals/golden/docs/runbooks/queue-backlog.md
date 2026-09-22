# Queue backlog

## Symptoms

The `queue-lag` alert fires when the oldest message in `orders.events` is older than 5 minutes. Customers see delayed order confirmations.

## Causes seen so far

- A worker deploy with a broken consumer (2025-01-14): messages were nacked in a loop.
- Downstream email provider throttling (2025-08-02): workers waited on 429 responses.

## Mitigation

Scale workers with `kubectl scale deploy/orders-worker --replicas=12`. If messages are being nacked in a loop, pause the consumer group with `queuectl pause orders-events`, deploy the fix, then resume.
