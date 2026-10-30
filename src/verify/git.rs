//! Git calls for runs (006 research R-7, R-9).
//!
//! Each call runs the engineer's own `git`. Reads check claims; the writes create and remove a
//! run's own branches (`fallow/r<id>/…`) and worktrees, and nothing else. The engineer's merge
//! lives in `publish`.

use std::path::{Path, PathBuf};

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::platform::command("git")
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

/// Changes to tracked files only: untracked files never block a merge.
pub fn tracked_changes(dir: &Path) -> Result<Vec<String>, String> {
    Ok(
        git(dir, &["status", "--porcelain", "--untracked-files=no"])?
            .lines()
            .filter(|l| l.len() > 3)
            .map(|l| l[3..].to_string())
            .collect(),
    )
}

/// Changed or untracked files in the worktree (`status --porcelain`).
pub fn changes(dir: &Path) -> Result<Vec<String>, String> {
    Ok(git(dir, &["status", "--porcelain"])?
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l[3..].to_string())
        .collect())
}

/// Changed or untracked files, each untracked file named on its own (not its folder).
pub fn changes_all(dir: &Path) -> Result<Vec<String>, String> {
    Ok(
        git(dir, &["status", "--porcelain", "--untracked-files=all"])?
            .lines()
            .filter(|l| l.len() > 3)
            .map(|l| l[3..].trim_matches('"').to_string())
            .collect(),
    )
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
    crate::platform::command("git")
        .current_dir(dir)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Whether `commit` is on `branch` (an ancestor of its tip, or the tip).
pub fn on_branch(dir: &Path, commit: &str, branch: &str) -> bool {
    crate::platform::command("git")
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
    let exists = crate::platform::command("git")
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

/// The files tracked at `commit`.
pub fn files_at(dir: &Path, commit: &str) -> Result<Vec<String>, String> {
    Ok(git(dir, &["ls-tree", "-r", "--name-only", commit])?
        .lines()
        .map(str::to_string)
        .collect())
}

/// The best common ancestor of two commits.
pub fn merge_base(dir: &Path, a: &str, b: &str) -> Result<String, String> {
    git(dir, &["merge-base", a, b]).map(|s| s.trim().to_string())
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
    let mut child = crate::platform::command("git")
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

/// Creates `branch` at `at` unless it exists (a run's branch keeps the work committed on it).
pub fn create_branch(dir: &Path, branch: &str, at: &str) -> Result<(), String> {
    match git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(_) => git(dir, &["branch", branch, at]).map(|_| ()),
    }
}

/// The full commit id of `rev`.
pub fn rev(dir: &Path, rev: &str) -> Result<String, String> {
    git(
        dir,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
    .map(|s| s.trim().to_string())
}

/// Moves `branch` (not checked out anywhere) to `commit`.
pub fn set_branch(dir: &Path, branch: &str, commit: &str) -> Result<(), String> {
    git(dir, &["branch", "-f", branch, commit]).map(|_| ())
}

/// Adds a worktree at `path` on `branch`, or detached at a commit when `branch` is `None`. A
/// worktree already there is kept.
pub fn worktree_add(
    repo: &Path,
    path: &Path,
    branch: Option<&str>,
    at: &str,
) -> Result<(), String> {
    if path.join(".git").exists() {
        return Ok(());
    }
    let p = path.display().to_string();
    match branch {
        Some(b) => git(repo, &["worktree", "add", "-q", &p, b]).map(|_| ()),
        None => git(repo, &["worktree", "add", "-q", "--detach", &p, at]).map(|_| ()),
    }
}

/// Removes a worktree and its administrative files.
pub fn worktree_remove(repo: &Path, path: &Path) -> Result<(), String> {
    let p = path.display().to_string();
    let r = git(repo, &["worktree", "remove", "--force", &p]).map(|_| ());
    let _ = git(repo, &["worktree", "prune"]);
    r
}

/// Merges `commit` into the branch checked out in `dir` with a merge commit. A conflict is
/// aborted and returned as the files in conflict.
pub fn merge_into(dir: &Path, commit: &str, message: &str) -> Result<String, Vec<String>> {
    match git(dir, &["merge", "--no-ff", "-m", message, commit]) {
        Ok(_) => head(dir).map_err(|e| vec![e]),
        Err(e) => {
            let conflicts: Vec<String> = git(dir, &["diff", "--name-only", "--diff-filter=U"])
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default();
            let _ = git(dir, &["merge", "--abort"]);
            Err(if conflicts.is_empty() {
                vec![e]
            } else {
                conflicts
            })
        }
    }
}

/// The change a commit made (its first parent to it), as a unified diff.
pub fn patch_of(dir: &Path, commit: &str) -> Result<String, String> {
    git(dir, &["show", "--format=", "--patch", "--no-color", commit])
}

/// The change between two commits below `path`, as a unified diff.
pub fn diff_between(dir: &Path, from: &str, to: &str, path: &str) -> Result<String, String> {
    git(dir, &["diff", from, to, "--", path])
}

/// Starts merging `commit` into the branch checked out in `dir`, taking `commit`'s side of any
/// conflicting hunk (a new lock's documents over the build's ticks, which Fallow then restores),
/// without committing: `commit_merge` ends it. A merge left over from before is aborted first.
pub fn merge_theirs(dir: &Path, commit: &str) -> Result<(), String> {
    let _ = git(dir, &["merge", "--abort"]);
    git(dir, &["merge", "--no-ff", "--no-commit", "-X", "theirs", commit])
        .inspect_err(|_| {
            let _ = git(dir, &["merge", "--abort"]);
        })
        .map(|_| ())
}

/// Ends a merge started by `merge_theirs` as one commit: what the merge staged, and `also`.
/// Anything else in the worktree (left by a step that did not finish) stays out of it.
pub fn commit_merge(dir: &Path, message: &str, also: &[&str]) -> Result<String, String> {
    for p in also {
        git(dir, &["add", "--", p])?;
    }
    let has_author = git(dir, &["config", "user.email"]).is_ok_and(|e| !e.trim().is_empty());
    let mut args: Vec<&str> = Vec::new();
    if !has_author {
        args.extend([
            "-c",
            "user.name=Fallow",
            "-c",
            "user.email=fallow@localhost.invalid",
        ]);
    }
    args.extend(["commit", "-q", "--no-verify", "-m", message]);
    git(dir, &args)?;
    head(dir)
}

/// Whether `dir`'s tracked files are unchanged and `HEAD` is still `at`. New untracked files,
/// such as the caches running a check leaves behind, do not count (found on a real run).
pub fn unchanged(dir: &Path, at: &str) -> Result<bool, String> {
    let tracked = git(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    Ok(tracked.trim().is_empty() && rev(dir, "HEAD")? == rev(dir, at)?)
}

/// Files that running code or tests leaves behind, never committed for an agent.
pub const CACHES: &[&str] = &[
    "__pycache__",
    "*.pyc",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    ".coverage",
    "node_modules",
];

/// Commits what changed in the worktree `dir` (only below `paths`, when given) with `message`,
/// as the repository's configured author, or as Fallow when none is configured. Returns the new
/// commit, or `None` when nothing changed. Fallow commits a step's work itself, so agents need
/// no git rights (006 research R-4, verified T001).
pub fn commit_all(
    dir: &Path,
    message: &str,
    paths: Option<&[&str]>,
) -> Result<Option<String>, String> {
    match paths {
        Some(ps) => {
            // Only these paths: whatever an agent staged itself is unstaged first (Phase 3
            // review: a plain commit took everything in the index).
            git(dir, &["reset", "-q"])?;
            // One path at a time: a path that matches nothing is not an error here.
            for p in ps {
                let _ = git(dir, &["add", "-A", "--", p]);
            }
        }
        None => {
            let mut args: Vec<String> = vec!["add".into(), "-A".into(), "--".into(), ".".into()];
            for c in CACHES {
                args.push(format!(":(exclude,glob)**/{c}"));
                args.push(format!(":(exclude,glob)**/{c}/**"));
            }
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            git(dir, &a)?;
        }
    }
    let staged = git(dir, &["diff", "--cached", "--name-only"])?;
    if staged.trim().is_empty() {
        return Ok(None);
    }
    let has_author = git(dir, &["config", "user.email"]).is_ok_and(|e| !e.trim().is_empty());
    let mut args: Vec<&str> = Vec::new();
    if !has_author {
        args.extend([
            "-c",
            "user.name=Fallow",
            "-c",
            "user.email=fallow@localhost.invalid",
        ]);
    }
    args.extend(["commit", "-q", "--no-verify", "-m", message]);
    git(dir, &args)?;
    head(dir).map(Some)
}

/// Moves the worktree's branch back to `commit`, keeping the files as they are (`reset --mixed`):
/// commits an agent made itself are undone, their changes left to be judged.
pub fn reset_mixed(dir: &Path, commit: &str) -> Result<(), String> {
    git(dir, &["reset", "-q", "--mixed", commit]).map(|_| ())
}

/// Puts `paths` in the worktree `dir` back as they are at `commit` (a path that is not there is
/// removed). Fallow undoes an agent's change to files it may not change.
pub fn restore_paths(dir: &Path, commit: &str, paths: &[String]) -> Result<(), String> {
    for p in paths {
        if git(dir, &["cat-file", "-e", &format!("{commit}:{p}")]).is_ok() {
            git(dir, &["checkout", "-q", commit, "--", p])?;
        } else {
            let _ = git(dir, &["rm", "-q", "--cached", "--ignore-unmatch", "--", p]);
            let _ = std::fs::remove_file(dir.join(p));
        }
    }
    Ok(())
}

/// Whether `dir` is inside a git repository (at its top or below).
pub fn inside_repo(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .map(|s| PathBuf::from(s.trim()))
}

/// Throws away uncommitted changes and untracked files in the worktree `dir` (a run's own
/// worktree only).
pub fn discard_changes(dir: &Path) -> Result<(), String> {
    git(dir, &["reset", "-q", "--hard"])?;
    git(dir, &["clean", "-q", "-fd"]).map(|_| ())
}

/// The URL of the remote `origin`, if any.
pub fn origin_url(dir: &Path) -> Option<String> {
    git(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
