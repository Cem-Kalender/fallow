//! GitHub, through the engineer's own `gh` (010 research R-2, R-3; contracts/host-adapter.md). It
//! never asks `gh` for a token (`auth token`, `--show-token`), and has no command that merges,
//! approves, reviews, closes or edits a pull request.

use std::path::Path;

use serde_json::Value;

use super::{Check, CheckState, Host, PrState, PrStatus, PullRequest, Repo};
use crate::publish::run::{READ, WRITE, run, run_full};

pub struct GitHub {
    program: String,
    lead: Vec<String>,
}

impl GitHub {
    pub fn new(program: String, lead: Vec<String>) -> GitHub {
        GitHub { program, lead }
    }

    fn args(&self, rest: &[&str]) -> Vec<String> {
        self.lead
            .iter()
            .cloned()
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    fn gh(&self, dir: &Path, rest: &[&str], limit: std::time::Duration) -> Result<String, String> {
        run(&self.program, &self.args(rest), dir, limit)
    }
}

impl Host for GitHub {
    fn service(&self) -> &str {
        "github"
    }

    fn signed_in(&self, dir: &Path) -> Result<String, String> {
        let (out, err) = run_full(
            &self.program,
            &self.args(&["auth", "status", "--hostname", "github.com"]),
            dir,
            READ,
        )
        .map_err(|e| {
            let l = e.to_lowercase();
            if l.contains("cannot start") {
                "GitHub's command-line tool gh is not installed: install it, run `gh auth login`, then try again".to_string()
            } else if l.contains("not logged in") || l.contains("gh auth login") || l.contains("is invalid") {
                "not signed in to github.com: run `gh auth login` in a terminal (it opens your browser), then try again".to_string()
            } else {
                e
            }
        })?;
        // gh 2.40 and later write to stdout ("account <login>"), older ones to stderr ("as
        // <login>").
        Ok(login_of(&out).or_else(|| login_of(&err)).unwrap_or_default())
    }

    /// Protected only when it can tell: an error or an answer it cannot read is never "not
    /// protected" (research R-3).
    fn protected(&self, dir: &Path, repo: &Repo, branch: &str) -> Result<bool, String> {
        let slug = repo.slug();
        // The branch first: a 404 tells a branch not pushed yet from a repository out of reach.
        let flag = match self.gh(dir, &["api", &format!("repos/{slug}/branches/{branch}")], READ) {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["protected"].as_bool())
                .ok_or_else(|| format!("could not read whether {branch} is protected on {slug}"))?,
            Err(e) if e.contains("Branch not found") => {
                return Err(format!("{branch} is not on {slug} yet"));
            }
            Err(e) if e.contains("404") => {
                return Err(format!(
                    "{slug} cannot be read with your GitHub account (it does not exist, or you have no access)"
                ));
            }
            Err(e) => return Err(e),
        };
        // A classic protection: the details need admin rights, so it counts as requiring a pull
        // request (the safe side).
        if flag {
            return Ok(true);
        }
        // Rulesets: a rule requiring a pull request, or restricting updates.
        let text = self.gh(dir, &["api", &format!("repos/{slug}/rules/branches/{branch}")], READ)?;
        let rules: Value = serde_json::from_str(&text)
            .map_err(|_| format!("could not read the rules of {branch} on {slug}"))?;
        let rules = rules
            .as_array()
            .ok_or_else(|| format!("could not read the rules of {branch} on {slug}"))?;
        Ok(rules.iter().any(|r| {
            matches!(r["type"].as_str(), Some("pull_request" | "update"))
        }))
    }

    fn open(
        &self,
        dir: &Path,
        repo: &Repo,
        head: &str,
        base: &str,
        title: &str,
        body: &Path,
    ) -> Result<PullRequest, String> {
        let body = body.display().to_string();
        let slug = repo.slug();
        let out = self.gh(
            dir,
            &[
                "pr", "create", "--repo", &slug, "--base", base, "--head", head, "--title", title,
                "--body-file", &body,
            ],
            WRITE,
        )?;
        pull_request_of(&out).ok_or_else(|| format!("gh opened no pull request: {}", out.trim()))
    }

    fn state(&self, dir: &Path, repo: &Repo, number: u64) -> Result<PrState, String> {
        let out = self.gh(
            dir,
            &[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                &repo.slug(),
                "--json",
                "state,mergedAt,url,statusCheckRollup",
            ],
            READ,
        )?;
        state_of(&out)
    }

    fn find(
        &self,
        dir: &Path,
        repo: &Repo,
        head: &str,
    ) -> Result<Option<(PullRequest, PrStatus)>, String> {
        let out = self.gh(
            dir,
            &[
                "pr",
                "list",
                "--repo",
                &repo.slug(),
                "--head",
                head,
                "--state",
                "all",
                "--json",
                "number,url,state,headRepositoryOwner",
                "--limit",
                "10",
            ],
            READ,
        )?;
        let v: Value = serde_json::from_str(&out).map_err(|e| format!("gh pr list: {e}"))?;
        // `--head` matches the branch name only: a fork's branch of the same name is not ours.
        let ours = |p: &&Value| {
            p["headRepositoryOwner"]["login"]
                .as_str()
                .is_none_or(|o| o.eq_ignore_ascii_case(&repo.owner))
        };
        Ok(v.as_array().and_then(|a| a.iter().find(ours)).and_then(|p| {
            Some((
                PullRequest {
                    number: p["number"].as_u64()?,
                    url: p["url"].as_str()?.to_string(),
                },
                status_of(p["state"].as_str()?),
            ))
        }))
    }
}

