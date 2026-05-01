//! Cross-CLI-target symmetry: enable/disable/uninstall must put symlinks in
//! the right per-target dir for every supported CLI (claude / codex / gemini /
//! opencode), and clean them up symmetrically.
//!
//! Each test runs in an isolated HOME tempdir and spawns the real `runai`
//! binary. Skipped on Windows for the same reason as `safety_e2e.rs`.
#![cfg(not(target_os = "windows"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

fn runai() -> Command {
    Command::cargo_bin("runai").expect("runai binary built by cargo test")
}

struct TestEnv {
    home: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        for cli in ["claude", "codex", "gemini", "opencode"] {
            std::fs::create_dir_all(home.path().join(format!(".{cli}/skills"))).unwrap();
        }
        std::fs::create_dir_all(home.path().join(".runai/skills")).unwrap();
        Self { home }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn cli_skills_dir(&self, cli: &str) -> PathBuf {
        // Mirrors src/core/cli_target.rs::skills_dir() on unix.
        self.home().join(format!(".{cli}/skills"))
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = runai();
        cmd.args(args)
            .env("HOME", self.home())
            .env_remove("RUNE_DATA_DIR")
            .env_remove("SKILL_MANAGER_DATA_DIR");
        cmd.output().unwrap()
    }
}

fn make_skill(parent: &Path, name: &str) {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test desc\n---\n\n# {name}\n"),
    )
    .unwrap();
}

fn dump(out: &std::process::Output, label: &str) {
    eprintln!(
        "--- {label} (exit={}) ---\n{}\n{}\n--- end ---",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Run the full enable → assert → disable → assert → uninstall → assert cycle
/// for one CLI target. Asserts symlinks land in the right per-target dir and
/// disappear symmetrically.
fn round_trip(target: &str, skill_name: &str) {
    let env = TestEnv::new();
    let skill_dir = env.home().join(".runai/skills");
    make_skill(&skill_dir, skill_name);
    assert!(
        env.run(&["scan"]).status.success(),
        "scan failed for {target}"
    );

    let link_path = env.cli_skills_dir(target).join(skill_name);

    // --- enable ---
    let en = env.run(&["enable", skill_name, "--target", target]);
    dump(&en, &format!("enable {skill_name} on {target}"));
    assert!(en.status.success(), "enable failed for {target}");
    assert!(
        std::fs::symlink_metadata(&link_path).is_ok(),
        "enable on {target} did not create symlink at expected path: {}",
        link_path.display()
    );
    let resolved = std::fs::read_link(&link_path).unwrap();
    assert_eq!(
        resolved,
        skill_dir.join(skill_name),
        "symlink on {target} points to wrong target: {}",
        resolved.display()
    );

    // No collateral: the *other* three CLI dirs must NOT have a same-named link.
    for other in ["claude", "codex", "gemini", "opencode"] {
        if other == target {
            continue;
        }
        let collateral = env.cli_skills_dir(other).join(skill_name);
        assert!(
            std::fs::symlink_metadata(&collateral).is_err(),
            "enabling on {target} accidentally created symlink under {other}: {}",
            collateral.display()
        );
    }

    // --- disable ---
    let dis = env.run(&["disable", skill_name, "--target", target]);
    dump(&dis, &format!("disable {skill_name} on {target}"));
    assert!(dis.status.success(), "disable failed for {target}");
    assert!(
        std::fs::symlink_metadata(&link_path).is_err(),
        "disable on {target} left symlink at {}",
        link_path.display()
    );

    // --- re-enable then uninstall (trash-first) ---
    assert!(
        env.run(&["enable", skill_name, "--target", target])
            .status
            .success()
    );
    let un = env.run(&["uninstall", skill_name]);
    dump(
        &un,
        &format!("uninstall {skill_name} (was enabled on {target})"),
    );
    assert!(un.status.success(), "uninstall failed for {target}");
    assert!(
        std::fs::symlink_metadata(&link_path).is_err(),
        "uninstall on {target} left symlink at {}",
        link_path.display()
    );
    assert!(
        !skill_dir.join(skill_name).exists(),
        "uninstall on {target} did not move skill out of managed skills/"
    );
    let trash = env.home().join(".runai/trash");
    assert!(
        trash.exists() && trash.read_dir().unwrap().next().is_some(),
        "uninstall on {target} did not deposit anything under ~/.runai/trash/"
    );
}

#[test]
fn round_trip_claude() {
    round_trip("claude", "rt-claude");
}

#[test]
fn round_trip_codex() {
    round_trip("codex", "rt-codex");
}

#[test]
fn round_trip_gemini() {
    round_trip("gemini", "rt-gemini");
}

#[test]
fn round_trip_opencode() {
    round_trip("opencode", "rt-opencode");
}

/// Cross-target enable: enabling the same skill on two different targets
/// produces two symlinks (one per target) pointing at the same managed dir.
/// Disabling one leaves the other intact.
#[test]
fn enable_two_targets_keeps_both_symlinks_independent() {
    let env = TestEnv::new();
    let skill_dir = env.home().join(".runai/skills");
    make_skill(&skill_dir, "shared");
    assert!(env.run(&["scan"]).status.success());

    assert!(
        env.run(&["enable", "shared", "--target", "claude"])
            .status
            .success()
    );
    assert!(
        env.run(&["enable", "shared", "--target", "codex"])
            .status
            .success()
    );

    let claude_link = env.cli_skills_dir("claude").join("shared");
    let codex_link = env.cli_skills_dir("codex").join("shared");
    assert!(std::fs::symlink_metadata(&claude_link).is_ok());
    assert!(std::fs::symlink_metadata(&codex_link).is_ok());

    // Disable on claude — codex must remain.
    assert!(
        env.run(&["disable", "shared", "--target", "claude"])
            .status
            .success()
    );
    assert!(
        std::fs::symlink_metadata(&claude_link).is_err(),
        "claude link should be gone after disable"
    );
    assert!(
        std::fs::symlink_metadata(&codex_link).is_ok(),
        "codex link should remain after disabling on claude"
    );
}
