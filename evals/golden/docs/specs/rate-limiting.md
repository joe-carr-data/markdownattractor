# Rate limiting

## Where

Rate limits are enforced at the API gateway with a token bucket per API key, stored in Redis. Redis is the only stateful part; a Redis outage fails open (no limits) and pages the on-call.

## Limits

- Public API keys: 1000 requests per minute.
- Partner keys: 10000 requests per minute.
- Login endpoint: 10 attempts per minute per IP.
