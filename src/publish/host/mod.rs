//! What Fallow needs from a hosting service (010 contracts/host-adapter.md; constitution IX by
//! analogy): whether the engineer is signed in, whether a branch is protected, opening a pull
//! request and reading it. The rules that use it are Fallow's, in `publish`; what is particular to
//! one service lives in its adapter. No method merges, approves, reviews, closes or edits a pull
//! request, or changes a branch: there is no such method to call.

pub mod github;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Settings;

/// A repository on a hosting service: `owner/name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrStatus {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Waiting,
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: CheckState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrState {
    pub state: PrStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    #[serde(default)]
    pub checks: Vec<Check>,
}

pub trait Host: Send + Sync {
    /// `github`. Shown with the remote.
    fn service(&self) -> &str;
    /// The account the engineer is signed in as with the service's tool (a username, shown in the
    /// publish confirmation); Err with what to do when not signed in.
    fn signed_in(&self, dir: &Path) -> Result<String, String>;
    /// Whether changes reach `branch` of `repo` only through a pull request.
    fn protected(&self, dir: &Path, repo: &Repo, branch: &str) -> Result<bool, String>;
    /// Opens a pull request from `head` into `base`; its number and address.
    fn open(
        &self,
        dir: &Path,
        repo: &Repo,
        head: &str,
        base: &str,
        title: &str,
        body: &Path,
    ) -> Result<PullRequest, String>;
    /// The pull request's state and checks.
    fn state(&self, dir: &Path, repo: &Repo, number: u64) -> Result<PrState, String>;
    /// The latest pull request from `head` of `repo`'s own branches, in any state, with the
    /// commit it is at (after a restart, research R-8; after a failed opening, FR-011).
    fn find(
        &self,
        dir: &Path,
        repo: &Repo,
        head: &str,
    ) -> Result<Option<(PullRequest, PrStatus, Option<String>)>, String>;
}

/// The adapter of `service`, with the engineer's tool from the settings; `None` for a service
/// Fallow cannot publish to yet.
pub fn host_for(service: &str, settings: &Settings) -> Option<Box<dyn Host>> {
    let tool = settings.host_tool(service)?;
    match service {
        "github" => Some(Box::new(github::GitHub::new(tool.command, tool.args))),
        _ => None,
    }
}
