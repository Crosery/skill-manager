//! Integration tests for the `runai recommend get` / `runai recommend post-tool`
//! adoption-and-counting pipeline.
//!
//! Pass criteria are written down here, not in prose:
//!   - `recommend_get_*` tests assert (stdout, stderr, usage_count delta,
//!     session_adoption row) after a single command invocation.
//!   - `recommend_post_tool_*` tests assert the same when the PostToolUse
//!     hook stdin matches / does not match a managed SKILL.md path.
//!
//! Every test runs the real `runai` binary against an isolated HOME tempdir;
//! the production `~/.runai/` is never touched.
#![cfg(not(target_os = "windows"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;
use std::io::Write;
use tempfile::TempDir;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn runai() -> Command {
    Command::cargo_bin("runai").expect("runai binary built by cargo test")
}

struct TestEnv {
    home: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("create tmp HOME");
        std::fs::create_dir_all(home.path().join(".runai/skills"))
            .expect("pre-create managed skills dir");
        Self { home }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn managed_skills_dir(&self) -> PathBuf {
        self.home().join(".runai/skills")
    }

    fn db_path(&self) -> PathBuf {
        self.home().join(".runai/runai.db")
    }

    /// Plant a SKILL.md so the binary considers it a managed skill, then
    /// register it in the DB by running `runai scan` (which inserts the
    /// resource row record_usage needs to find).
    fn plant_skill(&self, name: &str, body: &str) {
        let dir = self.managed_skills_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {body}\n---\n\n# {name}\n\n{body}\n"),
        )
        .unwrap();
        let out = self.run(&["scan"]);
        assert!(
            out.status.success(),
            "scan must succeed to register planted skill (stderr={})",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env_remove("RUNE_DATA_DIR")
            .env_remove("SKILL_MANAGER_DATA_DIR")
            .env_remove("CLAUDE_SESSION_ID");
        cmd.output().expect("runai binary spawn")
    }

