# Hypernet Sentinel

Hypernet Sentinel is an independent, self-hosted Internet connection history application. It schedules connection-quality and throughput diagnostics, stores measurements in SQLite, presents a responsive dashboard, and exposes a focused Model Context Protocol server.

- Repository: `Hyperclaw79/hypernet-sentinel`
- Container: `ghcr.io/hyperclaw79/hypernet-sentinel`

> Hypernet Sentinel is not affiliated with Cloudflare or the author of `cloudflare-speed-cli`. Cloudflare is the initial measurement provider, not a diagnosis of which physical or routing component caused a problem.

## What it measures

| Displayed metric | Actual measurement |
|---|---|
| Cloudflare download/upload | HTTP transfers against `speed.cloudflare.com/__down` and `__up` |
| HTTP latency and jitter | HTTP round-trip/time-to-first-byte samples against `speed.cloudflare.com/__down?bytes=0` |
| Bufferbloat | Nonnegative added latency: download- and upload-loaded median latency minus idle median latency, clamped to zero when sampling noise makes the delta negative |
| UDP packet loss | Experimental STUN binding round trips to `turn.cloudflare.com:3478` |

Packet loss is target-specific. Failure of the UDP probe is stored as unavailable rather than zero. Sentinel deliberately omits upstream MOS labels, connection-quality grades, automatic diagnosis, and fault attribution.

## Architecture

- `measurement-core` contains a deterministic, selected snapshot of the GPL-3.0 upstream Rust engine plus a small Sentinel-owned adapter.
- `sentinel-server` owns scheduling, concurrency, timeouts, SQLite, HTTP API, MCP, and static assets.
- The dashboard is static HTML, custom CSS, vanilla JavaScript, and locally bundled Chart.js zoom/pan assets.
- One process and one production image provide the dashboard, API, scheduler, and MCP server.

There is no Node.js runtime, external database, Redis, message broker, Docker socket, WebSocket, or separate job service.

## Defaults

- Quality diagnostic: hourly.
- Full diagnostic: every four hours.
- Manual diagnostic: all tests by default, with latency/jitter, download, upload, and packet loss individually selectable from the dashboard, `POST /api/run`, or MCP `run_test`.
- Download/upload duration: 10 seconds each.
- Idle HTTP latency window: 2 seconds at 250 ms intervals.
- UDP probes: 50.
- Overall timeout: 90 seconds.
- Retention: 90 days.
- One diagnostic at a time. Duplicate manual/MCP calls return a conflict and scheduled work waits for the next scheduler evaluation.

All values are configurable with environment variables.

## Run locally

Requirements: Rust 1.97.1 and Python 3 for upstream-update tooling. The optional hot-reload launcher also uses Node.js 22+ and `watchexec` 2.5.1 (`cargo install --locked watchexec-cli --version 2.5.1`).

```powershell
$env:SENTINEL_DATA_DIR = "$PWD/data"
$env:SENTINEL_SCHEDULER_ENABLED = "false"
cargo run -p sentinel-server
```

Open `http://127.0.0.1:8080`. Enabling the scheduler on an empty database starts a due full diagnostic on its first evaluation.

For automatic rebuilds and browser refresh, choose **Run Hypernet Sentinel (hot reload)** from VS Code's Run and Debug menu. Rust changes rebuild and restart the server through `watchexec`; files under `web/` are served directly in debug builds and trigger the loopback-only development proxy at `http://127.0.0.1:3000`. The proxy uses only Node.js built-ins, and production builds continue to use only the embedded assets.

Validation:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
docker build -t hypernet-sentinel:local .
```

Normal tests never perform real bandwidth measurements.

## Docker and Arcane

The image runs as UID/GID `10001`, writes only to `/data`, needs no Linux capabilities, and includes an application-level healthcheck.

```powershell
docker build -t hypernet-sentinel:local .
docker volume create sentinel-data
docker run --rm -p 8080:8080 --read-only --tmpfs /tmp --cap-drop ALL `
  -v sentinel-data:/data hypernet-sentinel:local
```

[`compose.example.yaml`](compose.example.yaml) is a portable example, not the installed Compose authority. The Hyperlab deployment is an Arcane GitSync project whose reviewed Compose source lives at `hypernet-sentinel/compose.yaml` in the `Hyperclaw79/hyperlab` Forgejo repository; GitHub remains authoritative for the application source and GHCR image. There is intentionally no local-Compose fallback: if GitSync cannot resolve the expected Compose revision, stop and fix GitSync rather than creating a second authority.

The deployment deliberately follows `ghcr.io/hyperclaw79/hypernet-sentinel:latest`. For this project, `latest` advances only when a strict `vMAJOR.MINOR.PATCH` release promotes an already tested immutable `sha-<commit>` image. Arcane owns rollout and rollback, while the immutable SHA image and registry digest remain the rollback anchors.

