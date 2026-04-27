//! Physical end-to-end safety tests for runai.
//!
//! Each test spawns the real `runai` binary in an **isolated HOME** (tempdir)
//! with `RUNE_DATA_DIR` / `SKILL_MANAGER_DATA_DIR` explicitly cleared (or
//! pointed at another tempdir for cross-data-dir cases) and asserts on the
//! resulting filesystem state.
//!
//! These guard the project against the destructive incidents documented in
//! `~/.claude/vault/40-postmortems/` and the safety contract in `AGENTS.md`.
//!
//! Skipped on Windows: symlinks require Developer Mode / Admin and the
//! existing `manager::tests` module is already gated the same way.
#![cfg(not(target_os = "windows"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

// ─── helpers ────────────────────────────────────────────────────────────────

fn runai() -> Command {
    Command::cargo_bin("runai").expect("runai binary built by cargo test")
}

/// Build a TestEnv: tempdir HOME with the four CLI skills dirs pre-created
/// so adoption / register flows have something to look at, plus an isolated
/// `~/.runai/` for managed data. RUNE_DATA_DIR / SKILL_MANAGER_DATA_DIR are
/// cleared by default so the binary uses HOME-rooted defaults.
struct TestEnv {
    home: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("create tmp HOME");
        for cli in ["claude", "codex", "gemini", "opencode"] {
            std::fs::create_dir_all(home.path().join(format!(".{cli}/skills")))
                .expect("pre-create CLI skills dir");
        }
        std::fs::create_dir_all(home.path().join(".runai/skills"))
            .expect("pre-create managed skills dir");
        Self { home }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn default_skills_dir(&self) -> PathBuf {
        self.home().join(".runai/skills")
    }

    fn cli_skills_dir(&self, cli: &str) -> PathBuf {
        self.home().join(format!(".{cli}/skills"))
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env_remove("RUNE_DATA_DIR")
            .env_remove("SKILL_MANAGER_DATA_DIR");
        cmd.output().expect("runai binary spawn")
    }

    fn run_with_rune_data(&self, rune_data: &Path, args: &[&str]) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env("RUNE_DATA_DIR", rune_data)
            .env_remove("SKILL_MANAGER_DATA_DIR");
        cmd.output().expect("runai binary spawn")
    }
}

fn make_skill(parent: &Path, name: &str, body: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(
        &skill_md,
        format!("---\nname: {name}\ndescription: {body}\n---\n\n# {name}\n\n{body}\n"),
    )
    .unwrap();
    dir
}

fn symlink(src: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(src, link).unwrap();
}

