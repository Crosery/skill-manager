---
module: core::resource
file: src/core/resource.rs
role: domain-types
---

# resource

## Purpose
Domain types for "something runai can enable/disable": `Resource`, `ResourceKind` (Skill / Mcp), `Source` (where it came from), `UsageStat`.

## Public API
- `enum ResourceKind { Skill, Mcp }` with `as_str()` / `from_str()`.
- `enum Source` — Local / Github(owner, repo, branch) / Market(url) / … Used by DB to round-trip provenance.
  - `to_meta_json(&self) -> String` + `from_meta_json(type, meta)` — DB serialization.
- `struct Resource { id, name, kind, description, source, enabled_for: HashMap<CliTarget, bool>, created_at, … }`.
  - `Resource::generate_id(source, name) -> String` — deterministic ID (sha256 prefix + name).
  - `resource.is_enabled_for(target) -> bool`.
- `struct UsageStat { id, name, count, last_used_at }`.
- `fn format_time_ago(ts: Option<i64>) -> String` — `"3h ago"` / `"just now"` etc. for CLI output.

## Key invariants
- `Resource::enabled_for` is cosmetic/runtime-derived — **persistence of enable state is filesystem** (symlink / config entry), not this field. Only fill it before presenting.
- `generate_id` must be stable across versions — identity depends on it for DB rows and group membership.

## Touch points
- **Upstream**: `Database` for rows, `SkillManager`, `scanner`, `mcp_discovery` for construction, MCP `sm_list` for output.
- **Downstream**: `CliTarget`, sha2.

## Gotchas
- `format_time_ago` takes `Option<i64>`; `None` → `"never"`. Don't pass `0` as "never".
- When adding a new `Source` variant: update `source_type()`, both `to_meta_json` and `from_meta_json`, DB migrations, and group-suggestion classifier.
