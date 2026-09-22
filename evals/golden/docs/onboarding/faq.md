# FAQ

## How do I turn a feature off quickly?

Flip its flag in the config service; it takes effect within ten seconds. Flags are not environment variables any more.

## How do I get a database password?

Never copy one from a colleague. `vault read db/creds/orders` gives you a credential that expires in 8 hours.

## Who do I page?

For a customer-facing outage, page the primary on-call through PagerDuty. For a question, ask in #platform.