    fn run_with_session(&self, session_id: &str, args: &[&str]) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env_remove("RUNE_DATA_DIR")
            .env_remove("SKILL_MANAGER_DATA_DIR")
            .env("CLAUDE_SESSION_ID", session_id);
        cmd.output().expect("runai binary spawn")
    }

    fn run_with_stdin(&self, args: &[&str], stdin_payload: &str) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env_remove("RUNE_DATA_DIR")
            .env_remove("SKILL_MANAGER_DATA_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn runai");
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_payload.as_bytes())
            .unwrap();
        child.wait_with_output().expect("wait runai")
    }

    /// Read `usage_count` for a skill from the test DB. Returns 0 when the
    /// resource row is missing — same default the production query produces.
    fn usage_count(&self, name: &str) -> i64 {
        let conn = rusqlite::Connection::open(self.db_path()).expect("open test db");
        conn.query_row(
            "SELECT COALESCE(MAX(usage_count), 0) FROM resources WHERE name = ?1",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    fn has_session_adoption(&self, session_id: &str, skill_name: &str) -> bool {
        let conn = rusqlite::Connection::open(self.db_path()).expect("open test db");
        conn.query_row(
            "SELECT 1 FROM router_session_adoptions WHERE session_id = ?1 AND skill_name = ?2",
            rusqlite::params![session_id, skill_name],
            |_| Ok(()),
        )
        .is_ok()
    }
}

// ─── `runai recommend get` ──────────────────────────────────────────────────

#[test]
fn recommend_get_returns_skill_md_and_increments_usage_count() {
    let env = TestEnv::new();
    env.plant_skill("alpha", "test skill alpha");
    assert_eq!(env.usage_count("alpha"), 0, "precondition: usage starts 0");

    let out = env.run_with_session("sess-A", &["recommend", "get", "alpha"]);
    assert!(
        out.status.success(),
        "get must exit 0 for an existing skill (stderr={})",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // stdout = SKILL.md body verbatim (frontmatter + body).
    assert!(
        stdout.contains("name: alpha"),
        "stdout must contain SKILL.md frontmatter, got:\n{stdout}"
    );
    assert!(
        stdout.contains("test skill alpha"),
        "stdout must contain SKILL.md body, got:\n{stdout}"
    );

    // stderr = bookkeeping receipt.
    assert!(
        stderr.contains("usage_count +1 recorded"),
        "stderr must confirm record, got:\n{stderr}"
    );
    assert!(stderr.contains("alpha"));

    // DB invariants.
    assert_eq!(
        env.usage_count("alpha"),
        1,
        "usage_count must be 1 after one get call"
    );
    assert!(
        env.has_session_adoption("sess-A", "alpha"),
        "session_adoptions row must be written when CLAUDE_SESSION_ID is set"
    );
}

#[test]
fn recommend_get_idempotent_in_session_increments_each_call() {
    let env = TestEnv::new();
    env.plant_skill("beta", "test skill beta");

    let _ = env.run_with_session("sess-B", &["recommend", "get", "beta"]);
    let _ = env.run_with_session("sess-B", &["recommend", "get", "beta"]);
    let _ = env.run_with_session("sess-B", &["recommend", "get", "beta"]);

    // usage_count is a raw counter: 3 calls → 3 increments.
    // (Session dedup happens at the router-recommend layer, not here.)
    assert_eq!(env.usage_count("beta"), 3);
    assert!(env.has_session_adoption("sess-B", "beta"));
}

#[test]
fn recommend_get_without_session_id_still_increments_usage_count() {
    let env = TestEnv::new();
    env.plant_skill("gamma", "test skill gamma");

    let out = env.run(&["recommend", "get", "gamma"]);
    assert!(out.status.success());
    assert_eq!(env.usage_count("gamma"), 1);
    // No session id → no session_adoption row (verified by absence).
    assert!(!env.has_session_adoption("", "gamma"));
}

#[test]
fn recommend_get_missing_skill_exits_nonzero_and_does_not_touch_db() {
    let env = TestEnv::new();
    env.plant_skill("real", "a real skill");

    let out = env.run_with_session("sess-X", &["recommend", "get", "ghost"]);
    assert!(
        !out.status.success(),
        "missing skill must produce non-zero exit"
    );
    assert_eq!(
        env.usage_count("real"),
        0,
        "an unrelated skill's usage_count must not move"
    );
    assert_eq!(env.usage_count("ghost"), 0);
    assert!(!env.has_session_adoption("sess-X", "ghost"));
}

// ─── `runai recommend post-tool` ────────────────────────────────────────────

#[test]
fn recommend_post_tool_records_on_skill_read() {
    let env = TestEnv::new();
    env.plant_skill("delta", "test skill delta");

    let skill_md = env.managed_skills_dir().join("delta").join("SKILL.md");
    let payload = format!(
        r#"{{"tool_name":"Read","tool_input":{{"file_path":"{}"}},"session_id":"sess-D"}}"#,
        skill_md.display()
    );

    let out = env.run_with_stdin(&["recommend", "post-tool"], &payload);
    assert!(out.status.success(), "post-tool always exits 0");

    assert_eq!(env.usage_count("delta"), 1);
    assert!(env.has_session_adoption("sess-D", "delta"));
}

#[test]
fn recommend_post_tool_ignores_non_read_tool() {
    let env = TestEnv::new();
    env.plant_skill("epsilon", "test skill epsilon");

    let payload = r#"{"tool_name":"Bash","tool_input":{"command":"ls"},"session_id":"sess-E"}"#;
    let out = env.run_with_stdin(&["recommend", "post-tool"], payload);
    assert!(out.status.success());

    assert_eq!(env.usage_count("epsilon"), 0);
    assert!(!env.has_session_adoption("sess-E", "epsilon"));
}

#[test]
fn recommend_post_tool_ignores_read_outside_skills_dir() {
    let env = TestEnv::new();
    env.plant_skill("zeta", "test skill zeta");

    let payload = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/random.txt"},"session_id":"sess-Z"}"#;
    let out = env.run_with_stdin(&["recommend", "post-tool"], payload);
    assert!(out.status.success());

    assert_eq!(env.usage_count("zeta"), 0);
    assert!(!env.has_session_adoption("sess-Z", "zeta"));
}

#[test]
fn recommend_post_tool_ignores_read_of_non_skill_md_inside_skill_dir() {
    let env = TestEnv::new();
    env.plant_skill("eta", "test skill eta");
    // A neighbour file inside the skill dir but not SKILL.md.
    let neighbour = env.managed_skills_dir().join("eta").join("notes.md");
    std::fs::write(&neighbour, "neighbouring notes").unwrap();

    let payload = format!(
        r#"{{"tool_name":"Read","tool_input":{{"file_path":"{}"}},"session_id":"sess-N"}}"#,
        neighbour.display()
    );
    let out = env.run_with_stdin(&["recommend", "post-tool"], &payload);
    assert!(out.status.success());

    // Reading a non-SKILL.md file even inside the skill dir must NOT count.
    assert_eq!(env.usage_count("eta"), 0);
    assert!(!env.has_session_adoption("sess-N", "eta"));
}

#[test]
fn recommend_post_tool_with_garbage_stdin_is_noop() {
    let env = TestEnv::new();
    env.plant_skill("theta", "test skill theta");

    let out = env.run_with_stdin(&["recommend", "post-tool"], "not json at all");
    assert!(out.status.success(), "garbage stdin must not error out");
    assert_eq!(env.usage_count("theta"), 0);
}
