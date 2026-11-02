//! Publishing a run's verified result (FR-009, FR-024 to FR-027; 010). Only the engineer's merge
//! puts anything on the target branch (constitution II): locally, or of a pull request on the
//! remote. This module is the only place that pushes or runs a hosting service's tool.

pub mod host;
pub mod local;
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
}
