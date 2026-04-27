---
module: core::doctor
file: src/core/doctor.rs
role: diagnostic
---

# doctor

## Purpose
Run a battery of environment checks — `runai doctor` output. Inspects `~/.runai/` layout, MCP registration in each CLI config, file permissions, DB reachability, etc.

## Public API
- `struct CheckResult { name, status, detail }`, `enum CheckStatus { Pass, Warn, Fail }`.
- `CheckStatus::icon() -> &str` — `"✓" / "⚠" / "✗"`.
- `run_doctor() -> Vec<CheckResult>` — runs every check and returns results in display order.
- `run_doctor_fix() -> FixReport { broken_symlinks_removed, dedupe_rows_removed }` — repair pass triggered by `runai doctor --fix`. Walks `~/.{claude,codex,gemini,opencode}/skills/`, removes symlinks where `path.exists() == false` (target gone), then re-runs `Database::dedupe_skills_by_name()`. The same dedupe also runs silently in `SkillManager::new()/with_base()`, so `--fix` typically reports zero rows removed — it exists as the explicit recovery surface for state that drifted mid-session.

## Key invariants
- `run_doctor` is read-only — users should feel safe running it anytime.
- `run_doctor_fix` is the ONLY mutating surface in this module; never call it from anywhere except the `--fix` flag handler. The dedupe SQL it triggers is idempotent and bounded; the symlink prune is filesystem-bounded to the four CLI skills dirs (no walk into the user home root).
- Every `Fail` must include a `detail` string suggesting a specific fix (`"Run 'runai register' to fix"`).

## Touch points
- **Upstream**: `runai doctor` CLI subcommand; MCP `sm_status` / `sm_doctor` may call a subset.
- **Downstream**: `mcp_register::is_registered`, `paths::data_dir`, `Database::open`.

## Gotchas
- New checks go in `run_doctor`'s body — keep them ordered by "user-fixable-first". Deep system checks last.
- `Warn` vs `Fail`: use Warn for optional features (e.g. a non-default CLI target config missing), Fail for things that prevent basic operation (missing `~/.runai/` write permission).
