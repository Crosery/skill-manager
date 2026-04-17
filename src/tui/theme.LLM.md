---
module: tui::theme
file: src/tui/theme.rs
role: tui-style
---

# tui::theme

## Purpose
Dark/light theme palettes. Exposes named styles (`primary`, `accent`, `muted`, `warn`, `error`, `selected`, `tab_active`, `tab_inactive`, …) and a toggle keybind handler.

## Public API
- `enum ThemeMode { Dark, Light }`.
- `struct Theme { mode, colors: HashMap<Role, Color> or similar }`.
- `Theme::dark()` / `Theme::light()`.
- `Theme::toggle(&mut self)`.
- Lookup methods: `theme.primary_style()`, `theme.accent_style()`, etc.

## Key invariants
- Colors **tuned for both light and dark terminals** — don't introduce a color that only looks OK on one side.
- No `Color::Rgb` hardcodes in `ui.rs` — go through `Theme` so a mode switch actually changes everything.
- Theme mode persists per-session only (not written to disk). If we later add persistence, put it under `AppPaths::config_path()`.

## Touch points
- **Upstream**: `tui::app` owns the `Theme`, hot-key `t` calls `toggle`.
- **Downstream**: `ratatui::style::Style`.

## Gotchas
- `Role` enum exhaustiveness matters — adding a new visual role without updating both `dark()` and `light()` yields surprising defaults.
- Avoid using `Color::Yellow` as a "dark" color — it reads poorly on light backgrounds. Prefer accent tokens from the theme.
