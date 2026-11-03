//! The engineer's local merge (FR-009): exactly the verified commit, onto the target branch,
//! with a merge commit. Never a push, never a force.

use std::path::Path;

use crate::verify::git;

/// Merges `commit` into `target`. When the repository's own folder has `target` checked out, the
/// merge happens there and needs a clean working tree; otherwise it happens in a temporary
/// worktree at `scratch`, which is removed afterwards. Returns the new tip of `target`.
pub fn merge(repo: &Path, target: &str, commit: &str, scratch: &Path) -> Result<String, String> {
    merge_as(repo, target, commit, scratch, false, None)
}

/// `merge`; `unchecked`: the engineer merged without the latest check (011 FR-009), and the merge
/// commit says so, never "verified" alone.
pub fn merge_as(
    repo: &Path,
    target: &str,
    commit: &str,
    scratch: &Path,
    unchecked: bool,
    not_verified: Option<&str>,
) -> Result<String, String> {
    if !git::commit_exists(repo, commit) {
        return Err(format!("commit {commit} is not in the repository"));
    }
    let short = &commit[..commit.len().min(10)];
    // Constitution I: a result Fallow did not verify is never said to be verified.
    let msg = if let Some(why) = not_verified {
        let latest = if unchecked {
            ", without the latest check"
        } else {
            ""
        };
        format!("Merge {short} (not verified by Fallow: {why}{latest}; merged by the engineer)")
    } else if unchecked {
        format!(
            "Merge {short} (verified by Fallow without the latest check: not verified on top of {target} as it was; merged by the engineer)"
        )
    } else {
        format!("Merge {short} (verified by Fallow, merged by the engineer)")
    };
    if git::branch(repo).as_deref() == Some(target) {
        if !git::tracked_changes(repo)?.is_empty() {
            return Err(format!(
                "{} has uncommitted changes on {target}; commit or stash them first",
                repo.display()
            ));
        }
        let aside = set_aside_installed(repo, commit);
        let r = git::merge_into(repo, commit, &msg).map_err(|e| e.join(", "));
        if r.is_err() {
            for (path, text) in aside {
                if !path.exists() {
                    let _ = std::fs::write(&path, text);
                }
            }
        }
        return r;
    }
    let _ = git::worktree_remove(repo, scratch);
    git::worktree_add(repo, scratch, Some(target), target)
        .map_err(|e| format!("cannot check out {target} for the merge: {e}"))?;
    let r = git::merge_into(scratch, commit, &msg).map_err(|e| e.join(", "));
    let _ = git::worktree_remove(repo, scratch);
    r
}

/// Removes, from the engineer's checkout, the untracked files the merge brings that are Fallow's
/// own unchanged Spec Kit copies (installed when the project was added, never committed): git
/// would otherwise refuse to overwrite them, though the feature's version (its constitution) is
/// the one meant to be there (T086). Returns them, to be put back if the merge fails.
fn set_aside_installed(repo: &Path, commit: &str) -> Vec<(std::path::PathBuf, &'static str)> {
    let mut aside = Vec::new();
    for path in git::added_files(repo, "HEAD", commit).unwrap_or_default() {
        let Some(text) = crate::project::install::shipped(&path) else {
            continue;
        };
        let file = repo.join(&path);
        if std::fs::read(&file).ok().as_deref() == Some(text.as_bytes())
            && std::fs::remove_file(&file).is_ok()
        {
            aside.push((file, text));
        }
    }
    aside
}
