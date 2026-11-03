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

/// The files `to` has that `from` does not (`diff --diff-filter=A from to`).
pub fn added_files(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    Ok(git(
        dir,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=A",
            from,
            to,
        ],
    )?
    .lines()
    .map(str::to_string)
    .collect())
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

/// Forgets worktrees whose folders are gone, so their branches can be deleted.
pub fn worktree_prune(repo: &Path) {
    let _ = git(repo, &["worktree", "prune"]);
}

/// Deletes `branch` (not checked out anywhere), merged or not; one that is not there is fine.
pub fn delete_branch(dir: &Path, branch: &str) -> Result<(), String> {
    if git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_err()
    {
        return Ok(());
    }
    git(dir, &["branch", "-D", branch]).map(|_| ())
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
    git(
        dir,
        &["merge", "--no-ff", "--no-commit", "-X", "theirs", commit],
    )
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

// ---- 009: read-only views of a working copy or a branch (research R-8) ----------------------

fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = crate::platform::command("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn nul_list(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// The files of the working copy `dir` that the project does not ignore: tracked and untracked,
/// by the project's own ignore rules.
pub fn listed(dir: &Path) -> Result<Vec<String>, String> {
    let mut v = nul_list(&git_bytes(
        dir,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?);
    v.sort();
    v.dedup();
    Ok(v)
}

/// What changed against `base`, as (`A`, `M` or `D`, path): the working tree of `dir` when `to`
/// is `None` (uncommitted changes and untracked files included), else the commit `to`.
pub fn name_status(
    dir: &Path,
    base: &str,
    to: Option<&str>,
) -> Result<Vec<(char, String)>, String> {
    let mut args = vec!["diff", "--name-status", "--no-renames", "-z", base];
    if let Some(t) = to {
        args.push(t);
    }
    let parts = nul_list(&git_bytes(dir, &args)?);
    let mut out = Vec::new();
    let mut it = parts.into_iter();
    while let (Some(st), Some(path)) = (it.next(), it.next()) {
        let c = match st.chars().next() {
            Some('A') => 'A',
            Some('D') => 'D',
            _ => 'M',
        };
        out.push((c, path));
    }
    if to.is_none() {
        for p in nul_list(&git_bytes(
            dir,
            &["ls-files", "-z", "--others", "--exclude-standard"],
        )?) {
            out.push(('A', p));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out.dedup_by(|a, b| a.1 == b.1);
    Ok(out)
}

/// The last commit after `base` up to `to` (default `HEAD`) that changed `path`: its id,
/// subject, body and time (seconds since 1970).
pub fn last_change(
    dir: &Path,
    base: &str,
    to: Option<&str>,
    path: &str,
) -> Result<Option<(String, String, String, i64)>, String> {
    let range = format!("{base}..{}", to.unwrap_or("HEAD"));
    let out = git(
        dir,
        &[
            "--literal-pathspecs",
            "log",
            "-1",
            "--format=%H%x1f%s%x1f%b%x1f%ct",
            &range,
            "--",
            path,
        ],
    )?;
    let out = out.trim_end();
    if out.is_empty() {
        return Ok(None);
    }
    let f: Vec<&str> = out.split('\x1f').collect();
    if f.len() < 4 {
        return Ok(None);
    }
    Ok(Some((
        f[0].trim().to_string(),
        f[1].to_string(),
        f[2].trim().to_string(),
        f[3].trim().parse().unwrap_or(0),
    )))
}

/// The unified diff of `path` against `base`, with `context` lines: the working tree of `dir`
/// when `to` is `None`, else the commit `to`.
pub fn diff_path(
    dir: &Path,
    base: &str,
    to: Option<&str>,
    path: &str,
    context: u32,
) -> Result<String, String> {
    let u = format!("-U{context}");
    // The path as it is, never a pattern; git's own diff, no tool of the project's.
    let mut args = vec![
        "--literal-pathspecs",
        "diff",
        "--no-color",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        u.as_str(),
        base,
    ];
    if let Some(t) = to {
        args.push(t);
    }
    args.push("--");
    args.push(path);
    git(dir, &args)
}

/// The line ranges (first, last; 1-based, inclusive) of `path` added against `base`.
pub fn added_ranges(
    dir: &Path,
    base: &str,
    to: Option<&str>,
    path: &str,
) -> Result<Vec<(u32, u32)>, String> {
    let d = diff_path(dir, base, to, path, 0)?;
    let mut out = Vec::new();
    for line in d.lines().filter(|l| l.starts_with("@@")) {
        // "@@ -a,b +c,d @@": the new side is the word that starts with '+'.
        let Some(plus) = line.split_whitespace().find(|w| w.starts_with('+')) else {
            continue;
        };
        let mut it = plus[1..].split(',');
        let start: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let count: u32 = it.next().map_or(Some(1), |s| s.parse().ok()).unwrap_or(1);
        if count > 0 && start > 0 {
            out.push((start, start + count - 1));
        }
    }
    Ok(out)
}

/// The bytes of `path` at `rev`; `Ok(None)` when `rev` has no such file.
pub fn show_bytes(dir: &Path, rev: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let spec = format!("{rev}:{path}");
    let exists = crate::platform::command("git")
        .current_dir(dir)
        .args(["cat-file", "-e", &spec])
        .output()
        .is_ok_and(|o| o.status.success());
    if !exists {
        return Ok(None);
    }
    git_bytes(dir, &["show", &spec]).map(Some)
}

/// Whether `rev` names a commit (a branch, say).
pub fn rev_exists(dir: &Path, rev: &str) -> bool {
    let spec = format!("{rev}^{{commit}}");
    crate::platform::command("git")
        .current_dir(dir)
        .args(["rev-parse", "-q", "--verify", &spec])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// For each file changed after `base` up to `to` (default `HEAD`), the last commit that changed
/// it: its subject and body. One `git log` for the whole list.
pub fn last_changes(
    dir: &Path,
    base: &str,
    to: Option<&str>,
) -> Result<std::collections::BTreeMap<String, (String, String)>, String> {
    let range = format!("{base}..{}", to.unwrap_or("HEAD"));
    let out = git(
        dir,
        &[
            "log",
            "--no-renames",
            "--format=%x1e%s%x1f%b%x1f",
            "--name-only",
            &range,
        ],
    )?;
    let mut map = std::collections::BTreeMap::new();
    for rec in out.split('\x1e').filter(|r| !r.trim().is_empty()) {
        let mut f = rec.splitn(3, '\x1f');
        let subject = f.next().unwrap_or_default().trim().to_string();
        let body = f.next().unwrap_or_default().trim().to_string();
        for path in f
            .next()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            // Newest first: the first commit seen for a path is its last change.
            map.entry(path.to_string())
                .or_insert_with(|| (subject.clone(), body.clone()));
        }
    }
    Ok(map)
}

// ---- 011: bringing a loop up to date (research R-1, R-2) ---------------------------------

/// Runs git with extra environment; the exit code, stdout and stderr, whatever the code.
fn git_full(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<(i32, String, String), String> {
    let mut c = crate::platform::command("git");
    c.current_dir(dir).args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().map_err(|e| format!("git: {e}"))?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    ))
}

/// The arguments that make Fallow the author when the repository names none.
fn author_args(dir: &Path) -> Vec<&'static str> {
    let set = |k: &str| git(dir, &["config", k]).is_ok_and(|v| !v.trim().is_empty());
    if set("user.email") && set("user.name") {
        Vec::new()
    } else {
        vec![
            "-c",
            "user.name=Fallow",
            "-c",
            "user.email=fallow@localhost.invalid",
        ]
    }
}

/// The installed git's version, `(major, minor)`.
pub fn git_version(dir: &Path) -> Option<(u32, u32)> {
    let v = git(dir, &["--version"]).ok()?;
    let nums = v.split_whitespace().find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut it = nums.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    Some((major, minor))
}

/// Updates need `merge-tree --write-tree` (research R-1).
pub const MERGE_TREE_GIT: (u32, u32) = (2, 38);

/// Why an update cannot run with the git found, if it cannot.
pub fn version_refusal(found: Option<(u32, u32)>) -> Option<String> {
    match found {
        Some(v) if v >= MERGE_TREE_GIT => None,
        Some((a, b)) => Some(format!(
            "updates need git {}.{} or newer (found {a}.{b})",
            MERGE_TREE_GIT.0, MERGE_TREE_GIT.1
        )),
        None => Some("could not read git's version".into()),
    }
}

/// A path a merge could not resolve: `Text` when the merged file holds conflict markers,
/// `NonText` otherwise (modify/delete, binary, rename, file/directory: git kept one side or added
/// `<path>~<side>` files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    Text,
    NonText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub kind: ConflictKind,
}

/// A merge computed by git without a working copy (research R-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeTree {
    /// The merged tree; with conflicts, it holds the conflicted files with their markers.
    pub tree: String,
    pub clean: bool,
    pub conflicts: Vec<Conflict>,
}

/// `git merge-tree --write-tree ours theirs`: exit 0 clean, 1 conflicts, anything else an error.
/// Refused below git 2.38, naming the version found.
pub fn merge_tree(dir: &Path, ours: &str, theirs: &str) -> Result<MergeTree, String> {
    if let Some(why) = version_refusal(git_version(dir)) {
        return Err(why);
    }
    let (code, out, err) = git_full(
        dir,
        &["-c", "core.quotePath=false", "merge-tree", "--write-tree", ours, theirs],
        &[],
    )?;
    if code != 0 && code != 1 {
        return Err(if err.is_empty() { format!("git merge-tree exited {code}") } else { err });
    }
    let mut lines = out.lines();
    let tree = lines
        .next()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or("git merge-tree wrote no tree")?
        .to_string();
    if code == 0 {
        return Ok(MergeTree { tree, clean: true, conflicts: Vec::new() });
    }
    // Conflicted file info: "<mode> <object> <stage>\t<path>", up to the blank line before the
    // informational messages.
    let mut paths: Vec<String> = Vec::new();
    for l in lines {
        if l.trim().is_empty() {
            break;
        }
        if let Some((_, p)) = l.split_once('\t') {
            let p = unquote(p);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }
    let mut conflicts = Vec::new();
    for path in paths {
        // The tree is not a commit, so `show` (which checks for one) is not used here.
        let markers = git(dir, &["cat-file", "-p", &format!("{tree}:{path}")])
            .is_ok_and(|text| text.lines().any(is_marker));
        conflicts.push(Conflict {
            path,
            kind: if markers { ConflictKind::Text } else { ConflictKind::NonText },
        });
    }
    Ok(MergeTree { tree, clean: false, conflicts })
}

/// A path as git prints it, without C-style quoting for the simple cases.
fn unquote(p: &str) -> String {
    let p = p.trim();
    if p.len() >= 2 && p.starts_with('"') && p.ends_with('"') {
        p[1..p.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        p.to_string()
    }
}

/// A conflict marker line: `<<<<<<< ` or `>>>>>>> ` at its start (the separator `=======` is
/// not one: it is also a Markdown underline).
fn is_marker(line: &str) -> bool {
    line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> ")
}

/// The files that would conflict if `target_tip` were merged into `integrated`, touching no
/// working copy, index or ref; with git older than 2.38, by a trial merge in a detached scratch
/// worktree under `scratch` (removed after).
pub fn would_conflict(
    dir: &Path,
    integrated: &str,
    target_tip: &str,
    scratch: &Path,
) -> Result<Vec<String>, String> {
    if git_version(dir).is_some_and(|v| v >= MERGE_TREE_GIT) {
        return merge_tree(dir, integrated, target_tip)
            .map(|m| m.conflicts.into_iter().map(|c| c.path).collect());
    }
    // A fresh folder each time: a leftover checkout must never be merged into (review).
    let scratch = scratch.join(format!(
        "trial-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let scratch = scratch.as_path();
    // Always removed, whatever fails on the way (review).
    let cleanup = |e: String| {
        let _ = worktree_remove(dir, scratch);
        let _ = std::fs::remove_dir_all(scratch);
        e
    };
    worktree_add(dir, scratch, None, integrated).map_err(cleanup)?;
    let at = rev(scratch, "HEAD").map_err(cleanup)?;
    if at != rev(dir, integrated).map_err(cleanup)? {
        return Err(cleanup("the trial merge's worktree is not at the verified commit".into()));
    }
    let author = author_args(dir);
    let mut args: Vec<&str> = author.clone();
    args.extend(["merge", "--no-commit", "--no-ff", "-q", target_tip]);
    let merged = git(scratch, &args);
    let conflicts: Vec<String> = git(
        scratch,
        &["-c", "core.quotePath=false", "diff", "--name-only", "--diff-filter=U"],
    )
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let _ = git(scratch, &["merge", "--abort"]);
    let _ = cleanup(String::new());
    match (merged, conflicts.is_empty()) {
        (Err(e), true) => Err(e),
        _ => Ok(conflicts),
    }
}

/// `git commit-tree tree -p <parent>… -m message`: a commit made by Fallow (as the repository's
/// author, or as Fallow when none is configured); no hook runs.
pub fn commit_tree(dir: &Path, tree: &str, parents: &[&str], message: &str) -> Result<String, String> {
    let author = author_args(dir);
    let mut args: Vec<&str> = author;
    args.extend(["commit-tree", tree]);
    for p in parents {
        args.extend(["-p", p]);
    }
    args.extend(["-m", message]);
    git(dir, &args).map(|s| s.trim().to_string())
}

/// `tree` with `paths` taken from `from_commit` (a path absent there is removed), through a
/// temporary index: the loop's own version of its documents kept in a merged tree (R-2).
pub fn tree_with_paths_from(
    dir: &Path,
    tree: &str,
    from_commit: &str,
    paths: &[String],
    index_file: &Path,
) -> Result<String, String> {
    let idx = index_file.display().to_string();
    let env = [("GIT_INDEX_FILE", idx.as_str())];
    let run = |args: &[&str]| -> Result<String, String> {
        let (code, out, err) = git_full(dir, args, &env)?;
        if code == 0 { Ok(out) } else { Err(err) }
    };
    let _ = std::fs::remove_file(index_file);
    run(&["read-tree", tree])?;
    for p in paths {
        run(&["rm", "-r", "-q", "-f", "--cached", "--ignore-unmatch", "--", p])?;
        let listed = git(dir, &["ls-tree", "-r", "--name-only", from_commit, "--", p])?;
        if !listed.trim().is_empty() {
            let spec = format!("{from_commit}:{p}");
            let kind = git(dir, &["cat-file", "-t", &spec])?;
            if kind.trim() == "tree" {
                run(&["read-tree", &format!("--prefix={}/", p.trim_end_matches('/')), &spec])?;
            } else {
                let line = git(dir, &["ls-tree", from_commit, "--", p])?;
                let (meta, _) = line.trim().split_once('\t').ok_or("ls-tree gave no entry")?;
                let mut m = meta.split_whitespace();
                let (mode, _, oid) = (m.next().unwrap_or(""), m.next(), m.next().unwrap_or(""));
                run(&["update-index", "--add", "--cacheinfo", &format!("{mode},{oid},{p}")])?;
            }
        }
    }
    let out = run(&["write-tree"])?;
    let _ = std::fs::remove_file(index_file);
    Ok(out.trim().to_string())
}

/// Conflict marker lines left in `paths` at `commit`: `(path, line)`.
pub fn marker_lines(dir: &Path, commit: &str, paths: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for p in paths {
        if let Ok(Some(text)) = show(dir, commit, p) {
            for l in text.lines().filter(|l| is_marker(l)) {
                found.push((p.clone(), l.to_string()));
            }
        }
    }
    found
}

/// Files git added beside a conflicted path (`<path>~<side>`) still there at `commit`.
pub fn side_files(dir: &Path, commit: &str, paths: &[String]) -> Vec<String> {
    let all = files_at(dir, commit).unwrap_or_default();
    all.into_iter()
        .filter(|f| paths.iter().any(|p| f.starts_with(&format!("{p}~"))))
        .collect()
}

/// Creates `name` at `at`, refusing a name already taken (the caller tries `-2`, `-3`…), unlike
/// `create_branch`, which keeps an existing branch.
pub fn create_new_branch(dir: &Path, name: &str, at: &str) -> Result<(), String> {
    if rev_exists(dir, &format!("refs/heads/{name}")) {
        return Err(format!("{name} is taken"));
    }
    git(dir, &["branch", name, at]).map(|_| ())
}

/// The first of `base`, `base-2` … `base-9` not taken, created at `at`.
pub fn create_numbered_branch(dir: &Path, base: &str, at: &str) -> Result<String, String> {
    for n in 1..=9 {
        let name = if n == 1 { base.to_string() } else { format!("{base}-{n}") };
        if create_new_branch(dir, &name, at).is_ok() {
            return Ok(name);
        }
    }
    Err(format!("{base} and its numbered names up to -9 are taken"))
}

/// `git log --oneline from..to -- path`, for the repair prompt (what each side changed).
pub fn log_paths(dir: &Path, from: &str, to: &str, path: &str) -> Vec<String> {
    git(dir, &["log", "--oneline", &format!("{from}..{to}"), "--", path])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// 011: puts the worktree `dir` at `commit`, detached (an update's repair starts there).
pub fn checkout_detach(dir: &Path, commit: &str) -> Result<(), String> {
    git(dir, &["checkout", "-q", "--detach", commit]).map(|_| ())
}

/// 011: checks `branch` out in the worktree `dir` again and moves it forward to `commit` (a
/// fast-forward only).
pub fn checkout_forward(dir: &Path, branch: &str, commit: &str) -> Result<(), String> {
    git(dir, &["checkout", "-q", branch])?;
    git(dir, &["merge", "-q", "--ff-only", commit]).map(|_| ())
}

/// 011: the parents of `commit`, in order.
pub fn parents(dir: &Path, commit: &str) -> Result<Vec<String>, String> {
    let line = git(dir, &["rev-list", "--parents", "-n", "1", commit])?;
    Ok(line.split_whitespace().skip(1).map(str::to_string).collect())
}

/// 011: the first line of `commit`'s message.
pub fn subject(dir: &Path, commit: &str) -> Result<String, String> {
    git(dir, &["log", "-1", "--format=%s", commit]).map(|s| s.trim().to_string())
}

/// 011: the tree of `commit`.
pub fn tree_of(dir: &Path, commit: &str) -> Result<String, String> {
    git(dir, &["rev-parse", &format!("{commit}^{{tree}}")]).map(|s| s.trim().to_string())
}

/// The commits on `to`'s first-parent line since `from`, oldest first (FR-002).
pub fn first_parent_commits(dir: &Path, from: &str, to: &str) -> Vec<String> {
    git(dir, &["rev-list", "--reverse", "--first-parent", &format!("{from}..{to}")])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod update_tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        git(dir, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    }

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-gitupd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        run(&d, &["init", "-q", "-b", "main"]);
        run(&d, &["config", "user.email", "t@example.invalid"]);
        run(&d, &["config", "user.name", "t"]);
        run(&d, &["config", "core.autocrlf", "false"]);
        d
    }

    fn commit(d: &Path, files: &[(&str, Option<&str>)], msg: &str) -> String {
        for (p, t) in files {
            let path = d.join(p);
            match t {
                Some(t) => {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, t).unwrap();
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        run(d, &["add", "-A"]);
        run(d, &["commit", "-q", "-m", msg]);
        head(d).unwrap()
    }

    /// A base with `a.txt` and `doc.md`; branch `loop` and `main` diverge from it.
    fn diverged(name: &str, ours: &[(&str, Option<&str>)], theirs: &[(&str, Option<&str>)]) -> (PathBuf, String, String) {
        let d = repo(name);
        commit(&d, &[("a.txt", Some("one\ntwo\nthree\n")), ("doc.md", Some("Title\n=======\n"))], "base");
        run(&d, &["checkout", "-q", "-b", "loop"]);
        let l = commit(&d, ours, "ours");
        run(&d, &["checkout", "-q", "main"]);
        let t = commit(&d, theirs, "theirs");
        (d, l, t)
    }

    #[test]
    fn a_clean_merge_is_computed_and_committed_with_two_parents() {
        let (d, l, t) = diverged("clean", &[("a.txt", Some("ONE\ntwo\nthree\n"))], &[("b.txt", Some("new\n"))]);
        let m = merge_tree(&d, &l, &t).unwrap();
        assert!(m.clean && m.conflicts.is_empty());
        let c = commit_tree(&d, &m.tree, &[&l, &t], "merged").unwrap();
        assert!(is_ancestor(&d, &l, &c) && is_ancestor(&d, &t, &c));
        assert_eq!(show(&d, &c, "b.txt").unwrap().as_deref(), Some("new\n"));
        // Nothing checked out changed: still on main at theirs.
        assert_eq!(head(&d).unwrap(), t);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_text_conflict_gives_a_tree_with_markers_usable_by_commit_tree() {
        let (d, l, t) = diverged("text", &[("a.txt", Some("one\nOURS\nthree\n"))], &[("a.txt", Some("one\nTHEIRS\nthree\n"))]);
        let m = merge_tree(&d, &l, &t).unwrap();
        assert!(!m.clean);
        assert_eq!(m.conflicts, vec![Conflict { path: "a.txt".into(), kind: ConflictKind::Text }]);
        let working = commit_tree(&d, &m.tree, &[&l], "working").unwrap();
        let marks = marker_lines(&d, &working, &["a.txt".into()]);
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert!(would_conflict(&d, &l, &t, &d.join("scratch")).unwrap() == vec!["a.txt".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_modify_delete_conflict_is_non_text_and_a_markdown_underline_is_no_marker() {
        let (d, l, t) = diverged("moddel", &[("a.txt", Some("one\nOURS\nthree\n"))], &[("a.txt", None)]);
        let m = merge_tree(&d, &l, &t).unwrap();
        assert_eq!(m.conflicts, vec![Conflict { path: "a.txt".into(), kind: ConflictKind::NonText }]);
        assert!(marker_lines(&d, &l, &["doc.md".into()]).is_empty(), "======= is not a marker");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn paths_are_taken_from_another_commit_into_a_tree() {
        let (d, l, t) = diverged("paths", &[("doc.md", Some("Ours\n"))], &[("doc.md", Some("Theirs\n")), ("c.txt", Some("c\n"))]);
        let m = merge_tree(&d, &l, &t).unwrap();
        let idx = d.join(".git").join("fallow-update-index");
        let kept = tree_with_paths_from(&d, &m.tree, &l, &["doc.md".into()], &idx).unwrap();
        let c = commit_tree(&d, &kept, &[&l, &t], "kept").unwrap();
        assert_eq!(show(&d, &c, "doc.md").unwrap().as_deref(), Some("Ours\n"));
        assert_eq!(show(&d, &c, "c.txt").unwrap().as_deref(), Some("c\n"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_directory_and_a_missing_path_are_taken_from_the_other_commit() {
        let (d, l, t) = diverged(
            "dirs",
            &[("specs/f/spec.md", Some("ours spec\n")), ("specs/f/tasks.md", Some("ours tasks\n"))],
            &[("specs/f/spec.md", Some("theirs spec\n")), ("specs/f/extra.md", Some("theirs only\n")), ("gone.txt", Some("theirs\n"))],
        );
        let m = merge_tree(&d, &l, &t).unwrap();
        let idx = d.join(".git").join("fallow-update-index");
        let kept = tree_with_paths_from(&d, &m.tree, &l, &["specs/f".into(), "gone.txt".into()], &idx).unwrap();
        let c = commit_tree(&d, &kept, &[&l, &t], "kept").unwrap();
        assert_eq!(show(&d, &c, "specs/f/spec.md").unwrap().as_deref(), Some("ours spec\n"));
        assert_eq!(show(&d, &c, "specs/f/tasks.md").unwrap().as_deref(), Some("ours tasks\n"));
        assert_eq!(show(&d, &c, "specs/f/extra.md").unwrap(), None, "the directory is ours as a whole");
        assert_eq!(show(&d, &c, "gone.txt").unwrap(), None, "a path absent there is removed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_old_git_is_refused_by_name() {
        assert_eq!(version_refusal(Some((2, 45))), None);
        assert_eq!(
            version_refusal(Some((2, 37))).as_deref(),
            Some("updates need git 2.38 or newer (found 2.37)")
        );
        assert!(version_refusal(None).is_some());
    }

    #[test]
    fn numbered_branches_never_take_a_taken_name() {
        let (d, l, _) = diverged("branches", &[("a.txt", Some("x\n"))], &[("b.txt", Some("y\n"))]);
        assert_eq!(create_numbered_branch(&d, "fallow/f/update-1", &l).unwrap(), "fallow/f/update-1");
        assert_eq!(create_numbered_branch(&d, "fallow/f/update-1", &l).unwrap(), "fallow/f/update-1-2");
        assert!(create_new_branch(&d, "fallow/f/update-1", &l).is_err());
        assert!(git_version(&d).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = crate::platform::command("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-gitview-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        run(&d, &["init", "-q", "-b", "main"]);
        run(&d, &["config", "user.email", "t@example.invalid"]);
        run(&d, &["config", "user.name", "t"]);
        run(&d, &["config", "core.autocrlf", "false"]);
        std::fs::write(d.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(d.join("gone.txt"), "x\n").unwrap();
        run(&d, &["add", "."]);
        run(&d, &["commit", "-q", "-m", "base"]);
        d
    }

    #[test]
    fn a_working_copy_lists_marks_and_ranges() {
        let d = repo("wc");
        let base = head(&d).unwrap();
        std::fs::write(d.join("a.txt"), "one\nnew\ntwo\n").unwrap();
        std::fs::remove_file(d.join("gone.txt")).unwrap();
        std::fs::write(d.join("new file.txt"), "n\n").unwrap();
        std::fs::create_dir_all(d.join("build")).unwrap();
        std::fs::write(d.join("build/out"), "o").unwrap();
        let l = listed(&d).unwrap();
        assert!(l.contains(&"new file.txt".to_string()), "{l:?}");
        assert!(!l.iter().any(|p| p.starts_with("build/")), "{l:?}");
        let ns = name_status(&d, &base, None).unwrap();
        assert!(ns.contains(&('M', "a.txt".into())), "{ns:?}");
        assert!(ns.contains(&('D', "gone.txt".into())), "{ns:?}");
        assert!(ns.contains(&('A', "new file.txt".into())), "{ns:?}");
        assert_eq!(
            added_ranges(&d, &base, None, "a.txt").unwrap(),
            vec![(2, 2)]
        );
        assert!(
            diff_path(&d, &base, None, "a.txt", 3)
                .unwrap()
                .contains("+new")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_branch_reads_without_a_working_copy() {
        let d = repo("br");
        let base = head(&d).unwrap();
        run(&d, &["checkout", "-q", "-b", "f"]);
        std::fs::write(d.join("b.txt"), "b\n").unwrap();
        run(&d, &["add", "."]);
        run(
            &d,
            &[
                "commit",
                "-q",
                "-m",
                "build T002: add b",
                "-m",
                "Fallow loop r3, step 4",
            ],
        );
        run(&d, &["checkout", "-q", "main"]);
        assert!(rev_exists(&d, "f"));
        assert!(!rev_exists(&d, "nope"));
        assert_eq!(
            name_status(&d, &base, Some("f")).unwrap(),
            vec![('A', "b.txt".to_string())]
        );
        assert_eq!(show_bytes(&d, "f", "b.txt").unwrap(), Some(b"b\n".to_vec()));
        assert_eq!(show_bytes(&d, "f", "none").unwrap(), None);
        let (_, subject, body, _) = last_change(&d, &base, Some("f"), "b.txt").unwrap().unwrap();
        assert_eq!(subject, "build T002: add b");
        assert_eq!(body, "Fallow loop r3, step 4");
        assert!(
            last_change(&d, &base, Some("f"), "a.txt")
                .unwrap()
                .is_none()
        );
        let all = last_changes(&d, &base, Some("f")).unwrap();
        assert_eq!(
            all.get("b.txt"),
            Some(&(
                "build T002: add b".to_string(),
                "Fallow loop r3, step 4".to_string()
            ))
        );
        assert!(!all.contains_key("a.txt"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
