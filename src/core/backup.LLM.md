---
module: core::backup
file: src/core/backup.rs
role: runtime
---

# backup

## Purpose
Timestamped snapshot + restore of managed data (`skills/`, `mcps/`, `groups/`) **and** every CLI's config file at the time of backup. Preserves symlinks as symlinks (not dereferenced).

## Public API
- `create_backup(paths) -> PathBuf` — writes to `<data_dir>/backups/<YYYYMMDD_HHMMSS>/`, returns the dir.
- `list_backups(paths) -> Vec<String>` — timestamps, newest first.
- `restore_backup(paths, timestamp) -> usize` — returns number of items restored.
- `has_backup(paths) -> bool`.

## Key invariants
- **Symlinks are preserved** — the copy walker reads `read_link` and re-creates the symlink on the destination side (platform-gated). Never dereferences.
- Backup is self-contained: re-inflating a backup dir onto a fresh machine rebuilds all symlinks pointing into the new `~/.runai/skills/`.
- CLI config files are copied verbatim — backup is safe to restore even if you deleted/modified `.claude.json` since.

## Touch points
- **Upstream**: `runai backup` / `runai restore` subcommands, MCP `sm_backup` / `sm_restore` tools.
- **Downstream**: `Linker::is_symlink`, `std::fs::read_link`, platform symlink APIs.

## Gotchas
- `copy_dir_preserving_symlinks` uses platform `cfg` branches for symlink creation — keep unix/windows in sync.
- Restore does **not** remove extra files that live in the current dirs but not the backup — it overlays. If you need a clean restore, delete `~/.runai/` first.
- `create_backup_impl` takes an explicit `home` param — tests pass `tmp.path()` to keep I/O sandboxed.