/// The login in `gh auth status`'s "Logged in to github.com account <login>" line (or "as
/// <login>" before gh 2.40).
fn login_of(out: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let rest = l
            .split("Logged in to github.com account ")
            .nth(1)
            .or_else(|| l.split("Logged in to github.com as ").nth(1))?;
        rest.split_whitespace().next().map(str::to_string)
    })
}

/// The address `gh pr create` prints, and the number at its end.
fn pull_request_of(out: &str) -> Option<PullRequest> {
    let url = out
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("https://") && l.contains("/pull/"))?;
    let number = url.rsplit('/').next()?.parse().ok()?;
    Some(PullRequest {
        number,
        url: url.to_string(),
    })
}

fn status_of(s: &str) -> PrStatus {
    match s.to_ascii_uppercase().as_str() {
        "MERGED" => PrStatus::Merged,
        "CLOSED" => PrStatus::Closed,
        _ => PrStatus::Open,
    }
}

/// `gh pr view --json state,mergedAt,url,statusCheckRollup`.
fn state_of(out: &str) -> Result<PrState, String> {
    let v: Value = serde_json::from_str(out).map_err(|e| format!("gh pr view: {e}"))?;
    let checks = v["statusCheckRollup"]
        .as_array()
        .map(|a| a.iter().filter_map(check_of).collect())
        .unwrap_or_default();
    Ok(PrState {
        state: status_of(v["state"].as_str().unwrap_or("OPEN")),
        merged_at: v["mergedAt"].as_str().map(str::to_string),
        checks,
    })
}

/// A check run (`name`, `status`, `conclusion`, `detailsUrl`) or a commit status (`context`,
/// `state`, `targetUrl`).
fn check_of(c: &Value) -> Option<Check> {
    let name = c["name"].as_str().or(c["context"].as_str())?.to_string();
    let url = c["detailsUrl"]
        .as_str()
        .or(c["targetUrl"].as_str())
        .filter(|u| !u.is_empty())
        .map(str::to_string);
    let state = if let Some(s) = c["state"].as_str() {
        match s {
            "SUCCESS" => CheckState::Passed,
            "FAILURE" | "ERROR" => CheckState::Failed,
            _ => CheckState::Waiting,
        }
    } else if c["status"].as_str().is_some_and(|s| s != "COMPLETED") {
        CheckState::Waiting
    } else {
        match c["conclusion"].as_str().unwrap_or("") {
            "SUCCESS" | "NEUTRAL" => CheckState::Passed,
            "SKIPPED" => CheckState::Skipped,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" | "STALE" => CheckState::Failed,
            _ => CheckState::Waiting,
        }
    };
    Some(Check { name, state, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_and_the_pull_request_are_read_from_gh() {
        let status = "github.com\n  ✓ Logged in to github.com account Cem-Kalender (keyring)\n  - Active account: true\n";
        assert_eq!(login_of(status).as_deref(), Some("Cem-Kalender"));
        assert_eq!(login_of("You are not logged into any GitHub hosts."), None);
        assert_eq!(
            login_of("github.com\n  ✓ Logged in to github.com as octo (oauth_token)\n").as_deref(),
            Some("octo")
        );
        let pr = pull_request_of("\nCreating pull request…\nhttps://github.com/acme/shop/pull/42\n").unwrap();
        assert_eq!((pr.number, pr.url.as_str()), (42, "https://github.com/acme/shop/pull/42"));
    }

    #[test]
    fn checks_map_to_four_states() {
        let out = r#"{"state":"OPEN","mergedAt":null,"url":"u","statusCheckRollup":[
            {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://x/1"},
            {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":""},
            {"__typename":"CheckRun","name":"slow","status":"IN_PROGRESS","conclusion":""},
            {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED"},
            {"__typename":"StatusContext","context":"ci/legacy","state":"PENDING","targetUrl":"https://x/2"}]}"#;
        let s = state_of(out).unwrap();
        assert_eq!(s.state, PrStatus::Open);
        let got: Vec<(&str, CheckState)> = s.checks.iter().map(|c| (c.name.as_str(), c.state)).collect();
        assert_eq!(
            got,
            [
                ("test", CheckState::Passed),
                ("lint", CheckState::Failed),
                ("slow", CheckState::Waiting),
                ("docs", CheckState::Skipped),
                ("ci/legacy", CheckState::Waiting),
            ]
        );
        assert_eq!(s.checks[0].url.as_deref(), Some("https://x/1"));
        assert_eq!(s.checks[1].url, None);
        let m = state_of(r#"{"state":"MERGED","mergedAt":"2026-11-02T21:00:00Z","url":"u","statusCheckRollup":[]}"#).unwrap();
        assert_eq!((m.state, m.merged_at.as_deref()), (PrStatus::Merged, Some("2026-11-02T21:00:00Z")));
    }

    #[test]
    fn the_command_lines_never_ask_for_a_token_or_a_merge() {
        let g = GitHub::new("gh".into(), vec!["--state".into(), "s".into()]);
        let a = g.args(&["auth", "status", "--hostname", "github.com"]);
        assert_eq!(a[..2], ["--state".to_string(), "s".to_string()]);
        let src = include_str!("github.rs");
        let code: String = src
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for bad in ["\"token\"", "--show-token", "\"merge\"", "\"review\"", "--approve", "\"close\"", "\"edit\""] {
            assert!(!code.contains(bad), "{bad}");
        }
    }
}
