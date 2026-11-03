//! Pushing a loop's verified commit to its own branch on the remote (010 research R-4, FR-009,
//! FR-012; constitution II): never to the target branch, never forced, never over another
//! branch's commits. The only push in Fallow.

use std::path::Path;

use super::run::{READ, WRITE, run};

/// Pushes `commit` to `branch` on `remote`, from the repository at `repo`; the branch name used
/// (another one, `-2`, `-3`, …, when `branch` holds other commits there). Refused before git runs
/// when the destination is the target branch.
pub fn push(
    repo: &Path,
    remote: &str,
    commit: &str,
    branch: &str,
    target: &str,
) -> Result<String, String> {
    for n in 1..=9 {
        let name = if n == 1 {
            branch.to_string()
        } else {
            format!("{branch}-{n}")
        };
        let refspec = refspec(commit, &name, target)?;
        let there = run(
            "git",
            &args(&["ls-remote", remote, &format!("refs/heads/{name}")]),
            repo,
            READ,
        )?;
        match there.split_whitespace().next() {
            // Already there, at exactly this commit: nothing to push.
            Some(at) if at == commit => return Ok(name),
            // Another commit on that name: never overwritten, another name.
            Some(_) => continue,
            None => {
                run("git", &args(&["push", remote, &refspec]), repo, WRITE)?;
                return Ok(name);
            }
        }
    }
    Err(format!(
        "{branch} and its numbered names up to -9 are taken on {remote} by other commits"
    ))
}

/// The branch among `branch`, `branch-2` … `branch-9` that is at `commit` on `remote`, if any
/// (what a push cut short left there, FR-012).
pub fn pushed_at(repo: &Path, remote: &str, branch: &str, commit: &str) -> Option<String> {
    (1..=9).find_map(|n| {
        let name = if n == 1 {
            branch.to_string()
        } else {
            format!("{branch}-{n}")
        };
        let there = run(
            "git",
            &args(&["ls-remote", remote, &format!("refs/heads/{name}")]),
            repo,
            READ,
        )
        .ok()?;
        (there.split_whitespace().next() == Some(commit)).then_some(name)
    })
}

/// `<commit>:refs/heads/<branch>`: one commit to one new branch, never `+` (forced), never the
/// target branch.
pub fn refspec(commit: &str, branch: &str, target: &str) -> Result<String, String> {
    let dest = branch.trim_start_matches("refs/heads/");
    if dest.eq_ignore_ascii_case(target.trim_start_matches("refs/heads/")) {
        return Err(format!(
            "refused: Fallow never pushes to the target branch {target} (constitution II)"
        ));
    }
    if commit.is_empty() || commit.starts_with('+') || dest.is_empty() || dest.starts_with('+') {
        return Err(format!(
            "refused: not a plain push of one commit ({commit} to {dest})"
        ));
    }
    Ok(format!("{commit}:refs/heads/{dest}"))
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_branch_and_forced_pushes_are_refused_before_git_runs() {
        assert!(
            refspec("abc", "main", "main")
                .unwrap_err()
                .contains("never pushes to the target")
        );
        assert!(refspec("abc", "refs/heads/Main", "main").is_err());
        assert!(refspec("+abc", "fallow/x/build", "main").is_err());
        assert!(refspec("abc", "+fallow/x/build", "main").is_err());
        assert_eq!(
            refspec("abc", "fallow/x/build", "main").unwrap(),
            "abc:refs/heads/fallow/x/build"
        );
        // Refused even with a repository that does not exist: git never runs.
        let e = push(Path::new("/no/such/repo"), "origin", "abc", "main", "main").unwrap_err();
        assert!(e.contains("never pushes to the target"), "{e}");
        let src = include_str!("push.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for bad in ["--force", "--force-with-lease", "\"-f\""] {
            assert!(!code.contains(bad), "{bad}");
        }
    }
}
