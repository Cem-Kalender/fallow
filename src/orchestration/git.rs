//! Read-only git calls during a burst (004 research R-4).
//!
//! Each call runs the user's own `git` in a session's worktree or the project folder and returns
//! its result; `spawn` runs one off the main loop and hands the result back through a callback.
//! Nothing here writes to a repository except `copy_at`, which creates a run's own copy in Fallow's
//! cache (005); the engineer's confirmed local merge lives in `publish`.

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

/// The files git tracks in `dir`.
pub fn tracked_files(dir: &Path) -> Result<Vec<String>, String> {
    Ok(git(dir, &["ls-files"])?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Whether `commit` names a commit in this repository.
pub fn commit_exists(dir: &Path, commit: &str) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .output()
        .is_ok_and(|o| o.status.success())
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

/// A read-only session's worktree now (FR-013): the tip of its branch (or `HEAD`), its
/// uncommitted changes, and the commits on its branch since `base` (none without a base).
pub fn read_only_check(
    dir: &Path,
    branch: Option<&str>,
    base: Option<&str>,
) -> Result<(String, Vec<String>, Vec<String>), String> {
    let at = branch.unwrap_or("HEAD");
    let tip = git(dir, &["rev-parse", at])?.trim().to_string();
    let changed = changes(dir)?;
    let commits = match base {
        Some(b) => commits_between(dir, b, at)?,
        None => Vec::new(),
    };
    Ok((tip, changed, commits))
}

/// The text of `path` at `commit`; `Ok(None)` when the commit has no such file (005 R-1).
pub fn show(dir: &Path, commit: &str, path: &str) -> Result<Option<String>, String> {
    if !commit_exists(dir, commit) {
        return Err(format!("no commit {commit}"));
    }
    let spec = format!("{commit}:{path}");
    let exists = Command::new("git")
        .current_dir(dir)
        .args(["cat-file", "-e", &spec])
        .output()
        .is_ok_and(|o| o.status.success());
    if !exists {
        return Ok(None);
    }
    git(dir, &["show", &spec]).map(Some)
}

/// Commits in `to` that are not in `from` and change one of `paths` (005 R-5).
pub fn commits_touching(
    dir: &Path,
    from: &str,
    to: &str,
    paths: &[&str],
) -> Result<Vec<String>, String> {
    let range = format!("{from}..{to}");
    let mut args = vec!["rev-list", range.as_str(), "--"];
    args.extend_from_slice(paths);
    Ok(git(dir, &args)?.lines().map(str::to_string).collect())
}

/// Whether `a` is `b` or an ancestor of it.
pub fn is_ancestor(dir: &Path, a: &str, b: &str) -> bool {
    on_branch(dir, a, b)
}

/// A one-way fingerprint of `text` with line endings normalised to LF (005 R-12), computed by
/// git so that no hashing crate is needed.
pub fn fingerprint(text: &str) -> Result<String, String> {
    use std::io::Write;
    let normalised = text.replace("\r\n", "\n");
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("git: no stdin")?
        .write_all(normalised.as_bytes())
        .map_err(|e| format!("git: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// A new repository at `dest` holding only `commit` of `project` and its history, checked out
/// on `main` (005 FR-011a, R-11). The project is only read; `dest` has no remote, so a run
/// cannot see another run's work.
pub fn copy_at(project: &Path, commit: &str, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    git(dest, &["init", "-q", "-b", "main"])?;
    let source = project.display().to_string();
    git(dest, &["fetch", "-q", "--no-tags", &source, commit])?;
    git(dest, &["checkout", "-q", "-B", "main", "FETCH_HEAD"])?;
    Ok(())
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
