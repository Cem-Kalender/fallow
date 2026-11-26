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
pub mod instructions;
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
    /// The running total the runtime last reported for the session resumed or forked (its
    /// `Outcome.session_total_usd`), as Fallow recorded it; none for a new session, or when the
    /// runtime reports no running total. A runtime whose cost is a running total for the whole
    /// session subtracts it, so `Outcome.cost_usd` is the call's own cost.
    pub session_total_usd: Option<f64>,
    pub cwd: PathBuf,
    pub time_limit: Duration,
    pub system: Option<String>,
    /// Stream the reply as it is written (`message_delta` events), for chats.
    pub partial: bool,
    /// The process group to start the agent in. Fallow keeps it to check that an MCP bridge
    /// connecting for this call is one of the agent's own processes (006 research R-5).
    pub group: Option<std::sync::Arc<crate::platform::job::Group>>,
    /// 012: images handed with the instruction (a runtime with `Capabilities.images` only).
    pub attachments: Vec<Attachment>,
    /// 012: whether the agent's web tools are on (the assistant with web access on only).
    pub web: bool,
    /// 012 (FR-042, research R-6): the engineer's own user-level settings may apply (the
    /// assistant only). Unattended kinds run with no settings of the runtime's own; a working
    /// copy's settings never apply.
    pub user_settings: bool,
}

/// 012 (research R-8): an image stored by Fallow, handed to the agent with its instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub path: PathBuf,
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub media_type: String,
}

/// 012 (contracts/runtime.md, "Permission requests"; IX): what an agent asks permission for, in
/// the contract's terms. Each runtime's adapter maps its own tools into it; Fallow judges only
/// this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub kind: RequestKind,
    /// The command, for `Run`.
    pub command: Option<String>,
    /// The file written, for `Edit`; the file or folder read, for `Read` (none: the working
    /// directory).
    pub path: Option<String>,
    /// The page fetched, for `Web`.
    pub url: Option<String>,
    /// The search, for `Web`; the file pattern of a search, for `Read`.
    pub query: Option<String>,
    /// The runtime's own name for the tool, for the record only.
    pub tool: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Run,
    Edit,
    Web,
    /// 012 (FR-043): a file read, a search in files, or a listing of names.
    Read,
    Other,
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
    /// 007: Fallow's `ask` and `progress` tools: the question, or the line for the feed.
    Ask,
    Progress,
    /// 012: a request to the web made as a tool call: a page fetched or a search.
    Web,
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
            ToolKind::Ask => "ask",
            ToolKind::Progress => "progress",
            ToolKind::Web => "web",
            ToolKind::Other => "tool",
        }
    }
}

/// One thing an agent said or did, from any runtime (contracts/runtime.md). Thinking is never
/// passed up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The agent's program runs: sent once by every runtime, as soon as it has started it, before
    /// anything else. Fallow keeps it as the fact that the agent was given its instruction,
    /// whatever the runtime reports afterwards. Never shown.
    Launched,
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
    /// When it happened, in milliseconds since the epoch (0 in lines written before 007 US3).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub at: u64,
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

fn is_zero(v: &u64) -> bool {
    *v == 0
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

/// Whether an agent's failure says it is not signed in to its provider (2026-11-09: a loop
/// spent all its repair rounds on "Failed to authenticate: OAuth session expired and could not
/// be refreshed"). No repair can fix that; the engineer signs in again.
///
/// Only the agent program's own failure counts (review): a project's error about its own
/// logins, in a test's output, is a failure to repair.
pub fn login_failure(why: &str) -> bool {
    let Some(w) = why.strip_prefix(ENDED_WITH_ERROR) else {
        return false;
    };
    let w = w.to_lowercase();
    [
        "failed to authenticate",
        "oauth session expired",
        "oauth token has expired",
        "invalid api key",
        "please run /login",
        "authentication_error",
    ]
    .iter()
    .any(|p| w.contains(p))
}

/// How a runtime's failure starts when the agent itself ended with an error.
pub const ENDED_WITH_ERROR: &str = "the agent ended with an error: ";

/// What the engineer is told to do when [`login_failure`] holds.
pub const SIGN_IN_AGAIN: &str = "sign in again (run the agent's command, such as `claude`, in a terminal and use /login there), then press Retry";

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
        at: 0,
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
        Event::Launched | Event::Message { .. } | Event::MessageDelta { .. } => None,
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
            // The whole output when the one-line summary loses something (lines, length).
            if output.chars().count() > 200 || output.trim().contains('\n') {
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
    /// What this call alone cost, whatever the runtime reports: callers add these up.
    pub cost_usd: Option<f64>,
    /// The running total for the session the runtime reported, when its cost is one (Claude
    /// Code's `total_cost_usd` keeps growing across `--resume`). Fallow records it and hands it
    /// back as `Call.session_total_usd` when the session is resumed or forked; it is never
    /// added up. None for a runtime that reports each call's own cost.
    pub session_total_usd: Option<f64>,
    /// The session the call ran in (for a fork, the new one).
    pub session: Option<String>,
    /// The commands the agent's rights refused.
    pub denials: Vec<String>,
    pub duration: Duration,
}

/// A call's own cost from a runtime's running total for the session (`reported`) and the total
/// it reported before for the session resumed or forked (`before`; none for a new session).
/// A total that went down is a new count (the runtime started the session's count again, or
/// reports per call): it is the call's own cost.
pub fn turn_cost(reported: f64, before: Option<f64>) -> f64 {
    match before {
        Some(b) if reported >= b => reported - b,
        _ => reported,
    }
}

/// Whether a runtime can be used, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Availability {
    pub installed: bool,
    pub version: Option<String>,
}

