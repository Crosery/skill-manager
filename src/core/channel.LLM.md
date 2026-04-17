---
module: core::channel
file: src/core/channel.rs
role: support
---

# channel

## Purpose
Release channel / market-source management — a list of named sources (e.g. "stable", "community") the user can add/remove/enable. Persisted as TOML under `~/.runai/`.

## Public API
- `struct Channel { name, url, description }` and `ChannelEntry` / `ChannelConfig`.
- `ChannelConfig::default_config()` — bundled list of well-known sources.
- `load(path)` / `save(&self, path)` — TOML round-trip.
- `add_channel(name, url, description)` / `remove_channel(idx)`.

## Key invariants
- Channel list is ordered — UI shows them in list order. `remove_channel(idx)` shifts following entries.
- `default_config` is merged (not overwritten) on load if the config file doesn't yet have the expected defaults — do not add a destructive "reset" without user opt-in.

## Touch points
- **Upstream**: TUI sources tab, `runai sources` CLI (if present), MCP `sm_sources`.
- **Downstream**: `toml`, `market::load_sources` may read the same config.

## Gotchas
- Distinct from `market::SourceEntry` (market's own cache model). Channel is user-facing curated list; SourceEntry is runtime per-repo index.
