# Glossary

- **Colour**: one of the two API tier deployments in blue-green; only one serves traffic.
- **Flip**: switching the load balancer from one colour to the other.
- **deployctl**: the CLI for releases, rollbacks and migrations.
- **Config service**: the source of feature flags and runtime settings, cached for 10 seconds.
- **Vault**: the secrets store with a 90-day audit log.
- **SEV1/SEV2/SEV3**: incident severities, see the on-call runbook.
