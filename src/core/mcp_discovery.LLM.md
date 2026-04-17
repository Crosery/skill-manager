---
module: core::mcp_discovery
file: src/core/mcp_discovery.rs
role: runtime
---

# mcp_discovery

## Purpose
Read each CLI's config file and extract its registered MCP servers. Used whenever runai needs to answer "what MCPs exist" — which is always, because MCP state is never cached in the DB.

## Public API
- `struct McpEntry { name, cli_target, command, args, enabled, kind: McpType }`.
- `enum McpType { Stdio, Sse, Http, Remote, Unknown }` — best-effort classification from config shape.
- `McpDiscovery::discover_all(home) -> Vec<McpEntry>` — reads all four CLI configs, produces a unified list.

## Key invariants
- **Read-only.** Discovery must never modify configs — that belongs to `mcp_register`.
- `enabled = !config.disabled.unwrap_or(false)` — an absent `disabled` field means enabled. Do not flip this polarity.
- Missing config file is fine — returns empty list for that CLI, no error.

## Touch points
- **Upstream**: `SkillManager::list_resources` (for MCPs), `SkillManager::status`, MCP `sm_list`, `doctor`.
- **Downstream**: `std::fs::read_to_string`, `serde_json`, `toml`, `CliTarget::mcp_config_path`.

## Gotchas
- Codex uses TOML (`[mcp_servers.<name>]`) — different parser path from the three JSON CLIs. Don't unify prematurely.
- OpenCode's `command` is an array; when reading, join with `' '` for display but preserve the array when forwarding.
- An MCP with the same name in two CLIs = **one McpEntry per CLI**, not one merged entry. Merging happens in `manager::list_resources` which dedupes by name.
