# Filesystem as Source of Truth — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the filesystem the single source of truth for all skill/MCP runtime state, so MCP management works in TUI, external changes are detected instantly, and migration is safe for existing users.

**Architecture:** DB keeps only skill metadata and group membership. MCP data comes entirely from CLI config files. Skill enabled state comes from symlink existence. Old DB tables are preserved but ignored for rollback safety.

**Tech Stack:** Rust, rusqlite, serde_json, ratatui, rmcp

**Spec:** `docs/superpowers/specs/2026-03-25-filesystem-source-of-truth-design.md`

---

## File Map

| File | Role | Change |
|------|------|--------|
| `src/core/db.rs` | DB schema + queries | Add `schema_version`, migration, `get_group_member_ids()`. Remove dead methods. |
| `src/core/manager.rs` | Core business logic | Rewrite `list_resources`, `enable/disable_resource`, `find_resource_id`, `get_group_members`, `resource_count`, `is_first_launch`. |
| `src/core/scanner.rs` | Skill discovery | Remove MCP registration and `set_target_enabled` calls. |
| `src/mcp/tools.rs` | MCP server tools | Update call sites to use new manager APIs. Fix tool description. |
| `src/tui/app.rs` | TUI application | Update call sites, extend `poll_config_changes`. |
| `src/core/market.rs` | Market fetch/install | Add `.claude-plugin` detection. |
| `CLAUDE.md` | Project docs | Fix "downloads only SKILL.md". |

---

## Task 1: DB Migration — schema_version + group_members FK removal

**Files:**
- Modify: `src/core/db.rs`

- [ ] **Step 1: Write failing test — migration creates schema_version table**

In `src/core/db.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn migration_creates_schema_version() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join("test.db")).unwrap();
    let version: i64 = db.conn.query_row(
        "SELECT version FROM schema_version", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(version, 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test migration_creates_schema_version -- --nocapture`
Expected: FAIL — `schema_version` table doesn't exist

- [ ] **Step 3: Implement schema_version + migration in `init_schema`**

In `src/core/db.rs`, add to `init_schema()` after the existing `CREATE TABLE` statements:

```rust
// Schema versioning
self.conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);"
)?;

let version: i64 = self.conn.query_row(
    "SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0)
)?;

if version < 2 {
    // Recreate group_members without FK constraint
    self.conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS group_members_new (
            group_id TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            PRIMARY KEY (group_id, resource_id)
        );
        INSERT OR IGNORE INTO group_members_new SELECT group_id, resource_id FROM group_members;
        DROP TABLE IF EXISTS group_members;
        ALTER TABLE group_members_new RENAME TO group_members;

        DELETE FROM schema_version;
        INSERT INTO schema_version VALUES (2);"
    )?;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test migration_creates_schema_version -- --nocapture`
Expected: PASS

- [ ] **Step 5: Write test — migration preserves existing group_members**

```rust
#[test]
fn migration_preserves_group_members() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");

    // Create old schema with FK (disable FK enforcement to insert mcp: row without resources entry)
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             CREATE TABLE resources (id TEXT PRIMARY KEY, name TEXT, kind TEXT, description TEXT, directory TEXT, source_type TEXT, source_meta TEXT, installed_at INTEGER);
             CREATE TABLE group_members (group_id TEXT, resource_id TEXT, PRIMARY KEY(group_id, resource_id), FOREIGN KEY(resource_id) REFERENCES resources(id));
             INSERT INTO resources VALUES ('local:foo','foo','skill','','','/tmp','local','{}',0);
             INSERT INTO group_members VALUES ('grp1','local:foo');
             INSERT INTO group_members VALUES ('grp1','mcp:bar');"
        ).unwrap();
    }

    // Open with migration
    let db = Database::open(&db_path).unwrap();
    let ids = db.get_group_member_ids("grp1").unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"local:foo".to_string()));
    assert!(ids.contains(&"mcp:bar".to_string()));
}
```

- [ ] **Step 6: Implement `get_group_member_ids` — ID-only query (no JOIN)**