Add an NPM route only after the direct service and `/healthz` are healthy. The final hostname and NAS storage mapping are deployment decisions and are not guessed here.

## HTTP API

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/healthz` | SQLite and scheduler readiness plus engine revision |
| `GET` | `/api/summary` | Latest per-metric values and failure count |
| `GET` | `/api/results?range=24h` | History; ranges are `24h`, `7d`, or `30d` |
| `GET` | `/api/current` | Active diagnostic and phase |
| `POST` | `/api/run` | Start selected manual diagnostics; returns `409` when busy |

CORS is not enabled. The API never accepts arbitrary targets, shell commands, SQL, or engine URLs.

The optional JSON body defaults every field to `true` when omitted:

```json
{"latency":true,"download":true,"upload":true,"packet_loss":true}
```

Bufferbloat is available when idle latency and at least one throughput direction are selected. Existing full-run rows are backfilled from their retained raw loaded-latency summaries during schema migration.

## MCP server

The MCP endpoint is `/mcp` on the same listener. It uses the official Rust SDK and Streamable HTTP. It intentionally matches the simple Hyperlab Docs operational model: no application authentication and no separate MCP container. NPM supplies the eventual HTTPS route.

Tools:

- `get_current_status`
- `get_latest_result`
- `query_results`
- `get_active_test`
- `run_test`

All results include structured JSON. `run_test` accepts the same four optional boolean selections as the dashboard and returns a tool error when another diagnostic is active. The server exposes no arbitrary network target, database query, configuration mutation, deletion, shell, container, or router operations.

Because the endpoint has no authentication, anyone who can reach its NPM route can read history and consume bandwidth by starting a test. Treat the NPM exposure boundary as the trust boundary.

## Configuration

| Variable | Default |
|---|---:|
| `SENTINEL_BIND` | `0.0.0.0:8080` |
| `SENTINEL_DATA_DIR` | `/data` |
| `SENTINEL_QUALITY_INTERVAL_SECONDS` | `3600` |
| `SENTINEL_FULL_INTERVAL_SECONDS` | `14400` |
| `SENTINEL_DOWNLOAD_SECONDS` | `10` |
| `SENTINEL_UPLOAD_SECONDS` | `10` |
| `SENTINEL_LATENCY_SECONDS` | `2` |
| `SENTINEL_UDP_PACKETS` | `50` |
| `SENTINEL_CONCURRENCY` | `6` |
| `SENTINEL_RUN_TIMEOUT_SECONDS` | `90` |
| `SENTINEL_RETENTION_DAYS` | `90` |
| `SENTINEL_SCHEDULER_ENABLED` | `true` |

## Persistence and recovery

`/data/sentinel.db` is authoritative. SQLite uses WAL, normal synchronous mode, a busy timeout, foreign keys, and explicit migrations. Timestamps are stored in UTC and rendered in the browser's local timezone.

On restart, any run left in `running` is marked failed with an interruption error. Never delete `/data` during image rollback. Check database migration compatibility before downgrading and retain the prior immutable image reference until acceptance checks pass.

## Troubleshooting

- `healthz` reports `database: false`: verify `/data` ownership and writable storage.
- A test remains active: the 90-second deadline cancels it; inspect bounded logs if it fails.
- UDP loss is unavailable: UDP/3478 resolution or traffic may be blocked; HTTP metrics can still be valid.
- Dashboard works directly but not through NPM: verify the NPM upstream, hostname, and TLS route after confirming the direct port.
- Scheduled runs do not start: check scheduler readiness and whether another test is active.
- Image rollback fails: confirm the image and schema combination; preserve the database for diagnosis.

Use Uptime Kuma for `/healthz` availability, Beszel for container resources, and Dozzle for private logs. Sentinel complements these tools by retaining active Internet diagnostic measurements; it does not replace service uptime monitoring or host telemetry.

## Upstream and licensing

The application is GPL-3.0-only because it incorporates adapted GPL-3.0 upstream source. See [`UPSTREAM.md`](UPSTREAM.md), [`ATTRIBUTION.md`](ATTRIBUTION.md), and [`LICENSE`](LICENSE).

The automated updater follows upstream `main`, but production builds never download live source. A weekly workflow refreshes the selected snapshot, validates the complete application and container, then commits directly to Sentinel `main` and dispatches the normal immutable build. It never opens a pull request anywhere. Trusted `main` builds publish a commit-addressed GHCR image; semantic-version tags promote that exact tested image without rebuilding source.

Future Hyperlab documentation work is tracked in [`docs/FUTURE-HYPERLAB-DOCS.md`](docs/FUTURE-HYPERLAB-DOCS.md).
