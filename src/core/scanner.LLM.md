---
module: core::scanner
file: src/core/scanner.rs
role: runtime
---

# scanner

## Purpose
Two jobs. (1) **Discover** — recursively walk a directory finding `SKILL.md`, classify each hit. (2) **Scan & adopt** — given a CLI's skills dir, take ownership of unmanaged entries by moving them under `~/.runai/skills/` and replacing with a symlink.

## Public API
- `Scanner::discover_skills(root) -> Vec<DiscoveredSkill>` — recursive. Filters out plugin/backup/VS-Code noise paths. Classifies each as `Managed` / `CliDir` / `Unmanaged`.
- `Scanner::scan_all(paths, db) -> ScanResult` — for every `CliTarget`, call `scan_cli_dir`.
- `Scanner::scan_cli_dir(...)` — iterate entries; `adopt_entry` decides: move real dirs under management, heal matching-name broken symlinks, leave orphan symlinks alone.
- `Scanner::extract_description(dir)` — parse `SKILL.md` frontmatter `description:`; fall back to first non-empty body line.

## Key invariants
- **Never auto-runs on startup.** User must invoke `scan` / `discover` explicitly — avoids clobbering existing symlinks.
- Orphan broken symlinks (no matching managed skill) are **left intact**, counted as skipped. Only broken symlinks whose basename matches a managed skill get healed (relinked to the managed dir).
- `NOISE_PATHS` compared against `path_str.replace('\\', '/')` — do **not** regress to raw `to_string_lossy()`, breaks on Windows.
- `walk_for_skills` depth cap = 8 levels, prevents runaway recursion.

## Touch points
- **Upstream**: `runai scan` / `runai discover` (cli/mod.rs), `SkillManager::scan` (manager.rs).
- **Downstream**: `Linker` for symlink operations, `Database` for insert/update.

## Gotchas
- `path_str.contains("/plugins/marketplaces/")` style — literal `/` checks. Normalized to forward slashes before comparison so Windows `\` paths match too.
- Symlink test fixtures have both `cfg(unix)` and `cfg(windows)` branches (`symlink` vs `symlink_dir`).
- Classification as `CliDir` depends on `/.claude/skills/` etc. substring — keep in sync with `CliTarget::skills_dir()`.