/// 012 (FR-010a, contracts/runtime.md): what a runtime declares it can do. Fallow's behaviour
/// without a capability is the same for every runtime: attachments are refused, the web switch
/// cannot be turned off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    /// It hands `Call.attachments` (images) to the model with the instruction.
    pub images: bool,
    /// It can run a call with its web tools off (`Call.web == false`).
    pub web_off: bool,
    /// The files in a working copy through which it reads settings of its own; protected from
    /// every agent's edits.
    pub control_files: Vec<String>,
}

/// A provider that runs agent calls (FR-029).
pub trait Runtime: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> Availability;
    /// 012: what it can do, declared by every runtime (FR-010a).
    fn capabilities(&self) -> Capabilities;
    /// Runs `call` to its end or its time limit. `sink` gets each event as it comes; `stop` ends
    /// the call early (a pause, a stop, or Fallow stopping). A runtime that cannot enforce the
    /// call's rights refuses the call: it never runs with fewer restrictions.
    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome;
}

/// The runtime registry (007 US7, FR-029, contracts/runtime.md): the runtime an agent
/// definition names, built from the settings' agent profiles. Everything above it starts calls
/// through `Runtime` and never knows which provider runs them.
pub mod registry {
    use super::{Availability, Runtime};
    use crate::config::Settings;

    /// The models a definition may name, per runtime, as the page offers them.
    pub fn models(name: &str) -> &'static [&'static str] {
        match name {
            "claude" => &["opus", "sonnet", "haiku"],
            _ => &[],
        }
    }

    /// The program and leading arguments of `name`: its profile in the settings, or for
    /// `claude` the `claude` program on the PATH.
    fn profile(name: &str, settings: &Settings) -> Option<(String, Vec<String>)> {
        match settings.agents.get(name) {
            Some(p) => Some((p.command.clone(), p.args.clone())),
            None if name == "claude" => Some(("claude".into(), Vec::new())),
            None => None,
        }
    }

    /// The runtime called `name`, ready to start calls; refused, naming it, when Fallow does
    /// not know it, it is not configured, or its program is not installed. The implementation
    /// is chosen by the runtime's name; a profile's `adapter` is not consulted.
    pub fn runtime(name: &str, settings: &Settings) -> Result<Box<dyn Runtime>, String> {
        let Some((command, args)) = profile(name, settings) else {
            return Err(match name {
                "fake" => {
                    "the runtime \"fake\" is not configured: add [agents.fake] to the settings and start Fallow again"
                        .to_string()
                }
                _ => format!(
                    "Fallow knows no runtime {name:?} (it knows {})",
                    crate::defs::RUNTIMES.join(", ")
                ),
            });
        };
        if crate::platform::find_program(&command).is_none() {
            return Err(format!(
                "the runtime {name:?} is not installed: {command} was not found"
            ));
        }
        Ok(match name {
            "claude" => Box::new(super::claude::Claude::new(command, args)),
            "fake" => Box::new(super::fake::Fake::from_profile(command, args)),
            _ => {
                return Err(format!(
                    "Fallow knows no runtime {name:?} (it knows {})",
                    crate::defs::RUNTIMES.join(", ")
                ));
            }
        })
    }

    /// 012 (IX): a permission prompt from the runtime called `name`, in the contract's terms.
    /// The mapping is the runtime's own (its adapter); a runtime Fallow does not know asks for
    /// nothing it can judge (`Other`, denied).
    pub fn permission_request(name: &str, raw: &serde_json::Value) -> super::PermissionRequest {
        match name {
            "claude" | "fake" => super::claude::permission_request(raw),
            _ => super::PermissionRequest {
                kind: super::RequestKind::Other,
                command: None,
                path: None,
                url: None,
                query: None,
                tool: String::new(),
            },
        }
    }

    /// 012 (FR-010a): what the runtime called `name` can do, as its adapter declares it, also
    /// when it is not installed (review L3).
    pub fn capabilities(name: &str, settings: &Settings) -> super::Capabilities {
        match name {
            "claude" => super::claude::Claude::new("claude", Vec::new()).capabilities(),
            "fake" => profile(name, settings)
                .map(|(c, a)| super::fake::Fake::from_profile(c, a).capabilities())
                .unwrap_or_default(),
            _ => super::Capabilities::default(),
        }
    }

    /// 012: Fallow's answer to a permission prompt, in the runtime's own form.
    pub fn permission_reply(
        name: &str,
        allow: bool,
        message: &str,
        raw: &serde_json::Value,
    ) -> serde_json::Value {
        // Every runtime Fallow knows answers in Claude Code's form.
        let _ = name;
        super::claude::permission_reply(allow, message, raw)
    }

    type Listed = Vec<(String, Availability, &'static [&'static str])>;
    /// The versions last probed, for the profiles they were probed with, and when; and whether
    /// a probe is under way.
    type Versions = (String, std::time::Instant, Vec<(String, Option<String>)>);
    static VERSIONS: std::sync::Mutex<Option<Versions>> = std::sync::Mutex::new(None);
    static PROBING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// `GET /api/runtimes`: each runtime, whether it is installed, its version and its models.
    /// It answers at once: whether a runtime is installed is a lookup of its program, and its
    /// version comes from a probe on a thread of its own (each program is killed after a few
    /// seconds), started when the versions are missing or a minute old. Until the probe is
    /// back, the version is not known.
    pub fn list(settings: &Settings) -> Listed {
        let key = format!("{:?}", settings.agents);
        let known = VERSIONS.lock().ok().and_then(|v| {
            v.as_ref()
                .filter(|(k, _, _)| *k == key)
                .map(|(_, at, l)| (*at, l.clone()))
        });
        let stale = known
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(60));
        let found: Vec<(String, Option<Box<dyn Runtime>>)> = crate::defs::RUNTIMES
            .iter()
            .map(|name| (name.to_string(), runtime(name, settings).ok()))
            .collect();
        let list = found
            .iter()
            .map(|(name, r)| {
                let version = known
                    .as_ref()
                    .and_then(|(_, l)| l.iter().find(|(n, _)| n == name))
                    .and_then(|(_, v)| v.clone());
                (
                    name.clone(),
                    Availability {
                        installed: r.is_some(),
                        version: version.filter(|_| r.is_some()),
                    },
                    models(name),
                )
            })
            .collect();
        if stale && !PROBING.swap(true, std::sync::atomic::Ordering::AcqRel) {
            std::thread::spawn(move || {
                let versions = found
                    .into_iter()
                    .map(|(name, r)| (name, r.and_then(|r| r.available().version)))
                    .collect();
                if let Ok(mut v) = VERSIONS.lock() {
                    *v = Some((key, std::time::Instant::now(), versions));
                }
                PROBING.store(false, std::sync::atomic::Ordering::Release);
            });
        }
        list
    }
}