fn dump(out: &std::process::Output, label: &str) {
    eprintln!(
        "--- {label} (exit={}) ---\n[stdout]\n{}\n[stderr]\n{}\n--- end ---",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// ─── tests ──────────────────────────────────────────────────────────────────

/// 4-20 / 4-27 root cause regression: `runai scan` with a non-default
/// `RUNE_DATA_DIR` must NOT `std::fs::rename` real skills out of the user's
/// default `~/.runai/skills/`. The Scanner cross-data-dir guard added in
/// `f4a2c0c` (and corrected in `e1c5c4c` to use `default_data_dir_no_env()`)
/// is what makes this safe.
#[test]
fn scan_with_rune_data_dir_does_not_rename_default_skills() {
    let env = TestEnv::new();

    // Real skill in the user's default location.
    let sentinel_dir = make_skill(&env.default_skills_dir(), "sentinel-skill", "sentinel body");
    let sentinel_md = sentinel_dir.join("SKILL.md");
    let original = std::fs::read(&sentinel_md).unwrap();

    // Symlink it into ~/.claude/skills/ so a scan finds something to adopt.
    symlink(
        &sentinel_dir,
        &env.cli_skills_dir("claude").join("sentinel-skill"),
    );

    // Switch to a non-default data dir and run scan. Without the guard, the
    // adopt path would `std::fs::rename` sentinel_dir into other_data/skills/.
    let other = tempfile::tempdir().unwrap();
    let out = env.run_with_rune_data(other.path(), &["scan"]);
    dump(&out, "scan with RUNE_DATA_DIR");

    // The sentinel must still live at its original default location.
    assert!(
        sentinel_md.exists(),
        "REGRESSION (4-20/4-27 root cause): scan moved default skill out of \
         ~/.runai/skills/. sentinel-skill/SKILL.md no longer exists."
    );
    assert_eq!(
        std::fs::read(&sentinel_md).unwrap(),
        original,
        "sentinel-skill content was mutated"
    );

    // Nothing should have been written into the foreign data dir's skills/.
    let foreign_skill = other.path().join("skills/sentinel-skill");
    assert!(
        !foreign_skill.exists(),
        "REGRESSION: scan rename'd skill into the non-default RUNE_DATA_DIR \
         at {}",
        foreign_skill.display()
    );
}

/// AGENTS.md safety contract rule 2: scan with isolated HOME must not touch
/// any path outside the test sandbox. This is the broader "test outside
/// boundaries" check — even if specific paths slip past the cross-data-dir
/// guard, the isolated-HOME assertion catches stray writes.
#[test]
fn scan_does_not_touch_paths_outside_isolated_home() {
    let env = TestEnv::new();

    // Sentinel in /tmp — outside HOME. We snapshot its content and timestamp.
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("dont-touch-me.txt");
    std::fs::write(&outside_file, "untouchable\n").unwrap();
    let mtime_before = std::fs::metadata(&outside_file)
        .unwrap()
        .modified()
        .unwrap();

    // Place a couple of skills inside HOME so scan has work to do.
    make_skill(&env.default_skills_dir(), "alpha", "alpha desc");
    make_skill(&env.default_skills_dir(), "beta", "beta desc");
    let stray = make_skill(env.home(), "stray-source", "stray body"); // inside HOME but outside .runai
    symlink(&stray, &env.cli_skills_dir("claude").join("stray-source"));

    let out = env.run(&["scan"]);
    dump(&out, "scan in isolated HOME");
    assert!(out.status.success(), "scan should succeed");

    // Outside-HOME sentinel must be untouched.
    assert_eq!(
        std::fs::read_to_string(&outside_file).unwrap(),
        "untouchable\n",
        "scan wrote to a file outside the isolated HOME"
    );
    let mtime_after = std::fs::metadata(&outside_file)
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(mtime_before, mtime_after, "scan touched outside-HOME mtime");
}

/// Bug 1: `enable` used to silently no-op when the link path already existed
/// (e.g. a stale symlink to a removed skill). Fix uses
/// `Linker::create_link_force` which clobbers and recreates.
#[test]
fn enable_succeeds_when_stale_symlink_exists_at_link_path() {
    let env = TestEnv::new();

    // Register a real skill and let scan adopt it as local:foo.
    make_skill(&env.default_skills_dir(), "foo", "foo desc");
    let scan_out = env.run(&["scan"]);
    dump(&scan_out, "scan to register foo");
    assert!(scan_out.status.success());

    // Plant a stale symlink at the would-be link path: it points at a
    // non-existent target so `path.exists()` is false but `is_symlink` is true
    // — the *exact* state where the old `if !link.exists() { create }` was
    // already a no-op for opposite reasons. Use a path that DOES exist as a
    // dangling target: a deleted file inside the sandbox.
    let nowhere = env.home().join(".runai/deleted-nowhere");
    let link_path = env.cli_skills_dir("claude").join("foo");
    symlink(&nowhere, &link_path);
    assert!(
        std::fs::symlink_metadata(&link_path).is_ok(),
        "stale symlink should exist before enable"
    );

    let out = env.run(&["enable", "foo", "--target", "claude"]);
    dump(&out, "enable foo (stale symlink present)");
    assert!(
        out.status.success(),
        "REGRESSION (bug 1): enable failed when stale symlink existed at link path"
    );

    // Link must now resolve to the real managed dir.
    let resolved = std::fs::read_link(&link_path).unwrap();
    let expected = env.default_skills_dir().join("foo");
    assert_eq!(
        resolved, expected,
        "REGRESSION (bug 1): symlink not redirected to managed skill dir"
    );
}

/// Bug 2: `status` used `path.exists()` which returns false for dangling
/// symlinks, undercounting enabled skills. Fix uses
/// `Linker::is_symlink` (via `symlink_metadata`).
#[test]
fn status_counts_dangling_symlink_as_enabled() {
    let env = TestEnv::new();

    // Register + enable foo for claude.
    make_skill(&env.default_skills_dir(), "foo", "foo desc");
    assert!(env.run(&["scan"]).status.success());
    let enable = env.run(&["enable", "foo", "--target", "claude"]);
    dump(&enable, "enable foo");
    assert!(enable.status.success());

    // Make the symlink dangling by removing the source dir.
    std::fs::remove_dir_all(env.default_skills_dir().join("foo")).unwrap();
    let link = env.cli_skills_dir("claude").join("foo");
    assert!(
        std::fs::symlink_metadata(&link).is_ok(),
        "dangling symlink should still exist on disk"
    );
    assert!(
        !link.exists(),
        "and `path.exists()` should return false for it (the bug surface)"
    );

    let out = env.run(&["status", "--target", "claude"]);
    dump(&out, "status with dangling symlink");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Skills: 1/"),
        "REGRESSION (bug 2): dangling symlink not counted as enabled. Got:\n{stdout}"
    );
}

/// Trash-first invariant: `runai uninstall` moves to `~/.runai/trash/`,
/// not permanent delete. `trash list` sees it, `trash restore` brings it back.
#[test]
fn uninstall_to_trash_then_restore_round_trip() {
    let env = TestEnv::new();
    make_skill(&env.default_skills_dir(), "round-trip", "round-trip desc");
    assert!(env.run(&["scan"]).status.success());
    assert!(
        env.run(&["enable", "round-trip", "--target", "claude"])
            .status
            .success()
    );

    let original_dir = env.default_skills_dir().join("round-trip");
    assert!(original_dir.join("SKILL.md").exists());

    // Uninstall: skill leaves managed dir, lands in trash.
    let un = env.run(&["uninstall", "round-trip"]);
    dump(&un, "uninstall round-trip");
    assert!(un.status.success());
    assert!(
        !original_dir.exists(),
        "uninstall should remove from managed skills/"
    );
    let trash_root = env.home().join(".runai/trash");
    assert!(
        trash_root.exists() && trash_root.read_dir().unwrap().next().is_some(),
        "uninstalled skill should land under ~/.runai/trash/, but {} is empty",
        trash_root.display()
    );

    // trash list should mention round-trip.
    let list = env.run(&["trash", "list"]);
    dump(&list, "trash list");
    assert!(list.status.success());
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(
        list_out.contains("round-trip"),
        "trash list missing round-trip. Got:\n{list_out}"
    );

    // Restore brings it back.
    let restore = env.run(&["trash", "restore", "round-trip"]);
    dump(&restore, "trash restore round-trip");
    assert!(restore.status.success());
    assert!(
        original_dir.join("SKILL.md").exists(),
        "trash restore did not bring round-trip back to managed skills/"
    );
}

/// 4-27 self-trip: `doctor --fix` is a *write* command, not read. It must
/// only prune dangling symlinks under the four `~/.{cli}/skills/` dirs and
/// NEVER touch:
///   1. valid (non-dangling) symlinks
///   2. files outside those four dirs
///   3. anything outside the isolated HOME
#[test]
fn doctor_fix_only_prunes_dangling_under_cli_skills_dirs() {
    let env = TestEnv::new();

    // Setup A: a valid enabled skill — must survive --fix.
    make_skill(&env.default_skills_dir(), "valid", "valid desc");
    assert!(env.run(&["scan"]).status.success());
    assert!(
        env.run(&["enable", "valid", "--target", "claude"])
            .status
            .success()
    );
    let valid_link = env.cli_skills_dir("claude").join("valid");
    assert!(std::fs::symlink_metadata(&valid_link).is_ok());

    // Setup B: a hand-planted dangling symlink under each CLI skills dir —
    // must be pruned.
    let dangling_paths: Vec<PathBuf> = ["claude", "codex", "gemini", "opencode"]
        .iter()
        .map(|cli| {
            let p = env.cli_skills_dir(cli).join("dangling-x");
            symlink(&env.home().join(".runai/never-existed"), &p);
            p
        })
        .collect();
    for p in &dangling_paths {
        assert!(
            std::fs::symlink_metadata(p).is_ok(),
            "dangling symlink should exist before --fix at {}",
            p.display()
        );
    }

    // Setup C: a file outside the four CLI skills dirs but inside HOME —
    // must NOT be touched.
    let inside_home_other = env.home().join("not-a-skill.txt");
    std::fs::write(&inside_home_other, "leave me alone\n").unwrap();
    let mtime_before = std::fs::metadata(&inside_home_other)
        .unwrap()
        .modified()
        .unwrap();

    // Setup D: a sentinel outside HOME — must NOT be touched.
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("untouchable.txt");
    std::fs::write(&outside_file, "untouchable\n").unwrap();
    let outside_mtime_before = std::fs::metadata(&outside_file)
        .unwrap()
        .modified()
        .unwrap();

    let out = env.run(&["doctor", "--fix"]);
    dump(&out, "doctor --fix");
    assert!(out.status.success());

    // Valid symlink survives.
    assert!(
        std::fs::symlink_metadata(&valid_link).is_ok(),
        "REGRESSION: doctor --fix removed a VALID enabled symlink at {}",
        valid_link.display()
    );

    // Dangling symlinks pruned.
    for p in &dangling_paths {
        assert!(
            std::fs::symlink_metadata(p).is_err(),
            "doctor --fix did not prune dangling symlink at {}",
            p.display()
        );
    }

    // Other inside-HOME file untouched.
    assert_eq!(
        std::fs::read_to_string(&inside_home_other).unwrap(),
        "leave me alone\n"
    );
    assert_eq!(
        std::fs::metadata(&inside_home_other)
            .unwrap()
            .modified()
            .unwrap(),
        mtime_before,
        "doctor --fix touched a non-skills-dir file inside HOME"
    );

    // Outside-HOME sentinel untouched.
    assert_eq!(
        std::fs::read_to_string(&outside_file).unwrap(),
        "untouchable\n"
    );
    assert_eq!(
        std::fs::metadata(&outside_file)
            .unwrap()
            .modified()
            .unwrap(),
        outside_mtime_before,
        "doctor --fix touched a path OUTSIDE the isolated HOME"
    );
}

/// Regression: a directory under `~/.<cli>/skills/` that is not itself a skill
/// (no SKILL.md at the top level, no SKILL.md in immediate children) must NOT
/// produce an "error" line in scan output. This used to surface as
/// `error: <name>: no SKILL.md found in <path>` for codex's bundle dirs like
/// `codex-primary-runtime/{slides,spreadsheets}/SKILL.md`.
#[test]
fn scan_silently_skips_non_skill_directories() {
    let env = TestEnv::new();

    // Bundle structure: container dir, real skills are nested two levels deep.
    let bundle = env.cli_skills_dir("codex").join("some-bundle");
    std::fs::create_dir_all(bundle.join("slides")).unwrap();
    std::fs::write(
        bundle.join("slides/SKILL.md"),
        "---\nname: slides\ndescription: x\n---\n",
    )
    .unwrap();
    std::fs::create_dir_all(bundle.join("spreadsheets")).unwrap();
    std::fs::write(
        bundle.join("spreadsheets/SKILL.md"),
        "---\nname: spreadsheets\ndescription: x\n---\n",
    )
    .unwrap();

    // An empty subdirectory — no SKILL.md anywhere underneath.
    std::fs::create_dir_all(env.cli_skills_dir("claude").join("empty-dir")).unwrap();

    let out = env.run(&["scan"]);
    dump(&out, "scan with non-skill dirs present");
    assert!(out.status.success(), "scan should succeed");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("no SKILL.md found"),
        "REGRESSION: scanner errored on non-skill dir. stderr:\n{stderr}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("0 errors"),
        "REGRESSION: scan reported errors when only non-skill dirs were present.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
