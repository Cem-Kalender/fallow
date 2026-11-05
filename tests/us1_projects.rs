//! 007 US1 (T022, FR-001, FR-002, constitution VI): adding a project installs Spec Kit only where
//! files are missing and overwrites nothing; an existing Spec Kit version is reported and left
//! alone; a repository, a folder that needs git and a subfolder of a repository are told apart
//! (012 FR-039: Spec Kit is written only into a repository); a missing folder is shown as
//! missing; removing a project deletes nothing in its folder.

use std::path::{Path, PathBuf};

use fallow::project::{Kind, Store, install};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-p-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// `dir` as a repository with one commit (012: only a repository gets Spec Kit).
fn committed(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@example.invalid"]);
    git(dir, &["config", "user.name", "t"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "seed"]);
}

#[test]
fn adding_installs_spec_kit_only_where_files_are_missing() {
    let root = scratch("install");
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let proj = root.join("Shop Demo");
    std::fs::create_dir_all(proj.join(".specify/memory")).unwrap();
    // The engineer's own constitution and an unrelated file must survive untouched.
    std::fs::write(proj.join(".specify/memory/constitution.md"), "# Ours\n").unwrap();
    std::fs::write(proj.join("main.py"), "print(1)\n").unwrap();
    committed(&proj);
    let mut store = Store::load(&data).unwrap();
    let (p, report) = store.add(&proj, "2026-10-30T12:00:00Z").unwrap();
    assert_eq!(p.id, "shop-demo");
    assert_eq!(p.kind(), Kind::Git);
    assert_eq!(report.found, None);
    assert!(
        report
            .kept
            .contains(&".specify/memory/constitution.md".to_string())
    );
    assert!(report.installed.len() >= 20, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(proj.join(".specify/memory/constitution.md")).unwrap(),
        "# Ours\n"
    );
    assert_eq!(
        std::fs::read_to_string(proj.join("main.py")).unwrap(),
        "print(1)\n"
    );
    assert!(
        proj.join(".claude/skills/speckit-specify/SKILL.md")
            .exists()
    );
    assert!(proj.join(".specify/scripts/bash/common.sh").exists());
    assert!(proj.join(".specify/LICENSE-spec-kit").exists());
    assert_eq!(
        install::version_in(&proj).as_deref(),
        Some(install::VERSION)
    );
    // Saved, and a second add of the same folder is refused.
    let mut again = Store::load(&data).unwrap();
    assert_eq!(again.projects.len(), 1);
    assert!(again.add(&proj, "later").is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn another_spec_kit_version_is_reported_and_left_alone() {
    let root = scratch("version");
    let proj = root.join("old");
    std::fs::create_dir_all(proj.join(".specify")).unwrap();
    std::fs::write(
        proj.join(".specify/integration.json"),
        "{\"version\": \"0.9.0\"}",
    )
    .unwrap();
    committed(&proj);
    let mut store = Store::load(&root).unwrap();
    let (_, report) = store.add(&proj, "now").unwrap();
    assert_eq!(report.found.as_deref(), Some("0.9.0"));
    assert!(
        report.installed.is_empty(),
        "nothing mixed into another version"
    );
    assert!(!proj.join(".claude").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn git_and_folder_kinds_a_missing_folder_and_removal() {
    let root = scratch("kinds");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    committed(&repo);
    let folder = root.join("plain");
    std::fs::create_dir_all(&folder).unwrap();
    let fresh = root.join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    git(&fresh, &["init", "-q"]);
    let mut store = Store::load(&root).unwrap();
    let (r, _) = store.add(&repo, "now").unwrap();
    let (f, _) = store.add(&folder, "now").unwrap();
    let (n, _) = store.add(&fresh, "now").unwrap();
    assert_eq!(r.kind(), Kind::Git);
    // 012 (FR-039): a folder without git, or a repository without a commit, needs git; nothing
    // is written in it.
    assert_eq!(f.kind(), Kind::NeedsGit);
    assert_eq!(n.kind(), Kind::NeedsGit);
    assert!(!folder.join(".specify").exists());
    // A subfolder of a repository is refused, naming the top folder: worktrees are of whole
    // repositories.
    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let err = store.add(&sub, "now").unwrap_err();
    assert!(err.contains("inside"), "{err}");
    // Moved away: shown as missing, its record kept.
    std::fs::rename(&folder, root.join("moved")).unwrap();
    let again = Store::load(&root).unwrap();
    assert_eq!(again.get(&f.id).unwrap().kind(), Kind::Missing);
    // Removing deletes nothing in the folder.
    let mut store = Store::load(&root).unwrap();
    store.remove(&r.id).unwrap();
    assert!(repo.join(".specify/memory/constitution.md").exists());
    assert!(repo.join(".git").exists());
    assert!(Store::load(&root).unwrap().get(&r.id).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn feature_folders_are_numbered_after_the_existing_ones() {
    let root = scratch("numbers");
    let proj = root.join("p");
    std::fs::create_dir_all(proj.join("specs/004-old-thing")).unwrap();
    std::fs::create_dir_all(proj.join("specs/notes")).unwrap();
    let mut store = Store::load(&root).unwrap();
    let (p, _) = store.add(&proj, "now").unwrap();
    assert_eq!(p.spec_dirs(), ["specs/004-old-thing"]);
    assert_eq!(p.next_dir("Discount codes!"), "specs/005-discount-codes");
    let _ = std::fs::remove_dir_all(&root);
}
