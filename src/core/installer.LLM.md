---
module: core::installer
file: src/core/installer.rs
role: runtime
---

# installer

## Purpose
GitHub-based skill installer. Parses `owner/repo[@branch]` inputs, downloads tarballs, extracts into `~/.runai/skills/`. The MCP variant of the install pipeline lives in `manager::install_github_repo` which delegates here.

## Public API
- `struct InstallResult { installed: Vec<String>, skipped: Vec<String>, errors: Vec<String> }`.
- `Installer::parse_github_source(input) -> Result<(owner, repo, branch)>` — accepts `owner/repo`, `owner/repo@branch`, full `https://github.com/owner/repo/` URLs, and URLs with `@branch` suffix.

## Key invariants
- Extraction strips the top-level `<repo>-<sha>/` prefix from tarballs so skills land at `skills/<skill-name>/` not `skills/<repo>-<sha>/<skill-name>/`.
- Install is atomic-per-skill: either the skill's final dir exists and is valid, or nothing was written for that skill.
- Conflict resolution: if `skills/<name>/` already exists, installer reports skip (not overwrite) — explicit `runai uninstall <name>` is required.

## Touch points
- **Upstream**: `cli::mod::Install`, `manager::install_github_repo`, MCP `sm_install`, market install path.
- **Downstream**: `reqwest`, `flate2::read::GzDecoder`, `tar`, `tempfile`.

## Gotchas
- `parse_github_source` tolerates trailing slashes and `.git` suffix — keep those cases exercised if you refactor.
- Network errors are reported via `InstallResult.errors`, not bubbled up — the caller decides whether to abort or continue the batch.
