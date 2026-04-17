---
module: core::linker
file: src/core/linker.rs
role: runtime
---

# linker

## Purpose
Cross-platform symlink wrapper. runai's "enabled skill" == "symlink exists pointing into `~/.runai/skills/<name>`"; this module is the only place that creates/removes/introspects those links.

## Public API
- `Linker::create_link(target, link)` — symlinks a directory. `cfg(unix)` → `os::unix::fs::symlink`, `cfg(windows)` → `os::windows::fs::symlink_dir`.
- `Linker::remove_link(link)` — unix `remove_file`, windows `remove_dir`. Only acts if path is a symlink.
- `Linker::is_symlink(path)` — uses `symlink_metadata` (does **not** follow links).
- `Linker::is_our_symlink(path, our_base)` — resolves the link target and checks `starts_with(our_base)`.
- `Linker::detect_entry_type(path, our_base) -> EntryType` — `OurSymlink` / `ForeignSymlink` / `RealDir` / `NotExists`.
- `Linker::adopt_to_managed(src, managed_dir, link_path)` — move src → managed_dir, then relink. Used by scanner adoption.
- `Linker::move_dir` / `copy_dir_recursive` — rename with cross-filesystem fallback via recursive copy.

## Key invariants
- Symlink **target is the managed dir**, never a file. `symlink_dir` (Windows API) required — `symlink_file` would fail silently on directory targets.
- `adopt_to_managed` always removes the link first if it already exists, even if pointing at the correct place — simpler and cheap.
- `detect_entry_type`: a dangling symlink reports as `OurSymlink` or `ForeignSymlink`, not `NotExists`.

## Touch points
- **Upstream**: `scanner.rs::adopt_entry`, `manager.rs::enable_resource/disable_resource`, `backup.rs::copy_dir_preserving_symlinks`.
- **Downstream**: `std::os::{unix,windows}::fs` platform-gated APIs, `std::fs::{rename, read_link, symlink_metadata}`.

## Gotchas
- Windows `symlink_dir` requires **Developer Mode** or **Administrator** — otherwise `ERROR_PRIVILEGE_NOT_HELD`. Document this in README.
- `std::fs::rename` fails across filesystems — `move_dir` falls back to recursive copy. Don't remove the fallback.
- `is_our_symlink` resolves via `read_link`; if the link target is relative, it's joined against the link's parent dir.
