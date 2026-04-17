---
module: core::mcp_register
file: src/core/mcp_register.rs
role: runtime
---

# mcp_register

## Purpose
Make runai show up as an MCP server inside every supported CLI's config on first launch. Writes format-specific entries into Claude JSON / Codex TOML / Gemini JSON / OpenCode JSON, idempotently.

## Public API
- `McpRegister::register_all(home) -> RegisterResult` — registers in all four target configs. `RegisterResult { registered, skipped, errors }`.
- `McpRegister::is_registered(home, rel_path) -> bool` — quick check without writing.
- `McpRegister::migrate_all(home) -> usize` — renames legacy `skill-manager` entries to `runai`.
- `McpRegister::unregister_all(home)` — removes every registered entry.

## Key invariants
- **Idempotent**: re-running does nothing if entry already points at the current binary path; updates the path if it moved.
- Binary path is `std::env::current_exe()`; fallback literal `"runai"` only if that fails.
- Each CLI uses a distinct schema — do not share code paths:
  - Claude: `mcpServers` at root of `.claude.json`, `{command: string, args: [..]}`
  - Codex: `[mcp_servers.<name>]` table in `.codex/config.toml`
  - Gemini: `mcpServers` in `.gemini/settings.json` (same shape as Claude)
  - OpenCode: `mcp` (not `mcpServers`), `command` is an **array of strings** not a single string, in `.config/opencode/opencode.json`

## Touch points
- **Upstream**: `main.rs` first-launch detection, `runai register` / `unregister` CLI commands, `SkillManager` migrations.
- **Downstream**: `std::fs`, `serde_json`, `toml`, `dirs::home_dir` (via passed-in `home: &Path`).

## Gotchas
- Tests pass `tmp.path()` directly as `home` — **do not** call `dirs::home_dir()` inside, always accept `home: &Path`.
- OpenCode's `command` schema is load-bearing: JSON writer must emit `["runai", "mcp-serve"]`, not `"runai mcp-serve"`. Mistake breaks OpenCode silently.
- Test fixtures that build JSON with `format!` around a path will produce invalid JSON on Windows (unescaped `\`). Use `serde_json::json!` macro.
