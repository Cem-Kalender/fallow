//! Publishing a run's verified result (FR-009, FR-024 to FR-027; 010). Only the engineer's merge
//! puts anything on the target branch (constitution II): locally, or of a pull request on the
//! remote. This module is the only place that pushes or runs a hosting service's tool.

pub mod host;
pub mod local;
pub mod remote;
pub mod run;

use serde::{Deserialize, Serialize};

/// What Fallow knows of a project's remote (010 data-model.md "Remote of a project").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    /// `none`, `no_target`, `unsupported` or `known`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// `github`, or the host's name for a service Fallow cannot publish to yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<host::Repo>,
    /// The target branch's name on the remote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `yes`, `no`, `unknown` or `checking`.
    pub protected: String,
    /// Why it is unknown (or why there is no target there), in plain words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The account the hosting tool is signed in as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// When the protection was last read; with `stale`, the last read failed and this is older.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
    /// A read of the protection is under way (the last value, if any, still shown).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checking: bool,
}

/// What a read of the protection through the hosting tool gave (on a thread).
#[derive(Debug, Clone)]
pub struct Read {
    pub account: Option<String>,
    pub protected: Result<bool, String>,
}

impl Remote {
    /// Whether two reads are of the same remote branch (a value read for one is not shown for
    /// another).
    pub fn same_place(&self, other: &Remote) -> bool {
        self.state == other.state
            && self.service == other.service
            && self.repo == other.repo
            && self.target == other.target
    }

    /// Whether the protection is known (`yes` or `no`).
    pub fn known(&self) -> bool {
        self.protected == "yes" || self.protected == "no"
    }

    /// Applies a read: a known value, or the reason it could not be read (keeping the last value,
    /// marked stale, when there was one).
    pub fn apply(&mut self, read: Read, now: String) {
        self.checking = false;
        if read.account.is_some() {
            self.account = read.account;
        }
        match read.protected {
            Ok(p) => {
                self.protected = if p { "yes" } else { "no" }.into();
                self.why = None;
                self.stale = false;
                self.checked_at = Some(now);
            }
            Err(why) => {
                if self.known() {
                    self.stale = true;
                } else {
                    self.protected = "unknown".into();
                }
                self.why = Some(why);
            }
        }
    }
}

/// Reads the protection of `remote`'s target through the hosting tool (on a thread: it may use
/// the network). Only for a remote of a supported service.
pub fn read_protection(
    dir: &std::path::Path,
    remote: &Remote,
    settings: &crate::config::Settings,
) -> Read {
    let (Some(service), Some(repo), Some(target)) = (&remote.service, &remote.repo, &remote.target)
    else {
        return Read {
            account: None,
            protected: Err("no remote to read".into()),
        };
    };
    let Some(host) = host::host_for(service, settings) else {
        return Read {
            account: None,
            protected: Err(format!("{service}: Fallow can only merge locally there")),
        };
    };
    match host.signed_in(dir) {
        Ok(account) => Read {
            account: (!account.is_empty()).then_some(account),
            protected: host.protected(dir, repo, target),
        },
        Err(why) => Read {
            account: None,
            protected: Err(why),
        },
    }
}

pub mod body;
pub mod push;

impl Remote {
    /// The target branch is not on the remote (never pushed): only a local merge (FR-007).
    pub fn target_missing(&self) -> bool {
        !self.known()
            && self
                .why
                .as_deref()
                .is_some_and(|w| w.contains(" is not on ") && w.ends_with(" yet"))
    }

    fn place(&self) -> String {
        format!(
            "{} on {}",
            self.target.as_deref().unwrap_or("?"),
            self.repo.as_ref().map(host::Repo::slug).unwrap_or_default()
        )
    }
}

