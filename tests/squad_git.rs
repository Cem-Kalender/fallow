//! 004 T007: the read-only git calls of research R-4, against a real temporary repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow::orchestration::git;

fn run(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn repo(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fallow-sq-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let r = std::path::absolute(base.join("repo")).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    run(&r, &["init", "-q", "-b", "main"]);
    run(&r, &["config", "user.email", "t@example.invalid"]);
    run(&r, &["config", "user.name", "t"]);
    run(&r, &["config", "commit.gpgsign", "false"]);
    std::fs::write(r.join("a.txt"), "one\n").unwrap();
    run(&r, &["add", "."]);
    run(&r, &["commit", "-q", "-m", "init"]);
    r
}

#[test]
fn reads_heads_branches_remotes_changes_and_ancestry() {
    let r = repo("basic");
    let start = git::head(&r).unwrap();
    assert_eq!(start.len(), 40);
    assert_eq!(git::branch(&r).as_deref(), Some("main"));
    assert!(!git::has_remote(&r));
    assert!(git::changes(&r).unwrap().is_empty());
    // A branch with one commit.
    run(&r, &["checkout", "-q", "-b", "fallow/builder-1"]);
    std::fs::write(r.join("b.txt"), "two\n").unwrap();
    run(&r, &["add", "."]);
    run(&r, &["commit", "-q", "-m", "b"]);
    let c = git::head(&r).unwrap();
    assert!(git::on_branch(&r, &c, "fallow/builder-1"));
    assert!(!git::on_branch(&r, &c, "main"));
    assert!(git::on_branch(&r, &start, "main"));
    assert_eq!(
        git::commits_between(&r, &start, "HEAD").unwrap(),
        vec![c.clone()]
    );
    assert_eq!(
        git::files_between(&r, &start, &c).unwrap(),
        vec!["b.txt".to_string()]
    );
    // An untracked file shows as a change; a read-only worktree with it is not clean.
    std::fs::write(r.join("c.txt"), "three\n").unwrap();
    let st = git::worktree_state(&r, &start).unwrap();
    assert_eq!(st.changes, vec!["c.txt".to_string()]);
    assert!(!st.clean());
    run(
        &r,
        &["remote", "add", "origin", "https://example.invalid/x.git"],
    );
    assert!(git::has_remote(&r));
    let _ = std::fs::remove_dir_all(r.parent().unwrap());
}

#[test]
fn work_runs_off_the_calling_thread() {
    let (tx, rx) = std::sync::mpsc::channel();
    let caller = std::thread::current().id();
    git::spawn(
        move || std::thread::current().id(),
        move |id| {
            let _ = tx.send(id);
        },
    );
    let id = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_ne!(id, caller);
}
