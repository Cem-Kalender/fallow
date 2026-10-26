//! Read-only git calls during a burst (004 research R-4).
//!
//! Each call runs the user's own `git` in a session's worktree or the project folder and returns
//! its result; `spawn` runs one off the main loop and hands the result back through a callback.
//! Nothing here writes to a repository; the engineer's confirmed local merge lives in `publish`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The commit checked out, in full.
pub fn head(dir: &Path) -> Result<String, String> {
    git(dir, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
}

/// The branch checked out, or `None` when `HEAD` is detached.
pub fn branch(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whether the repository has at least one remote (draft pull requests need one).
pub fn has_remote(dir: &Path) -> bool {
    git(dir, &["remote"]).is_ok_and(|s| !s.trim().is_empty())
}

/// Changed or untracked files in the worktree (`status --porcelain`).
pub fn changes(dir: &Path) -> Result<Vec<String>, String> {
    Ok(git(dir, &["status", "--porcelain"])?
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l[3..].to_string())
        .collect())
}

/// Commits in `to` that are not in `from` (`rev-list from..to`).
pub fn commits_between(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    Ok(git(dir, &["rev-list", &format!("{from}..{to}")])?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Whether `commit` is on `branch` (an ancestor of its tip, or the tip).
pub fn on_branch(dir: &Path, commit: &str, branch: &str) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["merge-base", "--is-ancestor", commit, branch])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Files changed between two commits.
pub fn files_between(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    Ok(
        git(dir, &["diff", "--name-only", &format!("{from}..{to}")])?
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

/// What a worktree looks like now, for the read-only check (FR-013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeState {
    pub path: PathBuf,
    pub changes: Vec<String>,
    pub own_commits: Vec<String>,
}

impl WorktreeState {
    pub fn clean(&self) -> bool {
        self.changes.is_empty() && self.own_commits.is_empty()
    }
}

/// Reads a read-only session's worktree: uncommitted changes and commits since `start`.
pub fn worktree_state(path: &Path, start: &str) -> Result<WorktreeState, String> {
    Ok(WorktreeState {
        path: path.to_path_buf(),
        changes: changes(path)?,
        own_commits: commits_between(path, start, "HEAD")?,
    })
}

/// Runs `job` on a short-lived thread and passes its result to `done` there.
pub fn spawn<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("orch-git".into())
        .spawn(move || done(job()));
}
