//! Agent steps: one non-interactive agent call each (006 research R-3, R-4).
//!
//! A call is a process in the step's worktree, given its instruction on stdin. Its streamed
//! output becomes activity lines for the page; its last event carries the structured result,
//! the cost, the session id and the commands its permission rules refused. Fallow treats the
//! result as a claim and verifies it elsewhere (constitution I).

pub mod allow;
pub mod claude;
pub mod prompts;
pub mod schemas;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How to start a call: the agent's command and its fixed leading arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub command: String,
    pub args: Vec<String>,
}

/// A new session with this id, or an existing one to resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    New(String),
    Resume(String),
}

impl Session {
    pub fn id(&self) -> &str {
        match self {
            Session::New(s) | Session::Resume(s) => s,
        }
    }
}

/// Everything one call needs.
#[derive(Debug, Clone)]
pub struct Call {
    pub agent: Agent,
    pub instruction: String,
    pub schema: Option<Value>,
    pub allowed: Vec<String>,
    pub mcp_config: Option<PathBuf>,
    pub session: Session,
    pub model: Option<String>,
    pub budget_usd: Option<f64>,
    pub cwd: PathBuf,
    pub time_limit: Duration,
    pub system: Option<String>,
    /// Pass `--dangerously-skip-permissions` (constitution VI: per run, confirmed).
    pub unrestricted: bool,
}

/// One line of a step's activity, for the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// Milliseconds since the step started.
    pub t: u64,
    /// `say`, `read`, `edit`, `write`, `run`, `search`, `tool`, `result`, `error`, `progress`.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// How a call ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The structured result, when the agent gave one that parses.
    pub result: Option<Value>,
    /// Why there is no usable result.
    pub failure: Option<String>,
    pub cost_usd: Option<f64>,
    pub session: Option<String>,
    /// The commands the permission rules refused.
    pub denials: Vec<String>,
    pub duration: Duration,
}
