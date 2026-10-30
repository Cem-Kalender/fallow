//! Agent steps through one runtime interface (007 research S-13, contracts/runtime.md; 006
//! research R-3, R-4).
//!
//! A call is a process in the step's worktree, given its instruction on stdin. A runtime turns
//! the call's rights into its own enforcement, streams back events (what the agent says, its tool
//! calls and their results), and ends with the structured result, the cost, the session id and
//! the commands its rules refused. Fallow treats the result as a claim and verifies it elsewhere
//! (constitution I). Nothing above this interface knows which provider runs the agent.

pub mod claude;
pub mod fake;
pub mod prompts;
pub mod rights;
pub mod schemas;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use rights::Rights;

/// How to start an agent program: its command and its fixed leading arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub command: String,
    pub args: Vec<String>,
}

/// A new session with this id, an existing one to resume, or a copy of an existing one to go on
/// from without changing it (research S-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    New(String),
    Resume(String),
    Fork(String),
}

impl Session {
    /// The id given: the new id, or the session resumed or forked.
    pub fn id(&self) -> &str {
        match self {
            Session::New(s) | Session::Resume(s) | Session::Fork(s) => s,
        }
    }
}

/// Everything one call needs, whatever runs it.
#[derive(Debug, Clone)]
pub struct Call {
    pub instruction: String,
    pub schema: Option<Value>,
    pub rights: Rights,
    pub mcp_config: Option<PathBuf>,
    pub session: Session,
    pub model: Option<String>,
    pub budget_usd: Option<f64>,
    pub cwd: PathBuf,
    pub time_limit: Duration,
    pub system: Option<String>,
    /// Stream the reply as it is written (`message_delta` events), for chats.
    pub partial: bool,
}

/// What a tool call does, as the feed shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Write,
    Run,
    Search,
    Mcp,
    Other,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::Read => "read",
            ToolKind::Edit => "edit",
            ToolKind::Write => "write",
            ToolKind::Run => "run",
            ToolKind::Search => "search",
            ToolKind::Mcp => "tool",
            ToolKind::Other => "tool",
        }
    }
}

/// One thing an agent said or did, from any runtime (contracts/runtime.md). Thinking is never
/// passed up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// What the agent says, a whole message.
    Message { text: String },
    /// Part of a message as it is written, for a chat's live reply only.
    MessageDelta { text: String },
    /// A tool call. `summary` is the path, the command or the pattern; `detail` is the diff of an
    /// edit or a write, or the whole command.
    ToolCall {
        id: String,
        kind: ToolKind,
        summary: String,
        detail: String,
    },
    /// A tool call's result. `refused` when the agent's rights refused it.
    ToolResult {
        id: String,
        ok: bool,
        refused: bool,
        output: String,
    },
}

/// One line of a step's activity, for the page and the step's activity file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// Milliseconds since the step started.
    pub t: u64,
    /// `say`, `read`, `edit`, `write`, `run`, `search`, `tool`, `result`, `error`, `refused`,
    /// `progress`.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The tool call a result belongs to.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
}

/// The longest text kept for one line's `detail`: a diff or a command's output.
pub const DETAIL_MAX: usize = 64 * 1024;

/// Cuts `s` to at most `max` bytes on a character boundary, saying so.
pub fn trim_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… ({} more bytes)", &s[..end], s.len() - end)
}

/// One line of text, at most `n` characters.
pub fn clip(s: &str, n: usize) -> String {
    let one = s.replace(['\r', '\n'], " ");
    if one.chars().count() <= n {
        one
    } else {
        let mut t: String = one.chars().take(n).collect();
        t.push('…');
        t
    }
}

/// The activity line an event becomes, if any (partial messages are not kept).
pub fn activity_of(e: &Event, t: u64) -> Option<Activity> {
    let line = |kind: &str, text: String, detail: String, id: &str| Activity {
        t,
        kind: kind.into(),
        text,
        detail,
        id: id.to_string(),
    };
    match e {
        Event::Message { text } if !text.trim().is_empty() => Some(line(
            "say",
            clip(text, 300),
            if text.chars().count() > 300 {
                trim_to(text, DETAIL_MAX)
            } else {
                String::new()
            },
            "",
        )),
        Event::Message { .. } | Event::MessageDelta { .. } => None,
        Event::ToolCall {
            id,
            kind,
            summary,
            detail,
        } => Some(line(
            kind.as_str(),
            clip(summary, 300),
            trim_to(detail, DETAIL_MAX),
            id,
        )),
        Event::ToolResult {
            id,
            ok,
            refused,
            output,
        } => Some(line(
            if *refused {
                "refused"
            } else if *ok {
                "result"
            } else {
                "error"
            },
            clip(output, 200),
            if output.chars().count() > 200 {
                trim_to(output, DETAIL_MAX)
            } else {
                String::new()
            },
            id,
        )),
    }
}

/// How a call ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The structured result, when the agent gave one that parses.
    pub result: Option<Value>,
    /// Why there is no usable result.
    pub failure: Option<String>,
    pub cost_usd: Option<f64>,
    /// The session the call ran in (for a fork, the new one).
    pub session: Option<String>,
    /// The commands the agent's rights refused.
    pub denials: Vec<String>,
    pub duration: Duration,
}

/// Whether a runtime can be used, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Availability {
    pub installed: bool,
    pub version: Option<String>,
}

/// A provider that runs agent calls (FR-029).
pub trait Runtime: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> Availability;
    /// Runs `call` to its end or its time limit. `sink` gets each event as it comes; `stop` ends
    /// the call early (a pause, a stop, or Fallow stopping). A runtime that cannot enforce the
    /// call's rights refuses the call: it never runs with fewer restrictions.
    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome;
}
