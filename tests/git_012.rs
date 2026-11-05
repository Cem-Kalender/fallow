//! 012 T008 (research R-2, R-3, R-10, R-18): Fallow's git commands for solo sessions run no
//! hook and sign nothing; a snapshot commits nothing when nothing changed, follows the ignore
//! rules, and refuses a working copy that left its branch; reads in the engineer's folder take
//! no lock.

mod common;

use common::*;
use fallow::verify::git as g;

fn setup(name: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let root = scratch(name);
    g::set_nohooks(root.join("data/nohooks")).unwrap();
    let repo = repo_with_markers(&root);
    let wt = root.join("wt");
    (root, repo, wt)
}

#[test]
fn a_session_worktree_and_its_snapshots_run_no_hook_and_sign_nothing() {
    let (root, repo, wt) = setup("g012-snap");
    // Signing is asked for in the repository: the snapshot still is not signed.
    git(&repo, &["config", "commit.gpgsign", "true"]);
    let start = g::head(&repo).unwrap();
    g::worktree_add_new(&repo, &wt, "fallow/solo/s1", &start).unwrap();
    assert!(g::worktree_add_new(&repo, &root.join("wt2"), "fallow/solo/s1", &start).is_err(), "a taken name is refused");
    g::rename_branch_nohooks(&wt, "fallow/solo/s1", "fallow/solo/fix-it").unwrap();
    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    std::fs::write(wt.join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(wt.join("build")).unwrap();
    std::fs::write(wt.join("build/out.bin"), "x").unwrap();
    let c = g::snapshot(&wt, "fallow/solo/fix-it", "Fallow solo s1, turn 1: fix it")
        .unwrap()
        .expect("a commit");
    assert!(hooks_ran(&root).is_empty(), "no hook ran: {:?}", hooks_ran(&root));
    let raw = git(&wt, &["cat-file", "-p", &c]);
    assert!(!raw.contains("gpgsig"), "not signed: {raw}");
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "Fallow solo s1, turn 1: fix it");
    let files = git(&wt, &["show", "--name-only", "--format=", &c]);
    assert!(files.contains("a.txt") && !files.contains("build/"), "ignore rules followed: {files}");
    assert_eq!(g::rev(&repo, "refs/heads/fallow/solo/fix-it").unwrap(), c);
    // The engineer's folder and branch did not move.
    assert_eq!(g::head(&repo).unwrap(), start);
    assert_eq!(g::branch(&repo).as_deref(), Some("main"));
    // Nothing changed: no commit.
    assert_eq!(g::snapshot(&wt, "fallow/solo/fix-it", "again").unwrap(), None);
    assert!(hooks_ran(&root).is_empty());
    // The control: an ordinary commit in the same repository does run the hooks.
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "control"]);
    let ran = hooks_ran(&root);
    assert!(ran.iter().any(|h| h == "pre-commit") && ran.iter().any(|h| h == "post-commit"), "{ran:?}");
}

#[test]
fn a_snapshot_refuses_a_working_copy_off_its_branch() {
    let (_root, repo, wt) = setup("g012-off");
    let start = g::head(&repo).unwrap();
    g::worktree_add_new(&repo, &wt, "fallow/solo/s2", &start).unwrap();
    git(&wt, &["checkout", "-q", "--detach"]);
    std::fs::write(wt.join("a.txt"), "x\n").unwrap();
    let e = g::snapshot(&wt, "fallow/solo/s2", "m").unwrap_err();
    assert!(e.contains("not on the session's branch (it is on a detached commit)"), "{e}");
}

#[test]
fn refs_and_the_folders_status_are_read_without_a_lock() {
    let (root, repo, _wt) = setup("g012-refs");
    let bare = bare_remote(&root, &repo, "https://github.com/acme/shop.git");
    let _ = bare;
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["fetch", "-q", "origin"]);
    let refs = g::ref_snapshot(&repo).unwrap();
    assert!(refs.contains_key("refs/heads/main"), "{refs:?}");
    assert!(refs.contains_key("refs/remotes/origin/main"), "{refs:?}");
    std::fs::write(repo.join("new.txt"), "n").unwrap();
    let st = g::folder_status(&repo).unwrap();
    assert!(st.iter().any(|l| l.ends_with("new.txt")), "{st:?}");
    assert!(!repo.join(".git/index.lock").exists());
    assert_eq!(g::blob_of(&repo, "new.txt").unwrap().len(), 40);
    assert_eq!(g::blob_of(&repo, "gone.txt"), None);
}

#[test]
fn the_identity_and_the_git_check() {
    let (_root, repo, _wt) = setup("g012-id");
    assert_eq!(g::identity(&repo), Some(("t".into(), "t@example.invalid".into())));
    let c = g::git_check();
    assert!(c.ok, "this machine has git {:?}", c.found);
    assert_eq!(c.min, "2.38");
}
