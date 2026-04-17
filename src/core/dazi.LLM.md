---
module: core::dazi
file: src/core/dazi.rs
role: feature-gated
feature: dazi
---

# dazi (feature = "dazi")

## Purpose
HTTP client + caches + session manager for the internal Dazi marketplace at `dazi.ktvsky.com`. Three resource types: Skills (ZIP), Agents (JSON → SKILL.md), Bundles (batch install). Only compiled in when the `dazi` cargo feature is active; separate `v*-dazi` tagged release channel.

## Public API (large — ~30 pub items)

**Models**: `DaziSkill`, `DaziAgent`, `DaziBundle`, `DaziKind {Skill, Agent, Bundle}` (with `next/prev/label` for TUI cycling), `DaziMcpConfig(Inner)`, `CachedToken { is_valid() }`, `DaziSession`, `SessionInfo/Data/User`, `TeamInfo`, `PublishResult`, `BundlePublishSummary/Result`.

**Cache I/O**: `load_cache_{skills,agents,bundles}` / `save_cache_{skills,agents,bundles}`.

**Token I/O**: `load_token` / `save_token` / `refresh_token_blocking`.

**Session I/O**: `load_session` / `save_session` / `clear_session`.

**MCP registration**: `register_dazi_mcp(home, url, token)` / `unregister_dazi_mcp(home)` — targets all four CLIs, writes a `dazi-marketplace` entry.

**Markers**: `mark_installed_{skills,agents}(items, installed_names)` — flip `installed` field for display.

**Client**: `DaziClient::new()` / `with_url(base)` — the HTTP caller, many `async fn` methods for endpoints (search/install/publish/login/bundles).

## Key invariants
- Cache files under `~/.runai/dazi-cache/` with 1h TTL (mtime-based).
- Token (`dazi-token.json`) auto-refreshes every 10 min while TUI is open; 10-day validity from server.
- Session (`dazi-session.json`) for team operations, 7-day validity, auto-renew on use.
- ZIP extraction strips common prefix (e.g. `docx/SKILL.md` → `SKILL.md`) — essential because marketplace archives often nest the skill dir.
- `DAZI_BASE_URL` env overrides the server (for testing / private deployments).

## Touch points
- **Upstream**: `mcp::dazi_tools` (12 tools), TUI "搭子" tab, `cli::MarketInstall` when source is Dazi.
- **Downstream**: `reqwest`, `tokio`, `serde_json`, `zip`, `mcp_register` (for the `dazi-marketplace` entry), `tempfile`.

## Gotchas
- `is_valid()` on `CachedToken` checks **expiry with a small skew**. Don't trust "not yet expired" — if you're about to do an operation, refresh preemptively.
- Publishing agents (not skills) goes through a different endpoint; `sm_dazi_publish_agent` and `sm_dazi_publish` are not interchangeable.
- Bundle publish is team-scoped and **requires an active session** (`sm_dazi_login` first) — bare token isn't enough.
- Non-dazi builds **must not** reference any symbol from this module — guard call sites with `#[cfg(feature = "dazi")]`.