/// What the loop page may offer to publish a finished loop, and why (FR-005 to FR-007): `pull_request`
/// and/or `local_merge`.
pub fn offer(remote: Option<&Remote>) -> (Vec<&'static str>, String) {
    let Some(r) = remote else {
        return (
            vec!["local_merge"],
            "the project is not on a git branch".into(),
        );
    };
    // Nothing until the remote is read: a protected target must never get a local merge.
    if r.state == "checking" || r.protected == "checking" {
        return (Vec::new(), "reading the remote first".into());
    }
    match r.state.as_str() {
        "none" => (vec!["local_merge"], "no remote: the merge is local".into()),
        "no_target" => (
            vec!["local_merge"],
            r.why
                .clone()
                .unwrap_or_else(|| "no remote for the target branch".into()),
        ),
        "unsupported" => (
            vec!["local_merge"],
            format!(
                "{}: Fallow can only merge locally there",
                r.service.as_deref().unwrap_or("this remote")
            ),
        ),
        _ if r.protected == "yes" => (
            vec!["pull_request"],
            format!(
                "{} is protected: changes reach it only through a pull request",
                r.place()
            ),
        ),
        _ if r.protected == "no" => (
            vec!["pull_request", "local_merge"],
            format!(
                "{} is not protected: a pull request first, or merge locally",
                r.place()
            ),
        ),
        _ if r.target_missing() => (vec!["local_merge"], r.why.clone().unwrap_or_default()),
        _ => (
            vec!["pull_request", "local_merge"],
            format!(
                "protection unknown{}: a local merge may not be pushable",
                r.why
                    .as_deref()
                    .map(|w| format!(" ({w})"))
                    .unwrap_or_default()
            ),
        ),
    }
}

