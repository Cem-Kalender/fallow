//! 002 T054: git worktrees for sessions against a real repository (US6, FR-031 to FR-037,
//! SC-010, research R-11).

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow::session::worktree::{self, Worktree, parse_list};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `<tmp>/<name>/repo` with one commit, so worktrees land in `<tmp>/<name>/repo.worktrees`.
fn repo(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fallow-wt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let r = std::path::absolute(base.join("repo")).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    git(&r, &["config", "user.email", "t@example.invalid"]);
    git(&r, &["config", "user.name", "t"]);
    git(&r, &["config", "commit.gpgsign", "false"]);
    std::fs::write(r.join("a.txt"), "original\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "init"]);
    r
}

fn cleanup(r: &Path) {
    let _ = std::fs::remove_dir_all(r.parent().unwrap());
}

#[test]
fn detection_walks_up_to_dot_git() {
    let r = repo("detect");
    std::fs::create_dir_all(r.join("src/deep")).unwrap();
    assert!(worktree::is_git_project(&r));
    assert!(worktree::is_git_project(&r.join("src/deep")));
    let plain = r.parent().unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    // The temp folder itself must not be inside a repository for this check to mean anything.
    if worktree::git_root(r.parent().unwrap()).is_none() {
        assert!(!worktree::is_git_project(&plain));
    }
    cleanup(&r);
}

#[test]
fn branches_and_paths_follow_the_contract() {
    let r = repo("names");
    let b = worktree::next_branch(&r, "claude", None).unwrap();
    assert_eq!(b, "fallow/claude-1");
    // An existing branch is never reused (FR-033).
    git(&r, &["branch", "fallow/claude-1"]);
    assert_eq!(
        worktree::next_branch(&r, "claude", None).unwrap(),
        "fallow/claude-2"
    );
    let p = worktree::worktree_dir(&r, None, "claude-2");
    assert_eq!(
        p,
        r.parent().unwrap().join("repo.worktrees").join("claude-2")
    );
    let root = r.parent().unwrap().join("elsewhere");
    assert_eq!(
        worktree::worktree_dir(&r, Some(&root), "x-1"),
        root.join("x-1")
    );
    cleanup(&r);
}

#[test]
fn worktrees_are_isolated_and_listed() {
    let r = repo("iso");
    let a = worktree::create(&r, "fallow/a-1", None).unwrap();
    let b = worktree::create(&r, "fallow/b-1", None).unwrap();
    assert!(a.path.join("a.txt").is_file() && b.path.join("a.txt").is_file());
    std::fs::write(a.path.join("a.txt"), "changed by agent A\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(b.path.join("a.txt"))
            .unwrap()
            .trim(),
        "original"
    );
    assert_eq!(
        std::fs::read_to_string(r.join("a.txt")).unwrap().trim(),
        "original"
    );
    // Only fallow/ branches are listed; the main worktree is not.
    git(&r, &["branch", "other"]);
    let wt2 = r.parent().unwrap().join("other-wt");
    git(
        &r,
        &["worktree", "add", &wt2.display().to_string(), "other"],
    );
    let mut listed = worktree::list(&r).unwrap();
    listed.sort_by(|x, y| x.branch.cmp(&y.branch));
    let branches: Vec<&str> = listed.iter().map(|w| w.branch.as_str()).collect();
    assert_eq!(branches, ["fallow/a-1", "fallow/b-1"]);
    let same = |x: &Path, y: &Path| {
        x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
    };
    assert!(listed.iter().any(|w| same(&w.path, &a.path)), "{listed:?}");
    cleanup(&r);
}

#[test]
fn removal_never_takes_uncommitted_work_and_keeps_the_branch() {
    let r = repo("remove");
    let w = worktree::create(&r, "fallow/c-1", None).unwrap();
    // Untracked file: refused.
    std::fs::write(w.path.join("new.txt"), "x").unwrap();
    assert!(!worktree::is_clean(&w.path).unwrap());
    assert!(worktree::remove(&r, &w.path).is_err());
    assert!(w.path.is_dir());
    std::fs::remove_file(w.path.join("new.txt")).unwrap();
    // Modified tracked file: refused.
    std::fs::write(w.path.join("a.txt"), "edit").unwrap();
    assert!(worktree::remove(&r, &w.path).is_err());
    git(&w.path, &["checkout", "--", "a.txt"]);
    // Clean: removed, branch kept.
    assert!(worktree::is_clean(&w.path).unwrap());
    worktree::remove(&r, &w.path).unwrap();
    assert!(!w.path.exists());
    let out = Command::new("git")
        .current_dir(&r)
        .args(["branch", "--list", "fallow/c-1"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("fallow/c-1"));
    cleanup(&r);
}

#[test]
fn a_failed_create_leaves_nothing_behind() {
    let r = repo("fail");
    let path = worktree::worktree_dir(&r, None, "d-1");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "a file where the folder should go").unwrap();
    let e = worktree::create(&r, "fallow/d-1", None).unwrap_err();
    assert!(e.contains("exists"), "{e}");
    assert!(worktree::list(&r).unwrap().is_empty());
    cleanup(&r);
}

#[test]
fn porcelain_parsing() {
    let text = "worktree C:/src/api\nHEAD abc\nbranch refs/heads/main\n\nworktree C:/src/api.worktrees/claude-1\nHEAD def\nbranch refs/heads/fallow/claude-1\n\nworktree C:/x\nHEAD 1\ndetached\n\n";
    let l = parse_list(text);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].branch, "fallow/claude-1");
    assert!(l[0].path.to_string_lossy().ends_with("claude-1"));
    let _ = Worktree {
        path: PathBuf::new(),
        branch: String::new(),
    };
}

#[test]
fn starting_fallow_runs_no_git() {
    // FR-037: AppState creation must not list worktrees or run git; it only reads the folder.
    let r = repo("startup");
    let before = std::fs::metadata(r.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    let s = fallow::app::state::AppState::new(Some(r.clone()), Vec::new());
    assert!(s.sessions.sessions.is_empty());
    let after = std::fs::metadata(r.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
    assert!(!r.parent().unwrap().join("repo.worktrees").exists());
    cleanup(&r);
}
