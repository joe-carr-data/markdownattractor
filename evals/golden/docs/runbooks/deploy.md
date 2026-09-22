# Deploy runbook

Owner: platform team. Last reviewed 2026-07-30.

## Release

1. Merge to `main`; CI builds the image and pushes `api:<sha>`.
2. Run `deployctl release --sha <sha>`. It provisions the idle colour, runs the smoke suite and flips traffic.
3. Watch the error-rate panel for ten minutes. Above 1% is a rollback.

## Rollback

Run `deployctl rollback --to <previous-sha>`. Traffic flips back to the previous colour within 20 seconds. The previous colour is kept warm for 30 minutes after a release; after that, a rollback is a full release of the old sha and takes about six minutes.

## Database migrations

Migrations run before the flip through `deployctl migrate`. A migration must be backward compatible with the previous release because both colours can serve traffic during the flip. Never drop a column in the same release that stops writing to it.
