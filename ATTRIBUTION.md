# Attribution

## cloudflare-speed-cli

Selected measurement-engine source is adapted from:

- Project: `kavehtehrani/cloudflare-speed-cli`
- Copyright: kavehtehrani and contributors
- License: GNU General Public License v3.0
- Source: https://github.com/kavehtehrani/cloudflare-speed-cli
- Included revision: see `crates/measurement-core/UPSTREAM_REVISION`

Hypernet Sentinel modifies the source boundary for SDK-style embedding, exposes selected internal modules to its adapter, excludes the upstream CLI/TUI/history product surface, and adds a Windows-specific test guard for upstream assertions that use Unix loopback interface names. These modifications began July 29, 2026.

## Chart.js

The dashboard bundles Chart.js 4.5.1, Copyright 2014-2024 Chart.js Contributors, under the MIT License. Source and license: https://github.com/chartjs/Chart.js/tree/v4.5.1

The dashboard also bundles chartjs-plugin-zoom 2.2.0 and Hammer.js 2.0.8 under the MIT License. Their license texts are retained under `licenses/`.

Hypernet Sentinel does not imply endorsement by or affiliation with Cloudflare, the upstream CLI author, or Chart.js contributors.
