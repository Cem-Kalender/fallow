//! Finding a project's remote with git alone (010 research R-1): the remote the target branch
//! tracks, or `origin`; the hosting service from its address as configured (not as git's
//! `insteadOf` rewrites it). Protection is read through the hosting adapter, elsewhere.

use std::path::Path;

use super::Remote;
use super::host::Repo;
use super::run::{READ, run};

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run("git", &args, dir, READ)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The remote of `target` in the repository at `repo`; `protected` is `checking` for a GitHub
/// remote (the adapter reads it), `unknown` otherwise.
pub fn find(repo: &Path, target: &str) -> Remote {
    let empty = |state: &str| Remote {
        state: state.into(),
        remote: None,
        service: None,
        repo: None,
        target: None,
        protected: "unknown".into(),
        why: None,
        account: None,
        checked_at: None,
        stale: false,
        checking: false,
    };
    let remotes: Vec<String> = git(repo, &["remote"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    if remotes.is_empty() {
        return empty("none");
    }
    // The remote and branch `target` tracks; otherwise `origin`, with the same name.
    let tracked = git(repo, &["config", "--get", &format!("branch.{target}.remote")])
        .filter(|r| remotes.contains(r));
    let (name, there) = match tracked {
        Some(r) => {
            let merge = git(repo, &["config", "--get", &format!("branch.{target}.merge")])
                .map(|m| m.trim_start_matches("refs/heads/").to_string())
                .unwrap_or_else(|| target.to_string());
            (r, merge)
        }
        None if remotes.iter().any(|r| r == "origin") => ("origin".to_string(), target.to_string()),
        None => {
            let mut r = empty("no_target");
            r.why = Some(format!(
                "{target} tracks none of this repository's remotes ({}), and there is no origin",
                remotes.join(", ")
            ));
            return r;
        }
    };
    let address = git(repo, &["config", "--get", &format!("remote.{name}.url")]).unwrap_or_default();
    let mut r = empty("known");
    r.remote = Some(name);
    r.target = Some(there);
    match parse_address(&address) {
        Some((host, owner, repo_name)) if host == "github.com" => {
            r.service = Some("github".into());
            r.repo = Some(Repo {
                owner,
                name: repo_name,
            });
            r.protected = "checking".into();
        }
        Some((host, owner, repo_name)) => {
            r.state = "unsupported".into();
            r.service = Some(host);
            r.repo = Some(Repo {
                owner,
                name: repo_name,
            });
        }
        None => {
            r.state = "unsupported".into();
            r.service = Some("a local folder".into());
        }
    }
    r
}

/// The host, owner and repository of a remote's address: `https://host/owner/name(.git)`,
/// `ssh://user@host[:port]/owner/name(.git)`, `git://…`, and git's short form
/// `user@host:owner/name(.git)`. A path on disk is `None`.
pub fn parse_address(address: &str) -> Option<(String, String, String)> {
    let a = address.trim();
    let (host, path) = if let Some(rest) = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .find_map(|p| a.strip_prefix(p).or_else(|| strip_prefix_ci(a, p)))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        let host = host.split(':').next()?;
        (host.to_string(), path.to_string())
    } else if !a.contains("://") && a.contains('@') && a.contains(':') {
        // `git@github.com:acme/shop.git`; not a Windows path (`C:/…` has no `@`).
        let (user_host, path) = a.split_once(':')?;
        let host = user_host.rsplit('@').next()?;
        (host.to_string(), path.to_string())
    } else {
        return None;
    };
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if host.is_empty() {
        return None;
    }
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let name = parts[parts.len() - 1];
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner = parts[..parts.len() - 1].join("/");
    (!name.is_empty()).then(|| (host, owner, name.to_string()))
}

fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    (s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)).then(|| &s[p.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &str) -> Option<(String, String, String)> {
        parse_address(a)
    }

    fn gh(owner: &str, name: &str) -> Option<(String, String, String)> {
        Some(("github.com".into(), owner.into(), name.into()))
    }

    #[test]
    fn addresses_name_their_host_owner_and_repository() {
        assert_eq!(p("https://github.com/acme/shop.git"), gh("acme", "shop"));
        assert_eq!(p("https://github.com/acme/shop"), gh("acme", "shop"));
        assert_eq!(p("https://github.com/acme/shop/"), gh("acme", "shop"));
        assert_eq!(p("HTTPS://WWW.GitHub.com/acme/shop.git"), gh("acme", "shop"));
        assert_eq!(p("https://user@github.com/acme/shop.git"), gh("acme", "shop"));
        assert_eq!(p("git@github.com:acme/shop.git"), gh("acme", "shop"));
        assert_eq!(p("ssh://git@github.com/acme/shop.git"), gh("acme", "shop"));
        assert_eq!(p("ssh://git@github.com:22/acme/shop"), gh("acme", "shop"));
        assert_eq!(
            p("git@gitlab.com:group/sub/shop.git"),
            Some(("gitlab.com".into(), "group/sub".into(), "shop".into()))
        );
        assert_eq!(p("C:/Users/x/remote.git"), None);
        assert_eq!(p("/srv/git/shop.git"), None);
        assert_eq!(p("file:///srv/git/shop.git"), None);
        assert_eq!(p("https://github.com/acme"), None);
    }

    #[test]
    fn the_remote_is_the_tracked_one_or_origin() {
        let dir = std::env::temp_dir().join(format!("fallow-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "{args:?}");
        };
        g(&["init", "-q", "-b", "main"]);
        assert_eq!(find(&dir, "main").state, "none");
        g(&["remote", "add", "mirror", "https://example.org/x/y.git"]);
        let r = find(&dir, "main");
        assert_eq!(r.state, "no_target", "{r:?}");
        g(&["remote", "add", "origin", "https://github.com/acme/shop.git"]);
        let r = find(&dir, "main");
        assert_eq!((r.state.as_str(), r.remote.as_deref()), ("known", Some("origin")));
        assert_eq!(r.protected, "checking");
        g(&["remote", "add", "upstream", "git@gitlab.com:acme/shop.git"]);
        g(&["config", "branch.main.remote", "upstream"]);
        g(&["config", "branch.main.merge", "refs/heads/trunk"]);
        let r = find(&dir, "main");
        assert_eq!(r.remote.as_deref(), Some("upstream"));
        assert_eq!(r.target.as_deref(), Some("trunk"));
        assert_eq!((r.state.as_str(), r.service.as_deref()), ("unsupported", Some("gitlab.com")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
