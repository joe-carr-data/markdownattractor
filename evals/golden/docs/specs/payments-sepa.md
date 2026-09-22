# SEPA payments

## Flow

A SEPA order is confirmed immediately and settled within 3 business days. The `payment_pending` state hides the order from fulfilment until the provider webhook confirms settlement.

## Failure handling

A rejected debit moves the order to `payment_failed` and emails the customer with a retry link valid for 7 days.

## Feature flag

SEPA is behind `payments.sepa` in the config service. It must default to its last known value, not to off (incident 2026-06-12).
