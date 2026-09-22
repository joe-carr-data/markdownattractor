# TLS certificate renewal

Certificates renew automatically through the ACME job every 60 days. This runbook is for when the job fails.

## Symptoms

The `cert-expiry` alert fires 14 days before expiry. The ACME job logs `challenge failed` when the DNS provider token has expired.

## Manual renewal

1. Rotate the DNS provider token in the secrets vault (`vault write dns/token`).
2. Re-run the job: `kubectl create job --from=cronjob/acme-renew acme-manual`.
3. Confirm with `openssl s_client -connect api.example.com:443 | openssl x509 -noout -dates`.