In `src/core/db.rs`, add:

```rust
/// Get group member IDs without joining resources table.
/// Returns raw resource_id strings like "local:foo" or "mcp:bar".
pub fn get_group_member_ids(&self, group_id: &str) -> Result<Vec<String>> {
    let mut stmt = self.conn.prepare(
        "SELECT resource_id FROM group_members WHERE group_id = ?1"
    )?;
    let rows = stmt.query_map(params![group_id], |row| row.get(0))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row?);
    }
    Ok(ids)
}
```

- [ ] **Step 7: Run both tests**

Run: `cargo test migration_ -- --nocapture`
Expected: PASS (2 tests)

- [ ] **Step 8: Add `schema_version` getter**

```rust
pub fn schema_version(&self) -> i64 {
    self.conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0)
    ).unwrap_or(0)
}
```

- [ ] **Step 9: Run all tests to verify no regressions**

Run: `cargo test`
Expected: All 52+ tests pass

- [ ] **Step 10: Commit**

```bash
git add src/core/db.rs
git commit -m "feat: add schema_version migration, get_group_member_ids"
```

---

## Task 2: Manager — MCP list from config files (no DB)

**Files:**
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Write failing test — list_resources(Mcp) reads config files**

Add to `src/core/manager.rs` tests:

```rust
#[test]
fn list_resources_mcp_reads_from_config_files() {
    let tmp = tempfile::tempdir().unwrap();

    // Write a fake .claude.json with MCPs
    let config = serde_json::json!({
        "mcpServers": {
            "server-a": { "command": "a", "args": [] },
            "server-b": { "command": "b", "args": [], "disabled": true }
        }
    });
    std::fs::write(
        tmp.path().join(".claude.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    ).unwrap();

    with_home(tmp.path(), || {
        let mgr = SkillManager::with_base(tmp.path().join("sm-data")).unwrap();
        let mcps = mgr.list_resources(
            Some(crate::core::resource::ResourceKind::Mcp), None
        ).unwrap();

        assert_eq!(mcps.len(), 2);
        let a = mcps.iter().find(|r| r.name == "server-a").unwrap();
        assert_eq!(a.id, "mcp:server-a");
        assert!(a.is_enabled_for(CliTarget::Claude));

        let b = mcps.iter().find(|r| r.name == "server-b").unwrap();
        assert!(!b.is_enabled_for(CliTarget::Claude));
    });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test list_resources_mcp_reads_from_config_files -- --nocapture`
Expected: FAIL — returns empty because no MCP rows in DB

- [ ] **Step 3: Implement — build MCP Resources from config files**

Replace the `list_resources` method in `src/core/manager.rs`:

```rust
pub fn list_resources(
    &self,
    kind: Option<ResourceKind>,
    enabled_for: Option<CliTarget>,
) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();

    // Skills: from DB, enabled state from symlinks
    if kind.is_none() || kind == Some(ResourceKind::Skill) {
        let mut skills = self.db.list_resources(Some(ResourceKind::Skill), None)?;
        for skill in &mut skills {
            skill.enabled = self.check_skill_symlinks(&skill.name);
        }
        if let Some(target) = enabled_for {
            skills.retain(|s| s.is_enabled_for(target));
        }
        resources.extend(skills);
    }

    // MCPs: entirely from config files
    if kind.is_none() || kind == Some(ResourceKind::Mcp) {
        let mcp_status = Self::read_mcp_status_from_configs();
        for (name, targets) in &mcp_status {
            if let Some(target) = enabled_for {
                if !targets.get(&target).copied().unwrap_or(false) {
                    continue;
                }
            }
            resources.push(Resource {
                id: format!("mcp:{name}"),
                name: name.clone(),
                kind: ResourceKind::Mcp,
                description: String::new(),
                directory: PathBuf::new(),
                source: Source::Local { path: PathBuf::new() },
                installed_at: 0,
                enabled: targets.clone(),
            });
        }
    }

    Ok(resources)
}

/// Check which CLI targets have a symlink for this skill name.
fn check_skill_symlinks(&self, name: &str) -> HashMap<CliTarget, bool> {
    let mut map = HashMap::new();
    for target in CliTarget::ALL {
        let link = target.skills_dir().join(name);
        let enabled = Linker::is_our_symlink(&link, self.paths.data_dir());
        map.insert(*target, enabled);
    }
    map
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test list_resources_mcp_reads_from_config_files -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: All pass (some existing tests may need adjustment — fix any failures)

- [ ] **Step 6: Commit**

```bash
git add src/core/manager.rs
git commit -m "feat: list_resources reads MCPs from config files, skills from symlinks"
```

---

## Task 3: Manager — enable/disable handles mcp: prefix without DB

**Files:**
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Write failing test — enable MCP by mcp: ID**

```rust
#[test]
fn enable_disable_mcp_by_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let config = serde_json::json!({
        "mcpServers": {
            "test-mcp": { "command": "test", "args": [], "disabled": true }
        }
    });
    let config_path = tmp.path().join(".claude.json");
    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    with_home(tmp.path(), || {
        let mgr = SkillManager::with_base(tmp.path().join("sm-data")).unwrap();

        // Enable — no DB lookup needed
        mgr.enable_resource("mcp:test-mcp", CliTarget::Claude, None).unwrap();

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["test-mcp"].get("disabled").is_none());

        // Disable
        mgr.disable_resource("mcp:test-mcp", CliTarget::Claude, None).unwrap();

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(content["mcpServers"]["test-mcp"]["disabled"], true);
    });
}
```

- [ ] **Step 2: Run test — expected FAIL (DB lookup fails for mcp: ID)**

Run: `cargo test enable_disable_mcp_by_prefix -- --nocapture`

- [ ] **Step 3: Rewrite enable_resource / disable_resource**

```rust
pub fn enable_resource(
    &self,
    resource_id: &str,
    target: CliTarget,
    cli_dir_override: Option<&Path>,
) -> Result<()> {
    if let Some(mcp_name) = resource_id.strip_prefix("mcp:") {
        Self::set_mcp_disabled(mcp_name, target, false)
    } else {
        let resource = self.db.get_resource(resource_id)?
            .ok_or_else(|| anyhow::anyhow!("resource not found: {resource_id}"))?;
        let cli_dir = cli_dir_override
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| target.skills_dir());
        std::fs::create_dir_all(&cli_dir)?;
        let link_path = cli_dir.join(&resource.name);
        if !link_path.exists() {
            Linker::create_link(&resource.directory, &link_path)?;
        }
        Ok(())
    }
}

pub fn disable_resource(
    &self,
    resource_id: &str,
    target: CliTarget,
    cli_dir_override: Option<&Path>,
) -> Result<()> {
    if let Some(mcp_name) = resource_id.strip_prefix("mcp:") {
        Self::set_mcp_disabled(mcp_name, target, true)
    } else {
        let resource = self.db.get_resource(resource_id)?
            .ok_or_else(|| anyhow::anyhow!("resource not found: {resource_id}"))?;
        let cli_dir = cli_dir_override
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| target.skills_dir());
        let link_path = cli_dir.join(&resource.name);
        if Linker::is_our_symlink(&link_path, self.paths.data_dir()) {
            Linker::remove_link(&link_path)?;
        }
        Ok(())
    }
}
```

Note: `db.set_target_enabled()` calls are removed — no DB write for enabled state.

- [ ] **Step 4: Run test — expected PASS**

Run: `cargo test enable_disable_mcp_by_prefix -- --nocapture`

- [ ] **Step 5: Run all tests**

Run: `cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/core/manager.rs
git commit -m "feat: enable/disable handles mcp: prefix without DB lookup"
```

---

## Task 4: Manager — find_resource_id discovers MCPs from config

**Files:**
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn find_resource_id_discovers_mcp_from_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = serde_json::json!({
        "mcpServers": {
            "my-tool": { "command": "tool", "args": [] }
        }
    });
    std::fs::write(
        tmp.path().join(".claude.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    ).unwrap();

    with_home(tmp.path(), || {
        let mgr = SkillManager::with_base(tmp.path().join("sm-data")).unwrap();
        let id = mgr.find_resource_id("my-tool");
        assert_eq!(id, Some("mcp:my-tool".to_string()));
    });
}
```