/// Whether a loop publishes itself when it ends (FR-020): only with "open a pull request" chosen
/// at the lock, a verified outcome, a supported remote and a protection that was read. Otherwise
/// why it waits for the engineer.
pub fn when_verified(
    choice: crate::run::events::WhenVerified,
    verified: bool,
    remote: Option<&Remote>,
) -> Result<(), String> {
    if choice != crate::run::events::WhenVerified::PullRequest {
        return Err("waits for you, as chosen at the lock".into());
    }
    if !verified {
        return Err("not verified by Fallow: it waits for you".into());
    }
    let r = remote.ok_or("the project is not on a git branch")?;
    if r.state != "known" {
        let (_, why) = offer(Some(r));
        return Err(format!("not published on its own: {why}"));
    }
    if !r.known() {
        return Err(format!(
            "not published on its own: the protection of {} could not be read{}",
            r.place(),
            r.why
                .as_deref()
                .map(|w| format!(" ({w})"))
                .unwrap_or_default()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod offer_tests {
    use super::*;
    use crate::run::events::WhenVerified;

    fn remote(state: &str, protected: &str, why: Option<&str>) -> Remote {
        Remote {
            state: state.into(),
            remote: Some("origin".into()),
            service: Some(
                if state == "unsupported" {
                    "gitlab.com"
                } else {
                    "github"
                }
                .into(),
            ),
            repo: Some(host::Repo {
                owner: "acme".into(),
                name: "shop".into(),
            }),
            target: Some("main".into()),
            protected: protected.into(),
            why: why.map(str::to_string),
            account: None,
            checked_at: None,
            stale: false,
            checking: false,
        }
    }

    #[test]
    fn what_is_offered_follows_the_remote() {
        assert_eq!(offer(None).0, ["local_merge"]);
        assert!(
            offer(Some(&remote("known", "checking", None))).0.is_empty(),
            "nothing while reading"
        );
        assert_eq!(
            offer(Some(&remote("none", "unknown", None))).0,
            ["local_merge"]
        );
        let (o, why) = offer(Some(&remote("unsupported", "unknown", None)));
        assert_eq!(o, ["local_merge"]);
        assert!(why.contains("gitlab.com: Fallow can only merge locally there"));
        let (o, why) = offer(Some(&remote("known", "yes", None)));
        assert_eq!(o, ["pull_request"]);
        assert!(why.contains("main on acme/shop is protected"), "{why}");
        assert_eq!(
            offer(Some(&remote("known", "no", None))).0,
            ["pull_request", "local_merge"]
        );
        let (o, why) = offer(Some(&remote(
            "known",
            "unknown",
            Some("not signed in to github.com"),
        )));
        assert_eq!(o, ["pull_request", "local_merge"]);
        assert!(why.contains("may not be pushable"), "{why}");
        let (o, why) = offer(Some(&remote(
            "known",
            "unknown",
            Some("main is not on acme/shop yet"),
        )));
        assert_eq!(o, ["local_merge"]);
        assert!(why.contains("not on acme/shop yet"), "{why}");
    }

    #[test]
    fn a_loop_publishes_itself_only_verified_with_a_known_protection() {
        let yes = remote("known", "yes", None);
        assert!(when_verified(WhenVerified::PullRequest, true, Some(&yes)).is_ok());
        assert!(
            when_verified(
                WhenVerified::PullRequest,
                true,
                Some(&remote("known", "no", None))
            )
            .is_ok()
        );
        assert!(when_verified(WhenVerified::Wait, true, Some(&yes)).is_err());
        assert!(
            when_verified(WhenVerified::PullRequest, false, Some(&yes))
                .unwrap_err()
                .contains("not verified")
        );
        let e = when_verified(
            WhenVerified::PullRequest,
            true,
            Some(&remote("known", "unknown", Some("not signed in"))),
        )
        .unwrap_err();
        assert!(e.contains("could not be read"), "{e}");
        assert!(
            when_verified(
                WhenVerified::PullRequest,
                true,
                Some(&remote("none", "unknown", None))
            )
            .is_err()
        );
        assert!(when_verified(WhenVerified::PullRequest, true, None).is_err());
    }
}

/// 010: a publication that went through: the branch pushed and the pull request opened.
#[derive(Debug, Clone)]
pub struct Done {
    pub remote: String,
    pub service: String,
    pub repo: String,
    pub branch: String,
    pub commit: String,
    pub target: String,
    pub pr: host::PullRequest,
}

/// 010: where a publication stopped, why, and what it had pushed (FR-012).
#[derive(Debug, Clone)]
pub struct Failed {
    pub step: String,
    pub why: String,
    pub pushed: Option<(String, String)>,
}

/// What a publication needs, taken on the engine thread.
#[derive(Debug, Clone)]
pub struct Job {
    pub repo: std::path::PathBuf,
    pub remote: Remote,
    pub commit: String,
    pub branch: String,
    pub title: String,
    pub body: std::path::PathBuf,
}

/// Publishes on the calling thread (a thread of its own): the protection read again, the push,
/// the pull request; after a failed `pr create`, a pull request already opened from the branch
/// counts (FR-011).
pub fn publish(job: &Job, settings: &crate::config::Settings) -> Result<Done, Failed> {
    let fail = |step: &str, why: String, pushed: Option<(String, String)>| Failed {
        step: step.into(),
        why,
        pushed,
    };
    let r = &job.remote;
    let (Some(name), Some(service), Some(repo), Some(target)) =
        (&r.remote, &r.service, &r.repo, &r.target)
    else {
        return Err(fail("check", "no remote to publish to".into(), None));
    };
    let host = host::host_for(service, settings).ok_or_else(|| {
        fail(
            "check",
            format!("{service}: Fallow can only merge locally there"),
            None,
        )
    })?;
    // The protection read again: a target not on the remote takes no pull request. Otherwise
    // a protection that cannot be read only informs (FR-007): the pull request is tried.
    let read = read_protection(&job.repo, r, settings);
    if let Err(why) = &read.protected {
        if why.contains(" is not on ") && why.ends_with(" yet") {
            return Err(fail("check", why.clone(), None));
        }
    }
    let branch = push::push(&job.repo, name, &job.commit, &job.branch, target)
        .map_err(|why| fail("push", why, None))?;
    let pushed = Some((branch.clone(), job.commit.clone()));
    let pr = match host.open(&job.repo, repo, &branch, target, &job.title, &job.body) {
        Ok(pr) => pr,
        // Opened all the same (a time-out, an answer lost): an open one from this branch at
        // exactly this commit counts, never another (FR-011).
        Err(why) => match host.find(&job.repo, repo, &branch) {
            Ok(Some((pr, host::PrStatus::Open, Some(head)))) if head == job.commit => pr,
            _ => return Err(fail("pull_request", why, pushed)),
        },
    };
    Ok(Done {
        remote: name.clone(),
        service: service.clone(),
        repo: repo.slug(),
        branch,
        commit: job.commit.clone(),
        target: target.clone(),
        pr,
    })
}

/// 011 (research R-7, FR-011a): the commit `target` is at on `remote`, its objects brought into
/// the repository by sha: no ref of the repository is written (no remote-tracking ref, no
/// `FETCH_HEAD`). Err when the remote cannot be read or the target is not on it.
pub fn fetch_target(repo: &std::path::Path, remote: &str, target: &str) -> Result<String, String> {
    let there = run::run(
        "git",
        &[
            "ls-remote".to_string(),
            remote.to_string(),
            format!("refs/heads/{target}"),
        ],
        repo,
        run::READ,
    )?;
    let sha = there
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("{target} is not on {remote}"))?;
    if !crate::verify::git::commit_exists(repo, &sha) {
        run::run(
            "git",
            &[
                "fetch".to_string(),
                "--no-write-fetch-head".to_string(),
                "--no-tags".to_string(),
                remote.to_string(),
                sha.clone(),
            ],
            repo,
            run::WRITE,
        )?;
    }
    Ok(sha)
}
