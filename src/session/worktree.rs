//! Git worktrees for sessions (002 US6, FR-031 to FR-037, research R-11).
//!
//! Fallow drives the user's own `git`. Each call is a short child process made as part of an
//! explicit user action (opening the start menu, starting, restarting or closing a session),
//! never in the background (FR-037). Worktrees live next to the repository in
//! `<project>.worktrees/`, branches are `fallow/<profile>-<n>`, existing branches are never reused
//! or reset, and a worktree with uncommitted work is never removed (`--force` is never passed).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Branch prefix of the worktrees Fallow creates.
pub const PREFIX: &str = "fallow/";

/// A worktree Fallow created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

/// The repository root above `dir`, found by looking for `.git` (a directory, or a file in a
/// worktree or submodule). No process is started.
pub fn git_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

pub fn is_git_project(dir: &Path) -> bool {
    git_root(dir).is_some()
}

/// Whether the repository has a commit to branch from (`git init` without a commit has an
/// unborn `HEAD`, and `git worktree add` cannot start there; 002 T062).
pub fn has_commits(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
}

/// What to tell the user when a repository has no commits yet.
pub const NO_COMMITS: &str =
    "This repository has no commits yet: make a first commit to use worktrees";

/// Runs `git` in `dir`; on failure returns git's own message.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            err
        })
    }
}

/// Normalizes a path from git's output for comparison (git prints `/` on Windows too).
fn norm(p: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(p.replace('/', "\\"))
    } else {
        PathBuf::from(p)
    }
}

/// The worktrees of `project` whose branch starts with `fallow/` (FR-032).
pub fn list(project: &Path) -> Result<Vec<Worktree>, String> {
    let text = git(project, &["worktree", "list", "--porcelain"])?;
    Ok(parse_list(&text))
}

/// Parses `git worktree list --porcelain`.
pub fn parse_list(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(norm(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let (Some(path), Some(branch)) = (path, branch) {
            if branch.starts_with(PREFIX) {
                out.push(Worktree { path, branch });
            }
        }
    }
    out
}

/// The folder for a new worktree: `<parent>/<project>.worktrees/<name>`, or `<root>/<name>` when
/// `sessions.worktree_root` is set (FR-034).
pub fn worktree_dir(project: &Path, root: Option<&Path>, name: &str) -> PathBuf {
    match root {
        Some(r) => r.join(name),
        None => {
            let base = project.file_name().map_or_else(
                || "project".to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            project
                .parent()
                .unwrap_or(project)
                .join(format!("{base}.worktrees"))
                .join(name)
        }
    }
}

/// The first free `fallow/<profile>-<n>` whose folder doesn't exist either (FR-033).
pub fn next_branch(project: &Path, profile: &str, root: Option<&Path>) -> Result<String, String> {
    for n in 1..10_000 {
        let name = format!("{profile}-{n}");
        let branch = format!("{PREFIX}{name}");
        let taken = git(
            project,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok();
        if !taken && !worktree_dir(project, root, &name).exists() {
            return Ok(branch);
        }
    }
    Err("no free branch name".into())
}

/// Creates a worktree on a new branch from the project's current commit. On failure nothing
/// half-created is left registered (US6 scenario 7).
pub fn create(project: &Path, branch: &str, root: Option<&Path>) -> Result<Worktree, String> {
    let name = branch.strip_prefix(PREFIX).unwrap_or(branch);
    let path = worktree_dir(project, root, name);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    if !has_commits(project) {
        return Err(NO_COMMITS.into());
    }
    let base = git(project, &["rev-parse", "HEAD"])?.trim().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let p = path.display().to_string();
    match git(project, &["worktree", "add", "-b", branch, &p, &base]) {
        Ok(_) => Ok(Worktree {
            path,
            branch: branch.to_string(),
        }),
        Err(e) => {
            // git may have registered the worktree before failing; a fresh checkout is safe to
            // unregister, and `worktree remove` without --force refuses anything else.
            let _ = git(project, &["worktree", "remove", &p]);
            let _ = git(project, &["worktree", "prune"]);
            Err(e)
        }
    }
}

/// True when the worktree has no uncommitted changes and no untracked files.
pub fn is_clean(path: &Path) -> Result<bool, String> {
    let s = git(path, &["status", "--porcelain", "--untracked-files=normal"])?;
    Ok(s.trim().is_empty())
}

/// Removes a clean worktree; the branch is kept (FR-035). Never forced.
pub fn remove(project: &Path, path: &Path) -> Result<(), String> {
    if !is_clean(path)? {
        return Err("it has uncommitted changes or untracked files".into());
    }
    git(
        project,
        &["worktree", "remove", &path.display().to_string()],
    )
    .map(|_| ())
}