- [ ] **Step 2: Run test — FAIL**

Run: `cargo test find_resource_id_discovers_mcp -- --nocapture`

- [ ] **Step 3: Update `find_resource_id` to check config files**

```rust
pub fn find_resource_id(&self, name: &str) -> Option<String> {
    // Check DB first (skill prefixes)
    for prefix in &["local:", "adopted:", "github:"] {
        let id = format!("{prefix}{name}");
        if let Ok(Some(_)) = self.db.get_resource(&id) {
            return Some(id);
        }
    }
    if let Ok(all) = self.db.list_resources(None, None) {
        for r in all {
            if r.name == name {
                return Some(r.id);
            }
        }
    }
    // Check MCP config files
    let mcp_status = Self::read_mcp_status_from_configs();
    if mcp_status.contains_key(name) {
        return Some(format!("mcp:{name}"));
    }
    None
}
```

- [ ] **Step 4: Run test — PASS**

Run: `cargo test find_resource_id_discovers_mcp -- --nocapture`

- [ ] **Step 5: Commit**

```bash
git add src/core/manager.rs
git commit -m "feat: find_resource_id discovers MCPs from config files"
```

---

## Task 5: Manager — get_group_members resolves mcp: members dynamically

**Files:**
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn get_group_members_resolves_mcp_dynamically() {
    let tmp = tempfile::tempdir().unwrap();
    let config = serde_json::json!({
        "mcpServers": {
            "my-mcp": { "command": "mcp-cmd", "args": [] }
        }
    });
    std::fs::write(
        tmp.path().join(".claude.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    ).unwrap();

    with_home(tmp.path(), || {
        let mgr = SkillManager::with_base(tmp.path().join("sm-data")).unwrap();

        // Add mcp member directly to group_members (no FK now)
        mgr.db().add_group_member("test-group", "mcp:my-mcp").unwrap();

        let members = mgr.get_group_members("test-group").unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "my-mcp");
        assert_eq!(members[0].kind, ResourceKind::Mcp);
        assert!(members[0].is_enabled_for(CliTarget::Claude));
    });
}
```

- [ ] **Step 2: Run test — FAIL**

Run: `cargo test get_group_members_resolves_mcp -- --nocapture`

- [ ] **Step 3: Implement `get_group_members` on SkillManager**

```rust
/// Get group members, resolving mcp: IDs from config files dynamically.
pub fn get_group_members(&self, group_id: &str) -> Result<Vec<Resource>> {
    let ids = self.db.get_group_member_ids(group_id)?;
    let mcp_status = Self::read_mcp_status_from_configs();
    let mut members = Vec::new();

    for id in &ids {
        if let Some(mcp_name) = id.strip_prefix("mcp:") {
            // Build MCP Resource from config
            let enabled = mcp_status.get(mcp_name).cloned().unwrap_or_default();
            members.push(Resource {
                id: id.clone(),
                name: mcp_name.to_string(),
                kind: ResourceKind::Mcp,
                description: String::new(),
                directory: PathBuf::new(),
                source: Source::Local { path: PathBuf::new() },
                installed_at: 0,
                enabled,
            });
        } else if let Ok(Some(mut res)) = self.db.get_resource(id) {
            res.enabled = self.check_skill_symlinks(&res.name);
            members.push(res);
        }
        // Skip IDs that don't resolve (stale group member)
    }

    Ok(members)
}
```

- [ ] **Step 4: Run test — PASS**

Run: `cargo test get_group_members_resolves_mcp -- --nocapture`

- [ ] **Step 5: Update enable_group / disable_group to use new get_group_members**

In `src/core/manager.rs`, replace `enable_group` and `disable_group`:

```rust
pub fn enable_group(
    &self,
    group_id: &str,
    target: CliTarget,
    cli_dir_override: Option<&Path>,
) -> Result<()> {
    let members = self.get_group_members(group_id)?;
    for member in &members {
        self.enable_resource(&member.id, target, cli_dir_override)?;
    }
    Ok(())
}

pub fn disable_group(
    &self,
    group_id: &str,
    target: CliTarget,
    cli_dir_override: Option<&Path>,
) -> Result<()> {
    let members = self.get_group_members(group_id)?;
    for member in &members {
        self.disable_resource(&member.id, target, cli_dir_override)?;
    }
    Ok(())
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`

- [ ] **Step 7: Commit**

```bash
git add src/core/manager.rs
git commit -m "feat: get_group_members resolves mcp: IDs, enable/disable_group updated"
```

---

## Task 6: Manager — resource_count + is_first_launch + status

**Files:**
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Write failing test — is_first_launch with MCPs only**

```rust
#[test]
fn is_first_launch_false_when_mcps_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let config = serde_json::json!({
        "mcpServers": { "x": { "command": "x" } }
    });
    std::fs::write(
        tmp.path().join(".claude.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    ).unwrap();

    with_home(tmp.path(), || {
        let mgr = SkillManager::with_base(tmp.path().join("sm-data")).unwrap();
        assert!(!mgr.is_first_launch());
    });
}
```

- [ ] **Step 2: Run test — FAIL (DB has 0 resources, returns true)**

Run: `cargo test is_first_launch_false_when_mcps -- --nocapture`

- [ ] **Step 3: Update is_first_launch, resource_count, status**

```rust
pub fn is_first_launch(&self) -> bool {
    // After migration, schema_version is always >= 2 (set in init_schema).
    // Check skills in DB + MCPs in config files.
    let (skills, mcps) = self.resource_count();
    skills + mcps == 0
}

/// Count total skills (from DB) + total MCPs (from config files).
pub fn resource_count(&self) -> (usize, usize) {
    let skills = self.db.skill_count().unwrap_or(0);
    let mcps = Self::read_mcp_status_from_configs().len();
    (skills, mcps)
}

pub fn status(&self, target: CliTarget) -> Result<(usize, usize)> {
    // Count enabled skills by checking symlinks
    let mut skill_enabled = 0;
    if let Ok(skills) = self.db.list_resources(Some(ResourceKind::Skill), None) {
        for skill in &skills {
            let link = target.skills_dir().join(&skill.name);
            if Linker::is_our_symlink(&link, self.paths.data_dir()) {
                skill_enabled += 1;
            }
        }
    }
    // Count enabled MCPs from config
    let mcp_status = Self::read_mcp_status_from_configs();
    let mcp_enabled = mcp_status.values()
        .filter(|targets| targets.get(&target).copied().unwrap_or(false))
        .count();
    Ok((skill_enabled, mcp_enabled))
}
```

Add `skill_count` to `db.rs`:

```rust
pub fn skill_count(&self) -> Result<usize> {
    let count: i64 = self.conn.query_row(
        "SELECT COUNT(*) FROM resources WHERE kind = 'skill'", [], |r| r.get(0)
    )?;
    Ok(count as usize)
}
```

- [ ] **Step 4: Run test — PASS**

Run: `cargo test is_first_launch_false_when_mcps -- --nocapture`

- [ ] **Step 5: Run all tests**

Run: `cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/core/db.rs src/core/manager.rs
git commit -m "feat: resource_count and is_first_launch use config files for MCPs"
```

---

## Task 7: Scanner — remove MCP registration and set_target_enabled

**Files:**
- Modify: `src/core/scanner.rs`

- [ ] **Step 1: Remove `set_target_enabled` calls from scanner**

In `src/core/scanner.rs`, remove these calls:
- Line 110: `let _ = db.set_target_enabled(&resource.id, *target, true);`
- Line 154: `let _ = db.set_target_enabled(&id, target, true);`

Replace line 110 (inside `scan_managed_dir`) with nothing — just remove the call.
Replace line 154 (inside `scan_cli_dir` `OurSymlink` branch) with nothing — the symlink existence IS the enabled state now.

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 3: Commit**

```bash
git add src/core/scanner.rs
git commit -m "refactor: scanner no longer writes enabled state to DB"
```

---

## Task 8: Update all call sites — tools.rs + tui/app.rs

**Files:**
- Modify: `src/mcp/tools.rs`
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Update `sm_list` group filter branch (tools.rs:133-134)**

Replace:
```rust
mgr.db().get_group_members(&group_id).unwrap_or_default()
```
With:
```rust
mgr.get_group_members(&group_id).unwrap_or_default()
```

- [ ] **Step 2: Update `sm_groups` member count (tools.rs:165)**

Replace:
```rust
let members = mgr.db().get_group_members(id).unwrap_or_default();
```
With:
```rust
let members = mgr.get_group_members(id).unwrap_or_default();
```

- [ ] **Step 3: Update `sm_status` resource count (tools.rs:182)**

Replace:
```rust
let (ts, tm) = mgr.db().resource_count().unwrap_or((0, 0));
```
With:
```rust
let (ts, tm) = mgr.resource_count();
```

- [ ] **Step 4: Fix tool description (tools.rs:381)**

Replace:
```rust
#[tool(description = "Install a single skill from the market (downloads only SKILL.md)")]
```
With:
```rust
#[tool(description = "Install a single skill from the market (downloads full skill directory)")]
```

- [ ] **Step 5: Update TUI reload (app.rs:201)**

Replace:
```rust
let members = self.mgr.db().get_group_members(&id).unwrap_or_default();
```
With:
```rust
let members = self.mgr.get_group_members(&id).unwrap_or_default();
```

- [ ] **Step 6: Update TUI reload_group_detail (app.rs:733)**

Replace:
```rust
self.detail_members = self.mgr.db()
    .get_group_members(&self.detail_group_id)
    .unwrap_or_default();
```
With:
```rust
self.detail_members = self.mgr
    .get_group_members(&self.detail_group_id)
    .unwrap_or_default();
```

- [ ] **Step 7: Update TUI status line (app.rs:207)**

Replace:
```rust
let (ts, tm) = self.mgr.db().resource_count().unwrap_or((0, 0));
```
With:
```rust
let (ts, tm) = self.mgr.resource_count();
```

- [ ] **Step 8: Extend poll_config_changes to watch skills dirs + mcp-configs**

In `src/tui/app.rs`, in `poll_config_changes`, add after the existing 4 config paths:

```rust
// Also watch skills directories
for target in CliTarget::ALL {
    let skills_dir = target.skills_dir();
    if skills_dir.exists() {
        let key = skills_dir.to_string_lossy().to_string();
        let mtime = std::fs::metadata(&skills_dir)
            .and_then(|m| m.modified())
            .ok();
        if let Some(mt) = mtime {
            let prev = self.config_mtimes.get(&key);
            if prev != Some(&mt) {
                self.config_mtimes.insert(key, mt);
                changed = true;
            }
        }
    }
}

// Watch mcp-configs directory
let mcp_configs = home.join(".claude").join("mcp-configs");
if mcp_configs.exists() {
    let key = mcp_configs.to_string_lossy().to_string();
    let mtime = std::fs::metadata(&mcp_configs)
        .and_then(|m| m.modified())
        .ok();
    if let Some(mt) = mtime {
        let prev = self.config_mtimes.get(&key);
        if prev != Some(&mt) {
            self.config_mtimes.insert(key, mt);
            changed = true;
        }
    }
}
```

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: All pass

- [ ] **Step 10: Build and verify**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 11: Commit**

```bash
git add src/mcp/tools.rs src/tui/app.rs
git commit -m "refactor: update all call sites to use filesystem-based APIs"
```

---

## Task 9: Market — plugin format detection + better error messages

**Files:**
- Modify: `src/core/market.rs`

- [ ] **Step 1: Make `GitTree` and `GitTreeNode` `pub(crate)` for testability**

In `src/core/market.rs`, change:
- `struct GitTree` → `pub(crate) struct GitTree`
- `struct GitTreeNode` → `pub(crate) struct GitTreeNode`
- Add `pub(crate)` to their fields (`tree`, `path`)

- [ ] **Step 2: Write failing test — detect plugin format**

Add at end of `src/core/market.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_detects_claude_plugin_format() {
        // Simulate a git tree with .claude-plugin but no SKILL.md
        let tree = GitTree {
            tree: vec![
                GitTreeNode { path: ".claude-plugin/plugin.json".into() },
                GitTreeNode { path: "README.md".into() },
                GitTreeNode { path: "skills/brainstorming/SKILL.md".into() },
            ],
        };

        let source = SourceEntry {
            owner: "test".into(),
            repo: "test-plugin".into(),
            branch: "main".into(),
            skill_prefix: String::new(),
            label: "Test".into(),
            description: "test".into(),
            builtin: false,
            enabled: true,
        };

        let result = Market::extract_skills(&tree, &source);
        assert!(result.plugin_detected);
        assert_eq!(result.skills.len(), 1); // still finds skills/ entries
    }
}
```

- [ ] **Step 3: Run test — FAIL (method `extract_skills` doesn't exist)**

Run: `cargo test fetch_detects_claude_plugin -- --nocapture`

- [ ] **Step 4: Refactor Market::fetch to separate extraction logic**

Extract skill detection into a testable function:

```rust
pub struct ExtractResult {
    pub skills: Vec<MarketSkill>,
    pub plugin_detected: bool,
}

impl Market {
    /// Extract skills from a git tree. Also detects .claude-plugin format.
    pub fn extract_skills(tree: &GitTree, source: &SourceEntry) -> ExtractResult {
        let label = &source.label;
        let repo_id = source.repo_id();
        let mut skills = Vec::new();
        let mut plugin_detected = false;

        for node in &tree.tree {
            if node.path.contains(".claude-plugin") {
                plugin_detected = true;
                continue;
            }

            if !node.path.ends_with("/SKILL.md") && node.path != "SKILL.md" {
                continue;
            }

            if node.path == "SKILL.md" {
                skills.push(MarketSkill {
                    name: source.repo.clone(),
                    repo_path: String::new(),
                    source_label: label.clone(),
                    source_repo: repo_id.clone(),
                    branch: source.branch.clone(),
                    installed: false,
                });
                continue;
            }

            let dir = node.path.trim_end_matches("/SKILL.md");
            let name = if !source.skill_prefix.is_empty() {
                match dir.strip_prefix(source.skill_prefix.as_str()) {
                    Some(s) => s.rsplit('/').next().unwrap_or(s).to_string(),
                    None => continue,
                }
            } else {
                dir.rsplit('/').next().unwrap_or(dir).to_string()
            };

            if name.is_empty() { continue; }

            skills.push(MarketSkill {
                name,
                repo_path: dir.to_string(),
                source_label: label.clone(),
                source_repo: repo_id.clone(),
                branch: source.branch.clone(),
                installed: false,
            });
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills.dedup_by(|a, b| a.name == b.name);
        ExtractResult { skills, plugin_detected }
    }
}
```

Update `Market::fetch` to use `extract_skills` internally.

- [ ] **Step 5: Run test — PASS**

Run: `cargo test fetch_detects_claude_plugin -- --nocapture`

- [ ] **Step 6: Update sm_market in tools.rs to show plugin hint**

In `sm_market` (tools.rs), when `all_skills` is empty after checking cache:
```rust
if all_skills.is_empty() {
    if let Some(ref search) = p.search {
        return Json(TextResult {
            result: format!("No skills matching '{}'. Use sm_sources to check available sources.", search)
        });
    }
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`

- [ ] **Step 8: Commit**

```bash
git add src/core/market.rs src/mcp/tools.rs
git commit -m "feat: market detects .claude-plugin format, better error messages"
```

---

## Task 10: DB + Manager cleanup — remove dead methods + register_mcps

**Files:**
- Modify: `src/core/db.rs`
- Modify: `src/core/manager.rs`

- [ ] **Step 1: Remove unused DB methods**

Remove these methods from `Database`:
- `set_target_enabled` — no longer called
- `get_targets_for_resource` — no longer called
- `enabled_count` — replaced by `manager.status()`
- `enabled_skill_count` — replaced by `manager.status()`

In `insert_resource`, remove the `set_target_enabled` loop (lines 70-73):
```rust
// DELETE this block from insert_resource:
for (target, enabled) in &res.enabled {
    self.set_target_enabled(&res.id, *target, *enabled)?;
}
```

Keep `resource_count` but update to only count skills:
```rust
pub fn resource_count(&self) -> Result<(usize, usize)> {
    let skills: i64 = self.conn.query_row(
        "SELECT COUNT(*) FROM resources WHERE kind = 'skill'", [], |r| r.get(0)
    )?;
    // MCP count comes from config files, not DB — return 0 here
    Ok((skills as usize, 0))
}
```

Also update `list_resources` to skip the `enabled_for` parameter for skills (no longer reads from `resource_targets`):
```rust
pub fn list_resources(
    &self,
    kind: Option<ResourceKind>,
    _enabled_for: Option<CliTarget>,  // ignored — enabled state from filesystem
) -> Result<Vec<Resource>> {
    let mut resources = match kind {
        Some(k) => {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, kind, description, directory, source_type, source_meta, installed_at
                 FROM resources WHERE kind = ?1 ORDER BY name"
            )?;
            self.collect_resources(&mut stmt, params![k.as_str()])?
        }
        None => {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, kind, description, directory, source_type, source_meta, installed_at
                 FROM resources ORDER BY name"
            )?;
            self.collect_resources(&mut stmt, params![])?
        }
    };
    // enabled field left empty — caller fills from filesystem
    for res in &mut resources {
        res.enabled = HashMap::new();
    }
    Ok(resources)
}
```

- [ ] **Step 2: Remove `register_mcps` from manager.rs**

Delete the `register_mcps` method from `SkillManager` (manager.rs:347-377). MCP data is no longer written to DB.

Also remove `register_mcps` call from `tui/app.rs` in `do_first_launch_scan` (around line 693):
```rust
// DELETE these lines:
let mcps_registered = self.mgr.register_mcps(&mcp_entries);
self.scan_log.push(format!("  Registered {} new MCPs", mcps_registered));
```
Replace with:
```rust
self.scan_log.push(format!("  Found {} MCP configs", mcp_entries.len()));
```

- [ ] **Step 3: Fix any compilation errors**

Run: `cargo build`

- [ ] **Step 4: Run all tests**

Run: `cargo test`

- [ ] **Step 5: Commit**

```bash
git add src/core/db.rs src/core/manager.rs src/tui/app.rs
git commit -m "refactor: remove dead DB methods, register_mcps, simplify list_resources"
```

---

## Task 11: CLAUDE.md + final verification

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Fix CLAUDE.md description**

Replace:
```
- **Market install downloads only SKILL.md** — not the entire repo
```
With:
```
- **Market install downloads full skill directory** — not just SKILL.md
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Compiles cleanly

- [ ] **Step 4: Manual smoke test — TUI**

Run: `./target/release/skill-manager`
Expected: TUI opens, MCPs tab shows all MCPs from config files, toggle works.

- [ ] **Step 5: Manual smoke test — MCP tools**

Run (in another Claude Code session):
- `sm_list` — shows skills and MCPs
- `sm_status` — shows correct counts
- `sm_enable(name="some-mcp", target="claude")` — enables MCP
- `sm_disable(name="some-mcp", target="claude")` — disables MCP

- [ ] **Step 6: Commit all remaining changes**

```bash
git add CLAUDE.md
git commit -m "docs: fix skill download description in CLAUDE.md"
```

- [ ] **Step 7: Final commit — tag release**

```bash
git tag v0.2.0
```
