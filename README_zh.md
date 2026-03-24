# Skill Manager

[English](README.md) | **中文**

终端界面的 AI CLI skill/MCP 资源管理器。支持 **Claude Code**、**Codex**、**Gemini CLI** 和 **OpenCode**。

## 功能特性

- **TUI 终端界面** — 浏览、启用/禁用、搜索 skills 和 MCPs
- **多 CLI 支持** — 跨 4 个 AI CLI 统一管理，`1234` 切换目标
- **分组管理** — 将 skills/MCPs 组织成组，批量启用/禁用
- **Skill 市场** — 浏览 2000+ 来自 5 个内置源的 skills，支持自定义 GitHub 源
- **MCP 服务器** — 22 个工具通过 MCP 协议暴露，首次启动自动注册到所有 CLI
- **备份与恢复** — 带时间戳的完整备份，安全回滚
- **命令行** — 子命令支持脚本自动化

## 安装

```bash
git clone https://github.com/Crosery/skill-manager.git
cd skill-manager
cargo build --release
cp target/release/skill-manager ~/.local/bin/
```

## 快速开始

```bash
# 启动 TUI（首次运行会自动扫描并注册 MCP）
skill-manager

# 或直接使用 CLI
skill-manager list                    # 列出所有 skills
skill-manager status                  # 查看启用数量
skill-manager enable brainstorming    # 启用某个 skill
skill-manager scan                    # 扫描新 skills
skill-manager backup                  # 创建备份
skill-manager restore                 # 从最近备份恢复
```

## TUI 快捷键

| 按键 | 操作 |
|------|------|
| `H/L` 或 `Tab` | 切换标签页（Skills / MCPs / Groups / Market） |
| `j/k` | 上下导航 |
| `Space` | 启用/禁用 |
| `1234` | 切换 CLI 目标（Claude/Codex/Gemini/OpenCode） |
| `/` | 搜索 |
| `Enter` | 打开分组详情 / 从市场安装 |
| `d` | 删除选中项 |
| `c` | 创建新分组 |
| `a` | 添加到分组（Skills/MCPs 页） |
| `s` | 源管理（Market 页）/ 扫描（其他页） |
| `[ ]` | 切换市场源 |
| `q` | 退出 |

## MCP 工具

作为 MCP 服务器运行时（`skill-manager mcp-serve`），提供 22 个工具：

**Skills 和 MCPs**

| 工具 | 说明 |
|------|------|
| `sm_list` | 列出 skills/MCPs（支持按类型、分组过滤） |
| `sm_status` | 各 CLI 的启用/总数统计 |
| `sm_enable` / `sm_disable` | 启用/禁用 skill/MCP |
| `sm_delete` | 删除 skill/MCP（文件 + 软链接 + 数据库） |
| `sm_scan` | 扫描目录发现新 skills |
| `sm_batch_enable` / `sm_batch_disable` | 批量启用/禁用多个 |

**分组**

| 工具 | 说明 |
|------|------|
| `sm_groups` | 列出所有分组及成员数 |
| `sm_create_group` / `sm_delete_group` | 创建/删除分组 |
| `sm_group_add` / `sm_group_remove` | 管理分组成员 |
| `sm_group_enable` / `sm_group_disable` | 批量启用/禁用分组内所有成员 |

**市场**

| 工具 | 说明 |
|------|------|
| `sm_market` | 浏览缓存的市场 skills（按源/关键词过滤） |
| `sm_market_install` | 从市场安装单个 skill（下载完整目录） |
| `sm_sources` | 列出/添加/删除/启用/禁用市场源 |

**备份与工具**

| 工具 | 说明 |
|------|------|
| `sm_backup` | 创建带时间戳的完整备份 |
| `sm_restore` | 从备份恢复（默认最新，可指定时间戳） |
| `sm_backups` | 列出所有可用备份 |
| `sm_register` | 注册 MCP 到所有 CLI 配置 |

MCP 服务器会在首次启动时自动注册到 `~/.claude.json`、`~/.codex/settings.json`、`~/.gemini/settings.json` 和 `~/.opencode/settings.json`。

## 市场源

内置源（在 Market 标签页按 `s` 管理）：

| 源 | Skills 数量 | 默认状态 |
|----|------------|----------|
| Anthropic Official | 23 | 启用 |
| Everything Claude Code | 125 | 启用 |
| Terminal Skills | 900+ | 禁用 |
| Antigravity Skills | 1300+ | 禁用 |
| OK Skills | 55 | 禁用 |

按 `a` 添加自定义源（格式：`owner/repo` 或 `owner/repo@branch`）。

## 备份与恢复

Skill Manager 在 `~/.skill-manager/backups/{时间戳}/` 创建完整备份：

```
backups/20260324_195000/
├── claude-skills/          # ~/.claude/skills/ 的完整副本（保留软链接）
├── codex-skills/           # ~/.codex/skills/ 的完整副本
├── claude.json             # ~/.claude.json 的副本
├── gemini-settings.json
└── timestamp               # 时间戳标记
```

```bash
skill-manager backup                              # 立即创建备份
skill-manager restore                              # 从最新备份恢复
skill-manager restore --timestamp 20260324_195000  # 恢复指定版本
```

首次扫描前会自动创建备份。

## 数据存储

所有数据存储在 `~/.skill-manager/`：
- `skills/` — 托管的 skill 目录（每个包含 SKILL.md）
- `groups/` — 分组定义（TOML 文件）
- `backups/` — 带时间戳的完整备份
- `market-cache/` — 市场 skill 列表缓存（JSON，1小时有效期）
- `market-sources.json` — 自定义市场源
- `skill-manager.db` — SQLite 数据库（资源、目标状态、分组成员）

## 许可证

MIT
