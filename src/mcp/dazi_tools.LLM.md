---
module: mcp::dazi_tools
file: src/mcp/dazi_tools.rs
role: mcp-server
feature: dazi
---

# mcp::dazi_tools (feature = "dazi")

## Purpose
12 additional MCP tools exclusive to the Dazi marketplace. Only compiled when `feature = "dazi"`. Exposes Dazi browsing, install, publish, and session management via MCP.

## Tool list

**Browse**: `sm_dazi_search`, `sm_dazi_list`, `sm_dazi_stats`.

**Install**: `sm_dazi_install`, `sm_dazi_install_bundle`.

**Publish** (requires session): `sm_dazi_publish`, `sm_dazi_publish_agent`, `sm_dazi_publishable`, `sm_dazi_publish_bundle`.

**Session**: `sm_dazi_login` (starts local HTTP server + opens browser), `sm_dazi_logout`.

**Cache**: `sm_dazi_refresh` (reload caches + refresh token).

## Key invariants
- Everything goes through `DaziClient` (in `core::dazi`). Never hand-craft HTTP calls here — the client handles auth headers, retries, base-URL override.
- Publish tools **require an active session** — return `{ok: false, error: "login required"}` if not logged in. Don't silently 401.
- `sm_dazi_login` spins up a short-lived HTTP server on localhost to receive the OAuth callback; tool returns after receiving the callback. Don't call it concurrently.

## Touch points
- **Upstream**: MCP clients (same transport as `tools.rs`).
- **Downstream**: `core::dazi::DaziClient`, `register_dazi_mcp` / `unregister_dazi_mcp`, session/token file I/O.

## Gotchas
- All bodies of functions in this file are inside `#[cfg(feature = "dazi")]` blocks — don't add non-Dazi logic here.
- Tool count for release notes: add the 12 Dazi tools to the 30 base tools → **42 total** in Dazi builds. Keep in sync with `AGENTS.md` if updated.
