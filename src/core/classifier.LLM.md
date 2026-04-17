---
module: core::classifier
file: src/core/classifier.rs
role: heuristic
---

# classifier

## Purpose
Heuristic group-suggestion engine. Given a resource's name + description (and optionally source URL), return a ranked list of group ids it should belong to (e.g. `rust`, `testing`, `github`, `frontend`).

## Public API
- `Classifier::suggest_groups(name, description) -> Vec<String>`.
- `Classifier::suggest_groups_with_source(name, description, source) -> Vec<String>` — also uses source repo owner/name for hints.

## Key invariants
- Pure function — no I/O, no DB. Safe to call from anywhere.
- Returns group ids in confidence order. Caller decides how many to keep (`auto_group` typically takes top N).
- Empty input → empty result, not an error.

## Touch points
- **Upstream**: `auto_group`, `manager::get_suggested_groups`, installer (post-install grouping), MCP tools.
- **Downstream**: none (pure logic).

## Gotchas
- Keyword lists are hand-tuned — edits here can shift thousands of auto-group assignments. Prefer adding new keywords over removing existing ones.
- Keep synonyms case-insensitive; input strings are lowercased before matching.
