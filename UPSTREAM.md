# Upstream measurement source

Hypernet Sentinel is independent from [`kavehtehrani/cloudflare-speed-cli`](https://github.com/kavehtehrani/cloudflare-speed-cli). It imports selected GPL-3.0 source because upstream has no reusable library target.

## Imported boundary

`crates/measurement-core/src/upstream/` contains:

- `src/engine/*.rs`
- `src/constants.rs`
- `src/metrics.rs`
- `src/model.rs`
- `src/stats.rs`

CLI, TUI, clipboard, local history, export, updater, and terminal formatting code are excluded. `UPSTREAM_REVISION` records the exact commit and `MANIFEST.json` records every imported file hash.

The Sentinel adapter owns the stable result model, configuration policy, validation of missing measurements, cancellation integration, and quality-only execution path.

## Refresh process

```powershell
python scripts/update_upstream.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
docker build -t hypernet-sentinel:upstream-validation .
```

The updater:

1. Resolves upstream `main` to a full commit.
2. Downloads that immutable archive into a temporary directory.
3. Builds a complete allowlisted snapshot rather than overlaying files.
4. Applies narrow, fail-closed SDK visibility and Windows-test adaptations.
5. Normalizes every imported source to UTF-8 with LF line endings, hashes those exact bytes, and replaces the previous snapshot only after the complete snapshot is staged.

Unexpected layout, missing files, ambiguous patch locations, test failure, lint failure, or container failure prevents acceptance.

The scheduled GitHub workflow commits directly to Hypernet Sentinel `main` only after full validation. Before pushing, it verifies that remote `main` still equals the revision that was validated; concurrent changes cause a failure rather than a merge, rebase, overwrite, or silent reconciliation. No PR is created against either repository.

After a validated sync is committed, the workflow explicitly dispatches the trusted build for that Sentinel commit. The build publishes one immutable `sha-<40-character-Sentinel-commit>` image. Stable semantic releases promote that already-tested image to the release version tags and `latest` without rebuilding it. No `main` or `upstream-*` image tags are published.
