# Search index rewrite — proposal

Author: Tomás. Draft 2026-08-18.

## Problem

The product search index is rebuilt every hour from a full export, which takes 40 minutes and makes new products invisible for up to an hour. Customers filed 23 tickets in July 2026 about missing products.

## Proposal

Index incrementally from the `products.events` stream. A product is searchable within 30 seconds of being saved. The full rebuild becomes a weekly consistency check.

## Risks

- The events stream has no replay beyond 7 days; the weekly rebuild covers gaps.
- Ranking changes must be evaluated on the golden query set before rollout.
