---
module: core::group
file: src/core/group.rs
role: domain-types
---

# group

## Purpose
Group definition. Each group lives as a TOML file under `~/.runai/groups/<id>.toml`. A group has a display name, a description, a kind (default/ecosystem/custom), and a list of members (each either a Skill or an MCP).

## Public API
- `enum GroupKind { Default, Ecosystem, Custom }`.
- `enum MemberType { Skill, Mcp }`.
- `struct GroupMember { name, member_type }`.
- `struct Group { name, description, kind, auto_enable, members }`.
  - `to_toml` / `from_toml` — round-trip strings.
  - `save_to_file(path)` / `load_from_file(path)` — convenience.

## Key invariants
- **Members are by `name`, not by resource id.** Name is what shows up in MCP tools; id can change if source moves.
- `auto_enable`: if true, newly-adopted resources matching the group's classifier get auto-enabled. Default groups (e.g. "default") set this to true.
- DB also tracks group membership by resource-id for fast lookup — `Group.toml` is the source of truth, DB is the index. Always write both (`manager::create_group`).

## Touch points
- **Upstream**: `SkillManager::{create_group, list_groups, update_group, ...}`, `Database::{add_group_member, remove_group_member, get_group_members}`, TUI groups tab.
- **Downstream**: `toml`, `serde`.

## Gotchas
- Don't store enable state on a group — enable is per-resource-per-target, and `enable_group(target)` just iterates members.
- `auto_enable` only triggers at adoption time (scanner/installer); editing it after the fact doesn't retroactively enable already-scanned resources.
