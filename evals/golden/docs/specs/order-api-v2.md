# Order API v2

## Endpoints

- `GET /v2/orders?since=&until=&status=` lists orders in a time window; pagination by cursor.
- `POST /v2/orders` creates an order; idempotent on the `Idempotency-Key` header for 24 hours.
- `POST /v2/orders/{id}/cancel` cancels before fulfilment.

## Rate limits

1000 requests per minute per API key; 429 with `Retry-After` beyond that.

## Deprecation of v1

v1 stays available until 2027-01-31. Requests to v1 return a `Sunset` header from 2026-10-01.
