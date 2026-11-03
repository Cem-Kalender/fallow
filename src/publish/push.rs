//! Pushing a loop's verified commit to its own branch on the remote (010 research R-4, FR-009,
//! FR-012; constitution II): never to the target branch, never forced, never over another
//! branch's commits. The only pushes in Fallow: the first publication, and 011's push forward of
//! the same branch after an update.

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

/// 011 (research R-7, FR-012): moves the loop's own `branch` on `remote` forward from `from` (the
/// commit Fallow pushed last) to `to`, which must descend from it. Done when the branch is at `to`
/// already; refused, with nothing pushed, when it is anywhere but `from` (someone else pushed) or
/// `to` does not descend from `from`. A plain push: no force, no `+`, never the target branch, and
/// never another branch name.
pub fn push_forward(
    repo: &Path,
    remote: &str,
    branch: &str,
    from: &str,
    to: &str,
    target: &str,
) -> Result<(), String> {
    let refspec = refspec(to, branch, target)?;
    let there = run(
        "git",
        &args(&["ls-remote", remote, &format!("refs/heads/{branch}")]),
        repo,
        READ,
    )?;
    match there.split_whitespace().next() {
        Some(at) if at == to => return Ok(()),
        Some(at) if at == from => {}
        Some(at) => {
            return Err(format!(
                "{branch} on {remote} is at {}, not at the commit Fallow pushed ({}): someone else pushed to it; nothing was pushed",
                short(at),
                short(from)
            ));
        }
        None => {
            return Err(format!(
                "{branch} is no longer on {remote}; nothing was pushed"
            ));
        }
    }
    if !crate::verify::git::is_ancestor(repo, from, to) {
        return Err(format!(
            "{} does not descend from {}: refused, nothing was pushed",
            short(to),
            short(from)
        ));
    }
    run("git", &args(&["push", remote, &refspec]), repo, WRITE).map(|_| ())
}

fn short(c: &str) -> &str {
    &c[..c.len().min(7)]
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

    /// 011 (FR-012): forward only, from exactly what Fallow pushed, never forced.
    #[test]
    fn a_push_forward_moves_only_from_what_fallow_pushed() {
        let base = std::env::temp_dir().join(format!("fallow-pf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let bare = base.join("remote.git");
        let work = base.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let git = |dir: &Path, a: &[&str]| {
            let o = std::process::Command::new("git")
                .current_dir(dir)
                .args(a)
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "git {a:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&base, &["init", "-q", "--bare", "remote.git"]);
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "t@example.invalid"]);
        git(&work, &["config", "user.name", "t"]);
        let commit = |name: &str| {
            std::fs::write(work.join(name), name).unwrap();
            git(&work, &["add", "."]);
            git(&work, &["commit", "-q", "-m", name]);
            git(&work, &["rev-parse", "HEAD"])
        };
        let remote = bare.display().to_string();
        let a = commit("a");
        git(
            &work,
            &["push", "-q", &remote, &format!("{a}:refs/heads/main")],
        );
        let b = commit("b");
        git(
            &work,
            &[
                "push",
                "-q",
                &remote,
                &format!("{b}:refs/heads/fallow/x/build"),
            ],
        );
        let c = commit("c");
        // The target is refused before git runs.
        assert!(push_forward(&work, &remote, "main", &a, &c, "main").is_err());
        // Forward from what Fallow pushed.
        push_forward(&work, &remote, "fallow/x/build", &b, &c, "main").unwrap();
        let at = git(&bare, &["rev-parse", "refs/heads/fallow/x/build"]);
        assert_eq!(at, c);
        // Again: already there.
        push_forward(&work, &remote, "fallow/x/build", &b, &c, "main").unwrap();
        // Not from what Fallow pushed: refused, nothing pushed.
        let d = commit("d");
        let e = push_forward(&work, &remote, "fallow/x/build", &b, &d, "main").unwrap_err();
        assert!(e.contains("someone else pushed"), "{e}");
        assert_eq!(git(&bare, &["rev-parse", "refs/heads/fallow/x/build"]), c);
        // Not descending from it: refused.
        git(&work, &["checkout", "-q", "-b", "side", &a]);
        let s = commit("s");
        let e = push_forward(&work, &remote, "fallow/x/build", &c, &s, "main").unwrap_err();
        assert!(e.contains("does not descend"), "{e}");
        assert_eq!(git(&bare, &["rev-parse", "refs/heads/fallow/x/build"]), c);
        let _ = std::fs::remove_dir_all(&base);
    }
}
