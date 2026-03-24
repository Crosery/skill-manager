# Skill Manager

**English** | [中文](README_zh.md)

A terminal-based resource manager for AI CLI skills, MCP servers, and groups. Works across **Claude Code**, **Codex**, **Gemini CLI**, and **OpenCode**.

## Features

- **TUI Interface** — Browse, enable/disable, search skills and MCPs with a terminal UI
- **Multi-CLI Support** — Manage resources across 4 AI CLIs, switch targets with `1234`
- **Groups** — Organize skills/MCPs into groups, batch enable/disable
- **Market** — Browse 2000+ skills from 5 built-in sources, add custom GitHub sources
- **MCP Server** — 22 tools exposed via MCP protocol, auto-registered to all CLIs
- **Backup & Restore** — Timestamped full backups of skill directories and configs, safe rollback
- **CLI** — Subcommands for scripting and automation

## Install

```bash
git clone https://github.com/Crosery/skill-manager.git
cd skill-manager
cargo build --release
cp target/release/skill-manager ~/.local/bin/
```

## Quick Start

```bash
# Launch TUI (first run will scan and register MCP automatically)
skill-manager

# Or use CLI directly
skill-manager list                    # List all skills
skill-manager status                  # Show enabled counts
skill-manager enable brainstorming    # Enable a skill
skill-manager scan                    # Scan for new skills
skill-manager backup                  # Create a backup
skill-manager restore                 # Restore from latest backup
```

## TUI Keybindings

| Key | Action |
|-----|--------|
| `H/L` or `Tab` | Switch tabs (Skills / MCPs / Groups / Market) |
| `j/k` | Navigate up/down |
| `Space` | Toggle enable/disable |
| `1234` | Switch CLI target (Claude/Codex/Gemini/OpenCode) |
| `/` | Search |
| `Enter` | Open group detail / Install from market |
| `d` | Delete selected item |
| `c` | Create new group |
| `a` | Add to group (Skills/MCPs tab) |
| `s` | Sources manager (Market tab) / Scan (other tabs) |
| `[ ]` | Switch market source |
| `q` | Quit |

## MCP Tools

When running as MCP server (`skill-manager mcp-serve`), 22 tools are available:

**Skills & MCPs**

| Tool | Description |
|------|-------------|
| `sm_list` | List skills/MCPs with filters (kind, group) |
| `sm_status` | Enabled/total counts per CLI target |
| `sm_enable` / `sm_disable` | Toggle skill/MCP for a CLI |
| `sm_delete` | Remove a skill/MCP (files + symlinks + DB) |
| `sm_scan` | Scan CLI directories for new skills |
| `sm_batch_enable` / `sm_batch_disable` | Batch toggle multiple by name list |

**Groups**

| Tool | Description |
|------|-------------|
| `sm_groups` | List all groups with member counts |
| `sm_create_group` / `sm_delete_group` | Create or delete a group |
| `sm_group_add` / `sm_group_remove` | Add/remove members |
| `sm_group_enable` / `sm_group_disable` | Batch toggle all members in a group |

**Market**

| Tool | Description |
|------|-------------|
| `sm_market` | Browse cached market skills (filter by source/search) |
| `sm_market_install` | Install single skill (downloads full directory) |
| `sm_sources` | List/add/remove/enable/disable market sources |

**Backup & Utility**

| Tool | Description |
|------|-------------|
| `sm_backup` | Create timestamped backup of all skill dirs and configs |
| `sm_restore` | Restore from backup (latest or by timestamp) |
| `sm_backups` | List all available backups |
| `sm_register` | Register MCP to all CLI configs |

The MCP server auto-registers to `~/.claude.json`, `~/.codex/settings.json`, `~/.gemini/settings.json`, and `~/.opencode/settings.json` on first launch.

## Market Sources

Built-in sources (enable/disable via `s` on Market tab):

| Source | Skills | Default |
|--------|--------|---------|
| Anthropic Official | 23 | Enabled |
| Everything Claude Code | 125 | Enabled |
| Terminal Skills | 900+ | Disabled |
| Antigravity Skills | 1300+ | Disabled |
| OK Skills | 55 | Disabled |

Add custom sources with `a` (format: `owner/repo` or `owner/repo@branch`).

## Backup & Restore

Skill Manager creates timestamped backups at `~/.skill-manager/backups/{timestamp}/`:

```
backups/20260324_195000/
├── claude-skills/      # Full copy of ~/.claude/skills/ (symlinks preserved)
├── codex-skills/       # Full copy of ~/.codex/skills/
├── claude.json         # Copy of ~/.claude.json
├── gemini-settings.json
└── timestamp           # Marker file
```

```bash
skill-manager backup                        # Create backup now
skill-manager restore                       # Restore latest
skill-manager restore --timestamp 20260324_195000  # Restore specific
```

First scan automatically creates a backup before making any changes.

## Data

All data stored in `~/.skill-manager/`:
- `skills/` — Managed skill directories (each with SKILL.md)
- `groups/` — Group definitions (TOML files)
- `backups/` — Timestamped full backups
- `market-cache/` — Cached market skill lists (JSON, 1hr TTL)
- `market-sources.json` — Custom market sources
- `skill-manager.db` — SQLite database (resources, targets, group members)

## License

MIT
