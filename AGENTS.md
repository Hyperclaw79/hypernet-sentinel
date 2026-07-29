# Contributor guidance

- Preserve Hypernet Sentinel as an independent application. Do not restructure it as an upstream fork or import upstream CLI/TUI product scope.
- Never edit `crates/measurement-core/src/upstream/` manually. Change `scripts/update_upstream.py`, rerun it, and review the complete generated diff.
- Keep application semantics in Sentinel-owned modules. Upstream zero-valued failed measurements must not become successful zero records.
- Never add arbitrary targets, shell execution, raw SQL, Docker access, or configuration mutation to HTTP or MCP interfaces.
- Scheduled, manual, API, and MCP diagnostics must share one coordinator and one concurrency lock.
- Do not add authentication without an explicit deployment decision. The current MCP route intentionally mirrors Hyperlab Docs' unauthenticated Streamable HTTP model.
- Upstream synchronization commits directly only after full validation. Do not create automated PRs or silently reconcile concurrent changes.
- Normal tests must not consume Internet bandwidth.
- Before handing off changes, run formatting, Clippy with warnings denied, workspace tests, and a production container build.
