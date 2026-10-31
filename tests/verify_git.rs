//! 004 T007: the read-only git calls of research R-4, against a real temporary repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow::verify::git;

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
fn reads_a_file_at_a_commit_and_the_commits_that_touched_paths() {
    // 005 T004: show at commit, commits over paths, ancestry.
    let r = repo("paths");
    let start = git::head(&r).unwrap();
    assert_eq!(
        git::show(&r, &start, "a.txt").unwrap().as_deref(),
        Some("one\n")
    );
    assert_eq!(git::show(&r, &start, ".fallow/work.toml").unwrap(), None);
    assert!(git::show(&r, "no-such-commit", "a.txt").is_err());
    std::fs::create_dir_all(r.join("contracts")).unwrap();
    std::fs::write(r.join("contracts/c.yaml"), "v1\n").unwrap();
    run(&r, &["add", "."]);
    run(&r, &["commit", "-q", "-m", "contract"]);
    let c1 = git::head(&r).unwrap();
    std::fs::write(r.join("b.txt"), "b\n").unwrap();
    run(&r, &["add", "."]);
    run(&r, &["commit", "-q", "-m", "other"]);
    let c2 = git::head(&r).unwrap();
    assert_eq!(
        git::commits_touching(&r, &start, &c2, &["contracts/c.yaml"]).unwrap(),
        vec![c1.clone()]
    );
    assert!(
        git::commits_touching(&r, &c1, &c2, &["contracts/c.yaml", ".fallow/work.toml"])
            .unwrap()
            .is_empty()
    );
    assert!(git::is_ancestor(&r, &c1, &c2));
    assert!(!git::is_ancestor(&r, &c2, &c1));
    let _ = std::fs::remove_dir_all(r.parent().unwrap());
}

#[test]
fn fingerprints_ignore_line_endings() {
    // 005 R-12: one-way, stable across checkouts.
    let a = git::fingerprint("one\r\ntwo\r\n").unwrap();
    let b = git::fingerprint("one\ntwo\n").unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 40);
    assert_ne!(a, git::fingerprint("one\ntwo\nthree\n").unwrap());
    assert!(!a.contains("one"));
}

#[test]
fn a_run_copy_holds_only_the_history_up_to_its_start() {
    // 005 T004, FR-011a: no other branch, no later commit; the project's refs unchanged.
    let r = repo("copy");
    let start = git::head(&r).unwrap();
    run(&r, &["checkout", "-q", "-b", "fallow/old-run"]);
    std::fs::write(r.join("solution.txt"), "earlier run\n").unwrap();
    run(&r, &["add", "."]);
    run(&r, &["commit", "-q", "-m", "an earlier solution"]);
    run(&r, &["checkout", "-q", "main"]);
    let refs_before = {
        let o = Command::new("git")
            .current_dir(&r)
            .args(["for-each-ref"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let dest = r.parent().unwrap().join("copy");
    git::copy_at(&r, &start, &dest).unwrap();
    assert_eq!(git::head(&dest).unwrap(), start);
    assert_eq!(git::branch(&dest).as_deref(), Some("main"));
    assert!(dest.join("a.txt").exists());
    let o = Command::new("git")
        .current_dir(&dest)
        .args(["log", "--all", "--format=%s"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&o.stdout);
    assert!(!log.contains("earlier solution"), "{log}");
    assert!(!git::has_remote(&dest), "no way back to the project");
    let refs_after = {
        let o = Command::new("git")
            .current_dir(&r)
            .args(["for-each-ref"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    assert_eq!(refs_before, refs_after, "the project is only read");
    let _ = std::fs::remove_dir_all(r.parent().unwrap());
}

/// T086 (found with real agents): Spec Kit installed into the engineer's folder, never committed,
/// left its template constitution there untracked; a feature bringing its own constitution could
/// not merge. Fallow's unchanged copy is set aside; the engineer's own file never is.
#[test]
fn a_merge_replaces_fallows_unchanged_spec_kit_copy_but_never_the_engineers_file() {
    let r = repo("kit-merge");
    let constitution = ".specify/memory/constitution.md";
    let template = fallow::project::install::shipped(constitution).unwrap();
    let feature = |name: &str| {
        run(&r, &["checkout", "-q", "-b", name]);
        std::fs::create_dir_all(r.join(".specify/memory")).unwrap();
        std::fs::write(r.join(constitution), "# Our constitution\n").unwrap();
        run(&r, &["add", "."]);
        run(&r, &["commit", "-q", "-m", "constitution"]);
        let c = git::head(&r).unwrap();
        run(&r, &["checkout", "-q", "main"]);
        std::fs::remove_dir_all(r.join(".specify")).ok();
        c
    };
    let scratch = r.parent().unwrap().join("scratch");

    // The engineer's own untracked constitution: the merge is refused and the file stays.
    let c1 = feature("f1");
    std::fs::create_dir_all(r.join(".specify/memory")).unwrap();
    std::fs::write(r.join(constitution), "mine\n").unwrap();
    let e = fallow::publish::local::merge(&r, "main", &c1, &scratch).unwrap_err();
    assert!(e.contains("constitution.md"), "{e}");
    assert!(
        !e.contains("did not go through"),
        "the caller says that: {e}"
    );
    assert_eq!(
        std::fs::read_to_string(r.join(constitution)).unwrap(),
        "mine\n"
    );

    // Fallow's unchanged copy: the feature's version takes its place.
    std::fs::write(r.join(constitution), template).unwrap();
    fallow::publish::local::merge(&r, "main", &c1, &scratch).unwrap();
    assert_eq!(
        std::fs::read_to_string(r.join(constitution))
            .unwrap()
            .replace('\r', ""),
        "# Our constitution\n"
    );
}
