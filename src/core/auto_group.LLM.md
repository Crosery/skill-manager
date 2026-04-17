---
module: core::auto_group
file: src/core/auto_group.rs
role: runtime
---

# auto_group

## Purpose
Inspect all existing resources and suggest/apply groups based on name + description heuristics. Used at first-launch adoption and as a one-shot `runai group auto` operation.

## Public API
- `AutoGroup::auto_group_all(mgr) -> AutoGroupResult` — scans, assigns, writes to DB + group TOML files.
- `AutoGroup::preview(resources) -> HashMap<group_id, Vec<resource_name>>` — dry-run view.

## Key invariants
- Uses `classifier::suggest_groups_with_source` — all heuristic logic lives there, this is orchestration only.
- Never creates groups that already exist with conflicting descriptions; merges silently.
- Results are idempotent — running twice is a no-op for already-grouped resources.

## Touch points
- **Upstream**: `runai group auto` (if exposed), first-launch in `SkillManager`, MCP tools.
- **Downstream**: `classifier::Classifier`, `manager::create_group` / `add_group_member`.

## Gotchas
- The preview is cheap; the apply path writes to both `groups/*.toml` files and DB in the same call — partial failures should be reported in `AutoGroupResult.errors`, not swallowed.