#[cfg(test)]
mod login_tests {
    use super::login_failure;

    #[test]
    fn a_login_failure_is_told_apart() {
        assert!(login_failure(
            "the agent ended with an error: Failed to authenticate: OAuth session expired and could not be refreshed"
        ));
        assert!(login_failure(
            "the agent ended with an error: Invalid API key · Please run /login"
        ));
        assert!(
            !login_failure("the agent ended without a result: test_login: invalid api key"),
            "the project's own error"
        );
        assert!(!login_failure(
            "the agent ended with an error: the user is not logged in"
        ));
        assert!(!login_failure(
            "the agent ended with an error: tests failed"
        ));
        assert!(!login_failure("the agent gave no structured result"));
    }
}

#[cfg(test)]
pub(crate) mod cost_tests {
    use super::turn_cost;

    /// The running totals Claude Code reported for one solo chat's thirteen turns
    /// (2026-11-24): the chat cost the last, not their sum ($34.28).
    pub(crate) const REPORTED: [f64; 13] = [
        0.5675, 2.6252, 2.6719, 2.7024, 2.7301, 2.7613, 2.7921, 2.8248, 2.8540, 2.8825, 2.9139,
        2.9431, 3.0137,
    ];

    #[test]
    fn a_turns_cost_is_the_difference_from_the_sessions_last_total() {
        let mut before = None;
        let mut turns = Vec::new();
        for t in REPORTED {
            turns.push(turn_cost(t, before));
            before = Some(t);
        }
        assert!((REPORTED.iter().sum::<f64>() - 34.2825).abs() < 1e-9);
        assert!((turns.iter().sum::<f64>() - 3.0137).abs() < 1e-9);
        assert!((turns[0] - 0.5675).abs() < 1e-9);
        assert!((turns[1] - 2.0577).abs() < 1e-9);
        for i in 1..13 {
            assert!((turns[i] - (REPORTED[i] - REPORTED[i - 1])).abs() < 1e-12);
        }
    }

    #[test]
    fn a_new_session_or_a_lower_total_starts_from_zero() {
        assert_eq!(turn_cost(0.4, None), 0.4, "a new session");
        assert_eq!(turn_cost(0.4, Some(3.0)), 0.4, "the count started again");
        assert_eq!(turn_cost(3.0, Some(3.0)), 0.0);
    }
}
