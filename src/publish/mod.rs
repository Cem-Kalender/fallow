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
