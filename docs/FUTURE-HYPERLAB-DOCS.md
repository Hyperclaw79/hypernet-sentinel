# Future Hyperlab Docs updates

Do not apply these updates until the validated application is deployed through Arcane GitSync.

- Purpose: retain scheduled and on-demand Cloudflare-targeted Internet throughput, HTTP latency/jitter, and UDP/STUN loss measurements.
- Canonical repository: `https://github.com/Hyperclaw79/hypernet-sentinel`.
- Image: `ghcr.io/hyperclaw79/hypernet-sentinel`; record deployed semantic version and immutable digest.
- Deployment: independent Arcane GitSync project sourced from `Hyperclaw79/hyperlab` in Forgejo at `hypernet-sentinel/compose.yaml`. GitHub is authoritative for application source and GHCR images; Forgejo is authoritative only for the reviewed Hyperlab Compose source. Record the exact GitHub application revision, GHCR digest, Forgejo Compose revision, and Arcane GitSync result. Do not create a local-Compose fallback.
- Persistent data: record the final NAS host path mapped to `/data`, UID/GID `10001`, authoritative SQLite classification, backup job, and restore-test status.
- Routing: record the final NPM hostname, direct NAS port, TLS behavior, and unmatched-host negative canary.
- Schedule: hourly quality diagnostic, four-hour full diagnostic, 90-second timeout, 90-day retention unless deployment overrides are approved.
- MCP: unauthenticated Streamable HTTP at `/mcp`; document `get_current_status`, `get_latest_result`, `query_results`, `get_active_test`, and `run_test`.
- Operations: `/healthz`, expected startup window, log location in Dozzle, rollback image/digest, schema compatibility check, and interrupted-run behavior.
- Observability: create a Kuma HTTP monitor for `/healthz`; confirm Beszel container visibility. Sentinel adds connection diagnostics and does not replace Kuma availability monitoring or Beszel host/container telemetry.
- Required NAS capabilities: outbound HTTPS/TCP 443, DNS, outbound UDP 3478, writable persistent `/data`; no Docker socket, host network, device, or additional Linux capability.
- Measurement semantics: HTTP latency is to `speed.cloudflare.com`, UDP loss is to `turn.cloudflare.com:3478`, and neither proves fault ownership.
