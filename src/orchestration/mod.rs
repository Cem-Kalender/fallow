//! Agent orchestration (feature 003): boards, events, deliveries, permissions, a task queue and
//! session-start requests, offered to agents in Fallow sessions as MCP tools.
//!
//! Everything lives in one in-memory `Hub`, owned by the main loop and created when the first
//! session with orchestration starts (FR-005). Nothing here is ever written to disk or logged
//! (FR-023, constitution VII).

pub mod activity;
pub mod board;
pub mod bridge;
pub mod briefing;
pub mod burst;
pub mod channel;
pub mod delegation;
pub mod delivery;
pub mod events;
pub mod evidence;
pub mod gates;
pub mod git;
pub mod identity;
pub mod inject;
pub mod lead;
pub mod manifest;
pub mod memory;
pub mod permissions;
pub mod publish;
pub mod record;
pub mod repair;
pub mod roles;
pub mod rpc;
pub mod runs;
pub mod signals;
pub mod squad;
pub mod tasks;
pub mod tools;
pub mod usage;

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use serde_json::{Value, json};

use crate::config::schema::OrchestrationSettings;
use crate::projects::ProjectId;
use activity::{Action, Activity};
use channel::{Channel, ConnId, Incoming, Outgoing};
use identity::Peer;

/// Who did something: the engineer, Fallow itself, or an agent in a session. Always set by
/// Fallow from the connection, never from tool arguments (003 FR-003, FR-007, SC-004). The
/// project, profile and role are a snapshot at the time of the action (004 FR-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    User,
    /// Fallow's own signals and posts (gates, briefings, "coordination ended"; 004).
    Fallow,
    Agent {
        session: u32,
        profile: String,
        project: String,
        /// The session's role in a coordination burst (004), fixed by Fallow.
        role: Option<String>,
    },
}

impl Party {
    /// `#3 Claude Code · api`, `#3 builder · Claude Code · api`, `you` or `Fallow`.
    pub fn label(&self) -> String {
        match self {
            Party::User => "you".into(),
            Party::Fallow => "Fallow".into(),
            Party::Agent {
                session,
                profile,
                project,
                role,
            } => match role {
                Some(r) => format!("#{session} {r} · {profile} · {project}"),
                None => format!("#{session} {profile} · {project}"),
            },
        }
    }

    /// The `from` object agents receive (research R-8; 004 adds the role).
    pub fn json(&self) -> Value {
        match self {
            Party::User => json!({ "kind": "user" }),
            Party::Fallow => json!({ "kind": "fallow" }),
            Party::Agent {
                session,
                profile,
                project,
                role,
            } => json!({
                "kind": "agent",
                "session": session,
                "role": role,
                "profile": profile,
                "project": project
            }),
        }
    }

    pub fn session(&self) -> Option<u32> {
        match self {
            Party::Agent { session, .. } => Some(*session),
            Party::User | Party::Fallow => None,
        }
    }

    pub fn role(&self) -> Option<&str> {
        match self {
            Party::Agent { role, .. } => role.as_deref(),
            _ => None,
        }
    }
}

/// "#3" or "any session of project 2".
pub fn scope_label(s: permissions::Scope) -> String {
    match s {
        permissions::Scope::Session(id) => format!("#{id}"),
        permissions::Scope::Project(p) => format!("any session of project {p}"),
    }
}

/// How a board is named in the activity log.
pub fn board_name(b: board::BoardId) -> String {
    match b {
        board::BoardId::Shared => "shared board".into(),
        board::BoardId::Project(p) => format!("project {p} board"),
    }
}

/// What the hub needs to know about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u32,
    pub project: ProjectId,
    pub project_name: String,
    pub profile_id: String,
    pub profile: String,
    /// The 002 state label ("working", "needs attention", …).
    pub state: String,
    pub running: bool,
    /// Idle at its prompt (002 `NeedsAttention`).
    pub idle: bool,
    /// The session's role in a coordination burst (004).
    pub role: Option<String>,
}

impl SessionInfo {
    pub fn party(&self) -> Party {
        Party::Agent {
            session: self.id,
            profile: self.profile.clone(),
            project: self.project_name.clone(),
            role: self.role.clone(),
        }
    }
}

/// The hub's view of the sessions (the app's `SessionManager`, or a fake in tests).
pub trait Sessions {
    fn info(&self, id: u32) -> Option<SessionInfo>;
    fn all(&self) -> Vec<SessionInfo>;
    /// Whether a connecting bridge really belongs to session `id`'s current run: its token
    /// matches and the kernel-reported peer is part of the session (research R-3).
    fn verify(&self, id: u32, token: &str, peer: Peer) -> Result<(), String>;
    /// The projects agents may name (tasks for "next free session", session starts): id, name
    /// and whether the folder is present. By default, the projects of the sessions.
    /// The profiles agents may start (id, name). By default, the profiles of the sessions.
    fn profiles(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = Vec::new();
        for s in self.all() {
            if !v.iter().any(|(id, _)| *id == s.profile_id) {
                v.push((s.profile_id, s.profile));
            }
        }
        v
    }

    /// The full profile `id` (004: a role's flags depend on the agent). `None`: unknown here,
    /// and a role then starts with no extra flags.
    fn profile(&self, id: &str) -> Option<crate::session::profiles::AgentProfile> {
        let _ = id;
        None
    }

    /// A session's folder and branch, for checking evidence (004 FR-024). `None`: unknown here.
    fn workdir(&self, id: u32) -> Option<(std::path::PathBuf, Option<String>)> {
        let _ = id;
        None
    }

    /// Whether another session may start now: 002's session limit (FR-007) and memory (FR-029).
    fn start_allowed(&self) -> Result<(), String> {
        Ok(())
    }

    fn projects(&self) -> Vec<(ProjectId, String, bool)> {
        let mut v: Vec<(ProjectId, String, bool)> = Vec::new();
        for s in self.all() {
            if !v.iter().any(|(p, ..)| *p == s.project) {
                v.push((s.project, s.project_name, true));
            }
        }
        v
    }
}

/// What a session is told through `inbox` (contracts/mcp-tools.md).
#[derive(Debug, Clone, Default)]
pub struct Inbox {
    pub deliveries: Vec<Value>,
    pub tasks: Vec<Value>,
    pub starts: Vec<Value>,
}

/// A per-session token bucket (FR-008).
#[derive(Debug, Clone)]
pub struct Rate {
    tokens: f64,
    last: Instant,
}

impl Rate {
    pub fn new(now: Instant, per_min: u64) -> Rate {
        Rate {
            tokens: per_min as f64,
            last: now,
        }
    }

    /// Takes one call; `Err(seconds until the next one)` when over the limit.
    pub fn take(&mut self, now: Instant, per_min: u64) -> Result<(), u64> {
        let cap = per_min.max(1) as f64;
        let rate = cap / 60.0;
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * rate).min(cap);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - self.tokens) / rate).ceil() as u64)
        }
    }
}

/// One session's connection state.
#[derive(Debug, Clone)]
pub struct Link {
    pub conn: Option<ConnId>,
    pub client: Option<(String, String)>,
    pub rate: Rate,
    /// When the engineer was last told this session is posting too fast.
    pub slow_notice: Option<Instant>,
}

struct Conn {
    session: u32,
    out: mpsc::Sender<Outgoing>,
}

/// The result of a tool call.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// A successful tool result (the `structuredContent`).
    Done(Value),
    /// A tool error (`isError: true`) with a code from contracts/mcp-tools.md.
    Error(&'static str, String),
    /// A protocol error (invalid arguments: `-32602`).
    Rpc(i64, String),
    /// Answered later (a wait).
    Parked,
}

impl Reply {
    pub fn error(code: &'static str, message: impl Into<String>) -> Reply {
        Reply::Error(code, message.into())
    }

    /// The JSON-RPC `result` of a `tools/call`.
    pub fn result(&self) -> Option<Value> {
        let (body, is_error) = match self {
            Reply::Done(v) => (v.clone(), false),
            Reply::Error(code, message) => (json!({ "error": code, "message": message }), true),
            Reply::Rpc(..) | Reply::Parked => return None,
        };
        Some(json!({
            "content": [{ "type": "text", "text": body.to_string() }],
            "structuredContent": body,
            "isError": is_error,
        }))
    }
}

/// All orchestration state.
pub struct Hub {
    pub settings: OrchestrationSettings,
    channel: Option<Channel>,
    conns: HashMap<ConnId, Conn>,
    pub links: HashMap<u32, Link>,
    pub activity: Activity,
    refusals: HashMap<u32, Instant>,
    /// Messages for the engineer (shown as notices by the app, then cleared).
    pub notices: Vec<String>,
    /// Decisions that are the engineer's (004 FR-036): shown before other notices.
    pub alerts: Vec<String>,
    pub boards: board::Boards,
    pub events: events::Events,
    /// The request being handled (for tools that answer later).
    ctx: CallCtx,
    /// Answers to calls made without a connection (tests drive the hub through `call`).
    pub answered: Vec<(Value, Reply)>,
    /// Sessions to wake because an event they asked for arrived while they were not waiting
    /// (turned into deliveries, US3).
    pub wakes: Vec<(u32, events::Key)>,
    pub deliveries: delivery::Deliveries,
    pub permissions: permissions::Permissions,
    /// Outcomes for each requesting session, returned by its next `inbox` call.
    pub outbox: HashMap<u32, Inbox>,
    pub tasks: tasks::Tasks,
    pub starts: Vec<StartRequest>,
    /// Coordination bursts, at most one per project (004).
    pub bursts: Vec<burst::Burst>,
    /// Read-only sessions to check now (they signalled; FR-013). The app drains it.
    pub check_worktrees: Vec<u32>,
    /// Gate sessions past their time limit, for the app to end (004 FR-022).
    pub end_gates: Vec<u32>,
    /// Failures to turn into repair tasks on the next tick: project, part, reason (FR-025).
    pub pending_repairs: Vec<(ProjectId, Option<String>, String)>,
    /// Reports to type into the sessions that created the tasks (004): session, text.
    pub pending_reports: Vec<(u32, String)>,
    /// Builders' signals to the lead (005 R-4).
    pub signals: signals::Signals,
    /// Starts waiting for memory (005 FR-038): start request id, shortfall in MB.
    pub memory_waits: Vec<(u64, u64)>,
    next_start: u64,
}

/// An agent's request to start a session (US5, FR-021).
#[derive(Debug, Clone)]
pub struct StartRequest {
    pub id: u64,
    pub from: Party,
    pub profile: String,
    pub profile_name: String,
    pub project: ProjectId,
    pub project_name: String,
    pub first_task: Option<String>,
    pub created: Instant,
    pub expires: Instant,
    pub covered_by: Option<u32>,
    pub state: StartState,
    /// 004: the new session's role in the burst, and the extra arguments it implies.
    pub role: Option<String>,
    pub args: Vec<String>,
    /// The role's command line makes the agent skip its own permission checks (FR-014): the
    /// engineer confirms the start even during a burst.
    pub bypass: bool,
    /// The part the first task is for, and its repair round (004 run 3).
    pub first_part: Option<(String, u8)>,
    /// What a builder starts for: a piece of work in the manifest and its part (005 R-2).
    pub assignment: Option<delegation::Assignment>,
    /// A reviewer's focus from the manifest's `[[review]]` (005 FR-031).
    pub focus: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartState {
    Pending,
    /// Approved; the app starts it on its next tick.
    Approved,
    Rejected,
    Expired,
    Refused(String),
    Started(u32),
}

impl StartState {
    pub fn label(&self) -> String {
        match self {
            StartState::Pending => "pending".into(),
            StartState::Approved => "approved".into(),
            StartState::Rejected => "rejected".into(),
            StartState::Expired => "expired".into(),
            StartState::Refused(why) => format!("refused: {why}"),
            StartState::Started(id) => format!("started as #{id}"),
        }
    }
}

/// Where the call being handled came from.
#[derive(Debug, Clone, Default)]
pub struct CallCtx {
    pub conn: Option<ConnId>,
    pub id: Value,
    pub progress: Option<Value>,
}

/// The state of a session whose agent shows a permission prompt (004 manual run): idle, but
/// waiting for the user, not for work.
pub const WAITING_FOR_APPROVAL: &str = "waiting for approval";

/// All text the hub stores (board messages, and later notes and task text), at most; the
/// oldest board messages go first (data-model.md "Limits", SC-007).
pub const TEXT_BUDGET: usize = 24 * 1024 * 1024;

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("channel", &self.channel)
            .field("links", &self.links.len())
            .finish_non_exhaustive()
    }
}

impl Hub {
    /// A hub with no channel (tests drive it through `call`).
    pub fn new(settings: OrchestrationSettings) -> Hub {
        Hub {
            activity: Activity::new(settings.activity_entries as usize),
            boards: board::Boards::new(settings.board_messages as usize),
            events: events::Events::default(),
            ctx: CallCtx::default(),
            answered: Vec::new(),
            wakes: Vec::new(),
            deliveries: delivery::Deliveries::default(),
            permissions: permissions::Permissions::default(),
            outbox: HashMap::new(),
            tasks: tasks::Tasks::default(),
            starts: Vec::new(),
            bursts: Vec::new(),
            check_worktrees: Vec::new(),
            end_gates: Vec::new(),
            pending_repairs: Vec::new(),
            pending_reports: Vec::new(),
            signals: Default::default(),
            memory_waits: Vec::new(),
            next_start: 0,
            settings,
            channel: None,
            conns: HashMap::new(),
            links: HashMap::new(),
            refusals: HashMap::new(),
            notices: Vec::new(),
            alerts: Vec::new(),
        }
    }

    /// The engineer writes on a board (FR-006); labelled as the user (US1 scenario 5).
    pub fn post_user(&mut self, board: board::BoardId, text: &str) -> Result<u64, String> {
        let text = board::clean_text(text, board::TEXT_MAX).map_err(|(_, m)| m)?;
        let subject = format!("{}: {text}", board_name(board));
        let id = self.boards.post(
            board,
            Party::User,
            text,
            std::time::SystemTime::now(),
            TEXT_BUDGET,
        );
        self.activity.add(Party::User, Action::Post, &subject, None);
        Ok(id)
    }

    pub fn with_channel(settings: OrchestrationSettings, channel: Channel) -> Hub {
        let mut h = Hub::new(settings);
        h.channel = Some(channel);
        h
    }

    pub fn endpoint(&self) -> Option<&str> {
        self.channel.as_ref().map(Channel::endpoint)
    }

    pub fn set_settings(&mut self, settings: OrchestrationSettings) {
        self.activity.set_cap(settings.activity_entries as usize);
        self.boards.per_board = (settings.board_messages as usize).max(1);
        self.settings = settings;
    }

    /// Whether session `id`'s bridge is connected.
    pub fn connected(&self, id: u32) -> bool {
        self.links.get(&id).is_some_and(|l| l.conn.is_some())
    }

    fn link(&mut self, id: u32, now: Instant) -> &mut Link {
        let per_min = self.settings.rate_per_min;
        self.links.entry(id).or_insert_with(|| Link {
            conn: None,
            client: None,
            rate: Rate::new(now, per_min),
            slow_notice: None,
        })
    }

    /// Handles one event from the channel. Returns true when something visible changed.
    pub fn handle(&mut self, ev: Incoming, sessions: &dyn Sessions, now: Instant) -> bool {
        match ev {
            Incoming::Hello {
                conn,
                session,
                token,
                peer,
                verdict,
                out,
            } => {
                let party = sessions.info(session).map_or(Party::User, |i| i.party());
                match sessions.verify(session, &token, peer) {
                    Ok(()) => {
                        // A new run of the same session replaces an older connection.
                        if let Some(old) = self.links.get(&session).and_then(|l| l.conn) {
                            if let Some(c) = self.conns.remove(&old) {
                                let _ = c.out.send(Outgoing::Close);
                            }
                        }
                        self.conns.insert(conn, Conn { session, out });
                        self.link(session, now).conn = Some(conn);
                        self.activity.add(
                            party,
                            Action::Connected,
                            &format!("session #{session}"),
                            None,
                        );
                        let _ = verdict.send(Ok(self.accepted(session)));
                    }
                    Err(why) => {
                        self.activity.add(
                            Party::User,
                            Action::RefusedConnection,
                            &format!("claimed session #{session}, process {}", peer.pid),
                            Some(why.clone()),
                        );
                        let recent = self
                            .refusals
                            .get(&session)
                            .is_some_and(|t| now.saturating_duration_since(*t).as_secs() < 60);
                        if !recent {
                            self.refusals.insert(session, now);
                            self.notices.push(format!(
                                "Refused a tool connection claiming session #{session}: {why}"
                            ));
                        }
                        let _ = verdict.send(Err(why));
                    }
                }
                true
            }
            Incoming::Client {
                conn,
                name,
                version,
            } => {
                if let Some(s) = self.conns.get(&conn).map(|c| c.session) {
                    self.link(s, now).client = Some((name, version));
                }
                true
            }
            Incoming::Call {
                conn,
                id,
                name,
                arguments,
                progress,
            } => {
                let Some(session) = self.conns.get(&conn).map(|c| c.session) else {
                    return false;
                };
                self.ctx = CallCtx {
                    conn: Some(conn),
                    id: id.clone(),
                    progress,
                };
                let reply = tools::call(self, sessions, session, &name, arguments, now);
                self.answer(Some(conn), &id, &reply);
                true
            }
            Incoming::Cancelled { conn, id } => {
                // The client gave up: the wait ends without a reply.
                self.events
                    .drop_where(|w| w.conn == Some(conn) && w.id == id)
                    > 0
            }
            Incoming::Closed { conn } => {
                self.events.drop_where(|w| w.conn == Some(conn));
                if let Some(c) = self.conns.remove(&conn) {
                    if let Some(l) = self.links.get_mut(&c.session) {
                        if l.conn == Some(conn) {
                            l.conn = None;
                        }
                    }
                }
                true
            }
        }
    }

    /// A session's briefing and tool list for its connection, when it has a role in a burst.
    pub fn accepted(&self, session: u32) -> channel::Accepted {
        let Some(b) = self.burst_for(session) else {
            return channel::Accepted::default();
        };
        let role = b.roles.get(&session).and_then(|r| b.squad.role(r));
        channel::Accepted {
            instructions: role.map(|r| briefing::short(b, r, session)),
            tools: role.map(|r| tools::list_for(Some(r))),
        }
    }

    /// Sends the answer to a call; calls made without a connection are kept in `answered`.
    pub(crate) fn answer(&mut self, conn: Option<ConnId>, id: &Value, reply: &Reply) {
        if matches!(reply, Reply::Parked) {
            return;
        }
        let Some(conn) = conn else {
            self.answered.push((id.clone(), reply.clone()));
            return;
        };
        let Some(c) = self.conns.get(&conn) else {
            return;
        };
        let line = match reply {
            Reply::Rpc(code, msg) => rpc::error_line(id, *code, msg),
            Reply::Parked => return,
            r => rpc::result_line(id, r.result().unwrap_or(Value::Null)),
        };
        let _ = c.out.send(Outgoing::Line(line));
    }

    /// Runs tool `name` for session `caller` (the identity of the connection, never an
    /// argument), with no connection: a call that answers later (a wait) returns `Parked`, and
    /// its answer lands in `answered` under request id `id`.
    pub fn call_as(
        &mut self,
        sessions: &dyn Sessions,
        caller: u32,
        id: u64,
        name: &str,
        args: Value,
        now: Instant,
    ) -> Reply {
        self.ctx = CallCtx {
            conn: None,
            id: Value::from(id),
            progress: None,
        };
        tools::call(self, sessions, caller, name, args, now)
    }

    /// `call_as` with request id 0.
    pub fn call(
        &mut self,
        sessions: &dyn Sessions,
        caller: u32,
        name: &str,
        args: Value,
        now: Instant,
    ) -> Reply {
        self.call_as(sessions, caller, 0, name, args, now)
    }

    pub(crate) fn ctx(&self) -> &CallCtx {
        &self.ctx
    }

    /// Signals an event as `from`: ends the waits on it and records wake candidates. Returns
    /// how many waits it ended.
    pub fn signal(
        &mut self,
        key: &events::Key,
        from: Party,
        message: Option<String>,
    ) -> Result<usize, String> {
        let sig = events::Signal {
            from: from.clone(),
            at: std::time::SystemTime::now(),
            message,
        };
        let (ended, wake) = self.events.signal(key, sig.clone())?;
        let n = ended.len();
        for w in ended {
            let reply = Reply::Done(tools::signalled(&sig));
            self.answer(w.conn, &w.id, &reply);
        }
        for s in wake {
            self.wakes.push((s, key.clone()));
        }
        self.activity.add(
            from,
            Action::Signal,
            &format!("{} ({})", key.1, board_name(key.0)),
            Some(format!("{n} waiting")),
        );
        Ok(n)
    }

    /// Pending session-start requests.
    pub fn pending_starts(&self) -> impl Iterator<Item = &StartRequest> {
        self.starts
            .iter()
            .filter(|s| s.state == StartState::Pending)
    }

    /// An agent asks to start a session (FR-021). Refused at once beyond the limits; approved
    /// at once under a `StartSession` permission with room left; otherwise it waits for the
    /// engineer.
    #[allow(clippy::too_many_arguments)]
    pub fn request_start(
        &mut self,
        sessions: &dyn Sessions,
        from: &SessionInfo,
        profile: &str,
        project: &str,
        first_task: Option<String>,
        role: Option<&str>,
        assignment: Option<delegation::Assignment>,
        now: Instant,
    ) -> Result<(u64, StartState, Option<String>), Reply> {
        let Some((pid, pname)) = sessions
            .profiles()
            .into_iter()
            .find(|(id, _)| id == profile)
        else {
            return Err(Reply::error("invalid", format!("no profile {profile:?}")));
        };
        let Some((proj, proj_name, present)) = sessions
            .projects()
            .into_iter()
            .find(|(_, n, _)| n.eq_ignore_ascii_case(project))
        else {
            return Err(Reply::error(
                "invalid",
                format!("no project named {project:?}"),
            ));
        };
        if !present {
            return Err(Reply::error(
                "invalid",
                format!("project {proj_name} is missing"),
            ));
        }
        let (role, args, bypass, warn, hold) =
            self.start_role(sessions, from, &pid, proj, role, assignment.as_ref())?;
        if let Some(w) = warn {
            self.notices.push(w);
        }
        self.next_start += 1;
        let id = self.next_start;
        let state = match sessions.start_allowed() {
            Err(why) => StartState::Refused(why),
            // Beyond what the burst may start on its own: the user decides (004 run 3).
            Ok(()) if hold.is_some() => {
                self.alerts.push(format!(
                    "{} asks to start a {} session in {proj_name}: {}{}. o to approve or reject",
                    from.party().label(),
                    role.as_deref().unwrap_or("?"),
                    hold.as_deref().unwrap_or_default(),
                    if bypass {
                        " (its flags skip the agent's permission checks)"
                    } else {
                        ""
                    }
                ));
                StartState::Pending
            }
            Ok(()) if bypass => {
                self.notices.push(format!(
                    "Role {} starts {pname} with flags that skip its permission checks: o to confirm",
                    role.as_deref().unwrap_or("?")
                ));
                StartState::Pending
            }
            Ok(()) => {
                let target = SessionInfo {
                    id: 0,
                    project: proj,
                    project_name: proj_name.clone(),
                    profile_id: pid.clone(),
                    profile: pname.clone(),
                    state: String::new(),
                    running: false,
                    idle: false,
                    role: None,
                };
                match self
                    .permissions
                    .cover(from, &target, permissions::Act::StartSession, now)
                {
                    Some(p) => {
                        self.notices.push(format!(
                            "P{p} used: {} starts {pname} in {proj_name}",
                            from.party().label()
                        ));
                        StartState::Approved
                    }
                    None => {
                        self.notices.push(format!(
                            "{} asks to start {pname} in {proj_name} (o to review)",
                            from.party().label()
                        ));
                        StartState::Pending
                    }
                }
            }
        };
        self.activity.add(
            from.party(),
            Action::StartRequested,
            &format!("{pname} in {proj_name}"),
            Some(state.label()),
        );
        self.starts.push(StartRequest {
            id,
            from: from.party(),
            profile: pid,
            profile_name: pname,
            project: proj,
            project_name: proj_name,
            first_task,
            created: now,
            expires: now + std::time::Duration::from_secs(self.settings.request_timeout_s),
            covered_by: None,
            state: state.clone(),
            role,
            args,
            bypass,
            first_part: None,
            assignment,
            focus: None,
        });
        if matches!(state, StartState::Refused(_)) {
            self.start_outcome(id);
        }
        Ok((id, state, hold))
    }

    fn start_outcome(&mut self, id: u64) {
        let Some(s) = self.starts.iter().find(|s| s.id == id).cloned() else {
            return;
        };
        if let Some(session) = s.from.session() {
            let mut v = serde_json::json!({ "start": id, "state": s.state.label() });
            if let StartState::Started(sid) = s.state {
                v["session"] = Value::from(sid);
            }
            self.outbox.entry(session).or_default().starts.push(v);
        }
        if !matches!(s.state, StartState::Refused(_)) {
            self.activity.add(
                s.from.clone(),
                Action::StartOutcome,
                &format!("{} in {}", s.profile_name, s.project_name),
                Some(s.state.label()),
            );
        }
    }

    /// The engineer approves or rejects a start request.
    pub fn decide_start(&mut self, id: u64, approve: bool) {
        if let Some(s) = self.starts.iter_mut().find(|s| s.id == id) {
            if s.state == StartState::Pending {
                s.state = if approve {
                    StartState::Approved
                } else {
                    StartState::Rejected
                };
            }
        }
        if !approve {
            self.start_outcome(id);
        }
    }

    /// Approved starts for the app to carry out.
    pub fn approved_starts(&self) -> Vec<StartRequest> {
        self.starts
            .iter()
            .filter(|s| s.state == StartState::Approved)
            .cloned()
            .collect()
    }

    /// The app started (or could not start) an approved request. A started session gets the
    /// request's first task, queued for it and already approved to be handed over.
    pub fn start_done(
        &mut self,
        sessions: &dyn Sessions,
        id: u64,
        result: Result<u32, String>,
        now: Instant,
    ) {
        let Some(s) = self.starts.iter_mut().find(|s| s.id == id) else {
            return;
        };
        s.state = match &result {
            Ok(sid) => StartState::Started(*sid),
            Err(e) => StartState::Refused(e.clone()),
        };
        let (from, task, part) = (s.from.clone(), s.first_task.clone(), s.first_part.clone());
        let (role, project, assignment) = (s.role.clone(), s.project, s.assignment.clone());
        let focus = s.focus.clone();
        if let (Ok(sid), Some(r)) = (&result, role) {
            if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
                if let Some(a) = &assignment {
                    b.assigned.insert(*sid, a.clone());
                }
                if let Some(f) = &focus {
                    b.focus.insert(*sid, f.clone());
                }
                let read_only = b.squad.role(&r).is_some_and(|r| !r.level.edits());
                b.roles.insert(*sid, r);
                // The first check records the worktree as it starts (FR-013).
                if read_only {
                    self.check_worktrees.push(*sid);
                }
            }
        }
        if let (Ok(sid), Some(text)) = (result, task) {
            if let Ok((t, _)) = self.create_task(
                sessions,
                Party::User,
                text,
                tasks::Target::Session(sid),
                Vec::new(),
                now,
            ) {
                if let Some(task) = self.tasks.get_mut(t) {
                    task.created_by = from;
                    task.approved_handover = true;
                    if let Some((w, p)) = &assignment {
                        task.work = Some(w.clone());
                        task.part.clone_from(p);
                    }
                    if let Some((p, round)) = part {
                        task.part = Some(p);
                        task.repair_round = round;
                    }
                }
            }
        }
        self.start_outcome(id);
    }

    /// Queued (including blocked) and blocked tasks, for the counts line.
    pub fn queue_counts(&self) -> (usize, usize) {
        let open =
            |t: &&tasks::Task| matches!(t.state, tasks::State::Queued | tasks::State::Blocked);
        let queued = self.tasks.list.iter().filter(open).count();
        let blocked = self
            .tasks
            .list
            .iter()
            .filter(|t| t.state == tasks::State::Blocked)
            .count();
        (queued, blocked)
    }

    /// Requests waiting for the engineer: tasks agents created.
    pub fn pending_tasks(&self) -> impl Iterator<Item = &tasks::Task> {
        self.tasks
            .list
            .iter()
            .filter(|t| t.state == tasks::State::PendingApproval)
    }

    fn tell_task(&mut self, session: Option<u32>, t: &tasks::Task) {
        if let Some(s) = session {
            let mut v = serde_json::json!({ "task": t.id, "state": t.state.label() });
            if let Some(n) = &t.note {
                v["note"] = Value::from(n.clone());
            }
            self.outbox.entry(s).or_default().tasks.push(v);
        }
    }

    fn task_changed(&mut self, id: u32, what: &str) {
        let Some(t) = self.tasks.get(id).cloned() else {
            return;
        };
        self.activity.add(
            t.created_by.clone(),
            Action::TaskChanged,
            &format!("T{id} {}: {}", t.target.label(), t.text),
            Some(what.to_string()),
        );
    }

    /// Creates a task (FR-018). The engineer's tasks are queued at once; an agent's need the
    /// engineer's approval unless a `CreateTask` permission covers them (US4 scenario 5).
    pub fn create_task(
        &mut self,
        sessions: &dyn Sessions,
        from: Party,
        text: String,
        target: tasks::Target,
        preconditions: Vec<events::Key>,
        now: Instant,
    ) -> Result<(u32, tasks::State), String> {
        let target_info = match &target {
            tasks::Target::Session(id) => sessions
                .info(*id)
                .filter(|s| s.running)
                .ok_or_else(|| format!("session #{id} is not running"))?,
            tasks::Target::NextFree {
                profile,
                project,
                project_name,
            } => SessionInfo {
                id: 0,
                project: *project,
                project_name: project_name.clone(),
                profile_id: profile.clone(),
                profile: profile.clone(),
                state: String::new(),
                running: false,
                idle: false,
                role: None,
            },
        };
        let approved = match from.session().and_then(|s| sessions.info(s)) {
            None => true,
            Some(src) => {
                let p =
                    self.permissions
                        .cover(&src, &target_info, permissions::Act::CreateTask, now);
                if let Some(p) = p {
                    self.notices
                        .push(format!("P{p} used: {} created a task", from.label()));
                }
                p.is_some()
            }
        };
        let id = self
            .tasks
            .add(text, target, preconditions, from.clone(), approved, now)?;
        if !approved {
            self.notices.push(format!(
                "{} asks to queue a task (o to review)",
                from.label()
            ));
        }
        self.task_changed(
            id,
            if approved {
                "queued"
            } else {
                "waiting for approval"
            },
        );
        let state = self
            .tasks
            .get(id)
            .map_or(tasks::State::Queued, |t| t.state.clone());
        Ok((id, state))
    }

    /// The engineer approves or rejects an agent's task.
    pub fn decide_task(&mut self, id: u32, approve: bool) {
        let creator = self.tasks.get(id).and_then(|t| t.created_by.session());
        match self.tasks.get_mut(id) {
            Some(t) if t.state == tasks::State::PendingApproval => {
                if approve {
                    t.state = tasks::State::Queued;
                } else {
                    t.state = tasks::State::Cancelled("rejected".into());
                    t.finished = Some(std::time::SystemTime::now());
                }
            }
            _ => return,
        }
        self.task_changed(id, if approve { "approved" } else { "rejected" });
        if let Some(t) = self.tasks.get(id).cloned() {
            self.tell_task(creator, &t);
        }
    }

    /// The engineer pauses or resumes a task (paused tasks are skipped by the queue).
    pub fn toggle_pause(&mut self, id: u32) {
        if let Some(t) = self.tasks.get_mut(id) {
            if !t.state.finished() {
                t.paused = !t.paused;
                let what = if t.paused { "paused" } else { "resumed" };
                self.task_changed(id, what);
            }
        }
    }

    /// The engineer cancels a task: it is never handed over (US4 scenario 6). A hand-over still
    /// waiting for approval is rejected with it.
    pub fn cancel_task(&mut self, id: u32) {
        let Some(t) = self.tasks.get(id).cloned() else {
            return;
        };
        if t.state.finished() {
            return;
        }
        if let Some(d) = t.delivery {
            if self
                .deliveries
                .finish(d, delivery::State::Rejected)
                .is_some()
            {
                self.activity.add(
                    Party::User,
                    Action::DeliveryOutcome,
                    &format!("T{id} hand-over"),
                    Some("cancelled with the task".into()),
                );
            }
        }
        self.tasks
            .finish(id, tasks::State::Cancelled("by the user".into()));
        self.task_changed(id, "cancelled");
        if let Some(t) = self.tasks.get(id).cloned() {
            self.tell_task(t.created_by.session(), &t);
        }
    }

    /// The engineer edits a task's text (before it is handed over).
    pub fn edit_task(&mut self, id: u32, text: String) -> Result<(), String> {
        let t = self.tasks.get_mut(id).ok_or("no such task")?;
        if !matches!(
            t.state,
            tasks::State::Queued | tasks::State::Blocked | tasks::State::PendingApproval
        ) {
            return Err(format!("T{id} is {}", t.state.label()));
        }
        t.text = text;
        self.task_changed(id, "edited");
        Ok(())
    }

    /// The agent that created a task withdraws it (004 manual run: a task that reached the wrong
    /// session). Any open task it created; a hand-over still waiting is rejected with it. The
    /// lead of a burst also withdraws Fallow's tasks of the burst and any task of it that failed
    /// or waits for a session that exited; the engineer's open tasks stay theirs (FR-042).
    pub fn withdraw_task(
        &mut self,
        caller: u32,
        id: u32,
        note: Option<String>,
    ) -> Result<tasks::State, Reply> {
        let t = self
            .tasks
            .get(id)
            .ok_or_else(|| Reply::error("not_found", format!("no task T{id}")))?;
        let lead = self.bursts.iter().any(|b| {
            let of_burst = t.created_at >= b.started
                && match &t.target {
                    tasks::Target::Session(s) => b.roles.contains_key(s),
                    tasks::Target::NextFree { project, .. } => *project == b.project,
                };
            let left = matches!(t.state, tasks::State::Failed(_))
                || matches!(t.target, tasks::Target::Session(s) if b.exited.contains(&s));
            b.lead == caller && of_burst && (t.created_by == Party::Fallow || left)
        });
        if t.created_by.session() != Some(caller) && !lead {
            return Err(Reply::error(
                "not_allowed",
                format!(
                    "T{id} was not created by your session; only its creator withdraws it (a lead also withdraws Fallow's tasks and ones that failed or wait for a session that exited)"
                ),
            ));
        }
        let lead = lead && t.created_by.session() != Some(caller);
        // A session working on it is told, instead of reporting it later in vain (review).
        let working = t
            .assigned
            .filter(|a| t.state == tasks::State::InProgress && *a != caller);
        // A failed task can be withdrawn once its work is handed again.
        if matches!(t.state, tasks::State::Done | tasks::State::Cancelled(_)) {
            return Err(Reply::error(
                "invalid",
                format!("T{id} is already {}", t.state.label()),
            ));
        }
        if let Some(d) = t.delivery {
            if self
                .deliveries
                .finish(d, delivery::State::Rejected)
                .is_some()
            {
                self.delivery_outcome(d);
            }
        }
        let by = if lead {
            "by the lead"
        } else {
            "by its creator"
        };
        let why = note.unwrap_or_else(|| format!("withdrawn {by}"));
        match self.tasks.get_mut(id) {
            // `finish` keeps a finished state: a failed task is withdrawn here.
            Some(t) if t.state.finished() => t.state = tasks::State::Cancelled(why),
            _ => self.tasks.finish(id, tasks::State::Cancelled(why)),
        }
        self.task_changed(id, &format!("withdrawn {by}"));
        if let (Some(w), Some(t)) = (working, self.tasks.get(id).cloned()) {
            self.tell_task(Some(w), &t);
        }
        Ok(self
            .tasks
            .get(id)
            .map_or(tasks::State::Done, |t| t.state.clone()))
    }

    /// The assigned agent reports a task done or failed (FR-020).
    pub fn report_task(
        &mut self,
        caller: u32,
        id: u32,
        done: bool,
        note: Option<String>,
        evidence: Option<evidence::Evidence>,
    ) -> Result<tasks::State, Reply> {
        let t = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| Reply::error("not_found", format!("no task T{id}")))?;
        if t.assigned != Some(caller) || t.state != tasks::State::InProgress {
            return Err(Reply::error(
                "not_allowed",
                format!("T{id} is not in progress in your session"),
            ));
        }
        t.note = note.clone();
        if evidence.is_some() {
            t.evidence = evidence;
        }
        let state = if done {
            tasks::State::Done
        } else {
            tasks::State::Failed(note.unwrap_or_default())
        };
        self.tasks.finish(id, state.clone());
        self.task_changed(id, if done { "done" } else { "failed" });
        if let Some(t) = self.tasks.get(id).cloned() {
            self.tell_task(t.created_by.session(), &t);
            // During a burst the lead hears every report, also of tasks Fallow created, and so
            // does a session that created the task (004 US7, FR-032).
            if let Some(b) = self.burst_for(caller) {
                let (project, lead) = (b.project, b.lead);
                let who = self
                    .role_of(caller)
                    .map_or(format!("#{caller}"), |r| format!("#{caller} {r}"));
                let mut text = format!(
                    "Task T{id}{} was reported {} by {who}.",
                    t.part
                        .as_ref()
                        .map_or(String::new(), |p| format!(" (part {p})")),
                    if done { "done" } else { "failed" }
                );
                if let Some(e) = &t.evidence {
                    text.push_str(&format!(" Evidence: {}.", e.summary()));
                }
                if let Some(n) = &t.note {
                    text.push_str(&format!(" Note: {n}"));
                }
                if caller != lead {
                    self.tell_lead(project, text.clone());
                }
                // A session that created the task and is not the lead hears it too.
                if let Some(creator) = t.created_by.session() {
                    if creator != caller && creator != lead {
                        self.pending_reports.push((creator, text));
                    }
                }
            }
        }
        Ok(state)
    }

    /// A reviewer's verdict completes its open review tasks (004 run 2: a review was reported
    /// done while it asked for changes, and the re-review was not tracked).
    pub(crate) fn finish_review_tasks(&mut self, caller: u32, approve: bool, part: Option<&str>) {
        let mine =
            |t: &&tasks::Task| t.assigned == Some(caller) && t.state == tasks::State::InProgress;
        // A verdict for a part completes that part's review task, or one without a part.
        let mut open: Vec<u32> = self
            .tasks
            .list
            .iter()
            .filter(mine)
            .filter(|t| part.is_none() || t.part.as_deref() == part)
            .map(|t| t.id)
            .collect();
        if open.is_empty() && part.is_some() {
            open = self
                .tasks
                .list
                .iter()
                .filter(mine)
                .filter(|t| t.part.is_none())
                .map(|t| t.id)
                .collect();
        }
        for id in open {
            if let Some(t) = self.tasks.get_mut(id) {
                t.note = Some(if approve {
                    "verdict: approved".into()
                } else {
                    "verdict: changes requested".into()
                });
            }
            self.tasks.finish(id, tasks::State::Done);
            self.task_changed(id, "done (verdict given)");
            if let Some(t) = self.tasks.get(id).cloned() {
                self.tell_task(t.created_by.session(), &t);
            }
        }
    }

    /// A hand-over delivery ended: delivered keeps the task in progress; rejected or expired
    /// pauses it (so it is not offered again at once); a target that exited puts a "next free"
    /// task back in the queue and fails a task for that session.
    fn task_handover_ended(&mut self, id: u32, state: &delivery::State) {
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        if t.state.finished() {
            return;
        }
        let what = match state {
            delivery::State::Delivered => {
                t.delivery = None;
                return;
            }
            delivery::State::Rejected | delivery::State::Expired => {
                t.state = tasks::State::Queued;
                t.paused = true;
                t.assigned = None;
                t.delivery = None;
                t.note = Some(format!("hand-over {}; Space resumes", state.label()));
                "paused (hand-over not approved)"
            }
            delivery::State::Failed(_) => {
                if matches!(t.target, tasks::Target::NextFree { .. }) {
                    t.state = tasks::State::Queued;
                    t.assigned = None;
                    t.delivery = None;
                    "back in the queue (target exited)"
                } else {
                    t.state = tasks::State::Failed("target exited".into());
                    t.delivery = None;
                    "failed (target exited)"
                }
            }
            _ => return,
        };
        self.task_changed(id, what);
    }

    /// Hands over every task whose target is free and whose preconditions are met, in queue
    /// order, one task per session at a time (FR-019).
    pub fn advance(&mut self, sessions: &dyn Sessions, now: Instant) -> bool {
        let mut changed = false;
        let mut busy: Vec<u32> = self
            .tasks
            .list
            .iter()
            .filter(|t| t.state == tasks::State::InProgress && !Hub::lead_work(t))
            .filter_map(|t| t.assigned)
            .collect();
        let all = sessions.all();
        let ids: Vec<u32> = self.tasks.list.iter().map(|t| t.id).collect();
        for id in ids {
            let Some(t) = self.tasks.get(id).cloned() else {
                continue;
            };
            if t.paused || !matches!(t.state, tasks::State::Queued | tasks::State::Blocked) {
                continue;
            }
            let unmet = t.preconditions.iter().any(|k| !self.condition_met(&t, k));
            let want = if unmet {
                tasks::State::Blocked
            } else {
                tasks::State::Queued
            };
            if t.state != want {
                if let Some(m) = self.tasks.get_mut(id) {
                    m.state = want.clone();
                }
                changed = true;
            }
            if unmet {
                continue;
            }
            let free = |s: &SessionInfo| {
                s.running
                    && s.idle
                    && s.state != WAITING_FOR_APPROVAL
                    && !busy.contains(&s.id)
                    && self.events.waiting(s.id, now).is_none()
            };
            let pick = match &t.target {
                tasks::Target::Session(sid) => all.iter().find(|s| s.id == *sid && free(s)),
                tasks::Target::NextFree {
                    profile, project, ..
                } => all
                    .iter()
                    // The next free session is another one: an agent asking for it never gets
                    // its own task back (004 manual run).
                    .find(|s| {
                        &s.profile_id == profile
                            && s.project == *project
                            && Some(s.id) != t.created_by.session()
                            && free(s)
                    }),
            }
            .map(|s| s.id);
            let Some(to) = pick else {
                continue;
            };
            // Builders begin after the baseline gate ran (004 FR-021).
            if self.held_for_baseline(to) {
                continue;
            }
            let by_user = t.created_by == Party::User || t.approved_handover;
            // The next role receives the evidence of the part's done tasks (004 FR-023).
            let mut text = t.text.clone();
            if let Some(part) = &t.part {
                for d in self.tasks.list.iter().filter(|d| {
                    d.id != t.id && d.part.as_ref() == Some(part) && d.state == tasks::State::Done
                }) {
                    if let Some(e) = &d.evidence {
                        text.push_str(&format!(
                            "\n\nEvidence from T{} ({}): {}",
                            d.id,
                            d.assigned.map_or("?".into(), |a| format!("#{a}")),
                            e.summary()
                        ));
                    }
                }
            }
            if let Ok((did, _)) = self.request_delivery(
                sessions,
                t.created_by.clone(),
                to,
                text,
                delivery::Purpose::Task(id),
                by_user,
                now,
            ) {
                busy.push(to);
                if let Some(m) = self.tasks.get_mut(id) {
                    m.state = tasks::State::InProgress;
                    m.assigned = Some(to);
                    m.delivery = Some(did);
                    m.started = Some(std::time::SystemTime::now());
                }
                self.task_changed(id, &format!("handed to #{to}"));
                if let Some(t) = self.tasks.get(id).cloned() {
                    let mut v = serde_json::json!({ "task": id, "state": "in progress" });
                    v["text"] = tools::labelled(&t.created_by, t.created, &t.text);
                    self.outbox.entry(to).or_default().tasks.push(v);
                }
                changed = true;
            }
        }
        changed
    }

    /// Requests waiting for the engineer (hint bar, counts line).
    pub fn pending_requests(&self) -> usize {
        self.deliveries.pending().count()
            + self.pending_tasks().count()
            + self.pending_starts().count()
    }

    /// Records a finished delivery's outcome for its requester and the log.
    fn delivery_outcome(&mut self, id: u64) {
        let Some(d) = self.deliveries.get(id).cloned() else {
            return;
        };
        let outcome = d.state.label();
        // A task's hand-over decides the task's fate (US4).
        if let delivery::Purpose::Task(t) = d.purpose {
            self.task_handover_ended(t, &d.state);
        }
        if let (Some(s), delivery::Purpose::Message) = (d.from.session(), &d.purpose) {
            let mut v = serde_json::json!({ "id": d.id, "to": d.to, "state": outcome });
            if d.edited {
                v["edited"] = Value::Bool(true);
            }
            self.outbox.entry(s).or_default().deliveries.push(v);
        }
        self.activity.add(
            d.from.clone(),
            Action::DeliveryOutcome,
            &format!("→ #{}: {}", d.to, d.text),
            Some(outcome),
        );
    }

    /// Asks to type `text` into session `to` on `from`'s behalf. A covering permission approves
    /// it at once (and is announced); otherwise it waits for the engineer. `by_user`: the
    /// engineer's own action caused it (a wake-up after their signal, a task they queued), so it
    /// is approved as it is made.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_delivery(
        &mut self,
        sessions: &dyn Sessions,
        from: Party,
        to: u32,
        text: String,
        purpose: delivery::Purpose,
        by_user: bool,
        now: Instant,
    ) -> Result<(u64, bool), String> {
        let target = sessions
            .info(to)
            .filter(|t| t.running)
            .ok_or_else(|| format!("session #{to} is not running"))?;
        self.deliveries.room_for(&from)?;
        let cover = match from.session().and_then(|s| sessions.info(s)) {
            Some(src) => self
                .permissions
                .cover(&src, &target, permissions::Act::Deliver, now),
            None => None,
        };
        let expires = now + std::time::Duration::from_secs(self.settings.request_timeout_s);
        let subject = format!("{} → {}: {text}", from.label(), target.party().label());
        let id = self
            .deliveries
            .add(from.clone(), to, text, purpose, now, expires, cover);
        self.activity
            .add(from.clone(), Action::DeliveryRequested, &subject, None);
        if by_user && cover.is_none() {
            let _ = self.deliveries.approve(id, None);
            return Ok((id, true));
        }
        match cover {
            Some(p) => {
                self.activity.add(
                    from.clone(),
                    Action::PermissionUsed,
                    &format!("P{p}: {} → #{to}", from.label()),
                    None,
                );
                self.notices
                    .push(format!("P{p} used: {} → #{to}", from.label()));
            }
            None => self.notices.push(format!(
                "{} asks to deliver to #{to} (o to review)",
                from.label()
            )),
        }
        Ok((id, cover.is_some()))
    }

    /// The engineer approves a request (optionally with edited text).
    pub fn approve(&mut self, id: u64, text: Option<String>) -> Result<(), String> {
        self.deliveries.approve(id, text)
    }

    /// The engineer rejects a request; nothing is typed.
    pub fn reject(&mut self, id: u64) {
        if self
            .deliveries
            .finish(id, delivery::State::Rejected)
            .is_some()
        {
            self.delivery_outcome(id);
        }
    }

    /// Approved deliveries waiting to be typed.
    pub fn ready(&self) -> Vec<u64> {
        self.deliveries
            .list
            .iter()
            .filter(|d| d.state == delivery::State::Approved)
            .map(|d| d.id)
            .collect()
    }

    /// The grant to type an approved delivery (the only way to get one, research R-7).
    pub fn take_grant(&self, id: u64) -> Option<delivery::DeliveryGrant> {
        self.deliveries.grant(id)
    }

    /// The typing of a delivery finished (or the target was gone).
    pub fn delivered(&mut self, id: u64, result: Result<(), String>) {
        let state = match result {
            Ok(()) => delivery::State::Delivered,
            Err(e) => delivery::State::Failed(e),
        };
        if self.deliveries.finish(id, state).is_some() {
            self.delivery_outcome(id);
        }
    }

    /// The engineer grants a standing permission (FR-015).
    pub fn grant(&mut self, g: &permissions::Grant, now: Instant) -> Result<u32, String> {
        let id = self.permissions.grant(g, now)?;
        let p = self.permissions.list.last().cloned();
        if let Some(p) = p {
            self.activity.add(
                Party::User,
                Action::PermissionGranted,
                &format!(
                    "P{id}: {} → {} ({}) for {} min",
                    scope_label(p.source),
                    scope_label(p.target),
                    p.actions_label(),
                    g.duration.as_secs() / 60
                ),
                None,
            );
        }
        Ok(id)
    }

    pub fn revoke(&mut self, id: u32) {
        if self.permissions.revoke(id).is_some() {
            self.activity.add(
                Party::User,
                Action::PermissionRevoked,
                &format!("P{id}"),
                None,
            );
        }
    }

    /// Revokes every permission at once (SC-005): requests handled after this prompt again.
    pub fn revoke_all(&mut self) -> usize {
        // A burst is a permission: revoking all ends every burst too (004 FR-004).
        let projects: Vec<ProjectId> = self.bursts.iter().map(|b| b.project).collect();
        for p in projects {
            self.end_burst(p, "ended by you");
        }
        let n = self.permissions.revoke_all().len();
        if n > 0 {
            self.activity.add(
                Party::User,
                Action::PermissionRevoked,
                &format!("all ({n})"),
                None,
            );
        }
        n
    }

    /// The engineer clears an event: it is no longer signalled.
    pub fn clear_event(&mut self, key: &events::Key) {
        if let Some(e) = self.events.map.get_mut(key) {
            e.signal = None;
        }
        if let board::BoardId::Project(p) = key.0 {
            if let Some(b) = self.bursts.iter_mut().find(|b| b.project == p) {
                b.signed.remove(&key.1);
            }
        }
    }

    /// Events of a project's scope and the shared scope, sorted by scope then name.
    pub fn event_list(&self, project: Option<ProjectId>) -> Vec<events::Key> {
        let mut v: Vec<events::Key> = self
            .events
            .map
            .keys()
            .filter(|(s, _)| match s {
                board::BoardId::Shared => true,
                board::BoardId::Project(p) => Some(*p) == project,
            })
            .cloned()
            .collect();
        v.sort_by(|a, b| {
            (matches!(a.0, board::BoardId::Shared), &a.1)
                .cmp(&(matches!(b.0, board::BoardId::Shared), &b.1))
        });
        v
    }

    /// The event session `id` waits for, and since when (FR-010).
    pub fn waiting(&self, id: u32, now: Instant) -> Option<(&str, Instant)> {
        self.events.waiting(id, now)
    }

    /// Takes one call from the session's rate budget (FR-008). Over the limit, the engineer is
    /// told at most once a minute.
    pub(crate) fn charge(
        &mut self,
        caller: u32,
        sessions: &dyn Sessions,
        now: Instant,
    ) -> Result<(), Reply> {
        let per_min = self.settings.rate_per_min;
        let link = self.link(caller, now);
        match link.rate.take(now, per_min) {
            Ok(()) => Ok(()),
            Err(after) => {
                let tell = link
                    .slow_notice
                    .is_none_or(|t| now.saturating_duration_since(t).as_secs() >= 60);
                if tell {
                    link.slow_notice = Some(now);
                    let who = sessions
                        .info(caller)
                        .map_or(format!("#{caller}"), |i| i.party().label());
                    self.notices
                        .push(format!("{who} is calling Fallow's tools too fast"));
                }
                Err(Reply::Error(
                    "rate_limited",
                    format!("more than {per_min} calls per minute; retry after {after} s"),
                ))
            }
        }
    }

    /// Periodic work: waits that reached their limit, progress for parked waits, and (later
    /// stories) expiries and the queue. Returns true when something visible changed.
    pub fn tick(&mut self, sessions: &dyn Sessions, now: Instant) -> bool {
        // A restarted session keeps its number: it counts again (review).
        for b in &mut self.bursts {
            b.exited
                .retain(|id| !sessions.info(*id).is_some_and(|s| s.running));
        }
        let mut changed = self.tick_bursts(now);
        changed |= self.tick_gates(now);
        for (to, text) in std::mem::take(&mut self.pending_reports) {
            // Fallow's own message, approved as it is made (like the burst's briefing note).
            let _ = self.request_delivery(
                sessions,
                Party::Fallow,
                to,
                text,
                delivery::Purpose::Message,
                true,
                now,
            );
            changed = true;
        }
        for (project, part, why) in std::mem::take(&mut self.pending_repairs) {
            self.repair(sessions, project, part, &why, now);
            changed = true;
        }
        changed |= self.tick_deadlock(sessions, now);
        changed |= self.tick_signals(now);
        changed |= self.tick_lead(sessions, now);
        for (w, end) in self.events.due(now) {
            changed = true;
            let reply = match end {
                events::WaitEnd::TimedOut => {
                    let party = sessions.info(w.session).map_or(Party::User, |i| i.party());
                    self.activity
                        .add(party, Action::WaitEnded, &w.key.1, Some("timed out".into()));
                    Reply::Done(serde_json::json!({ "status": "timed_out" }))
                }
                events::WaitEnd::StillWaiting(d) => Reply::Done(
                    serde_json::json!({ "status": "still_waiting", "waited_s": d.as_secs() }),
                ),
                events::WaitEnd::Signalled(s) => Reply::Done(tools::signalled(&s)),
            };
            self.answer(w.conn, &w.id, &reply);
        }
        // Requests nobody answered in time (FR-012).
        let expired: Vec<u64> = self
            .deliveries
            .pending()
            .filter(|d| now >= d.expires)
            .map(|d| d.id)
            .collect();
        for id in expired {
            self.deliveries.finish(id, delivery::State::Expired);
            self.delivery_outcome(id);
            if let Some(d) = self.deliveries.get(id) {
                self.notices
                    .push(format!("Request from {} expired", d.from.label()));
            }
            changed = true;
        }
        for p in self.permissions.expire(now) {
            self.activity.add(
                Party::User,
                Action::PermissionRevoked,
                &format!("P{}", p.id),
                Some("expired".into()),
            );
            changed = true;
        }
        // Wake-ups for idle sessions whose event arrived (US2 scenario 5).
        for (session, key) in std::mem::take(&mut self.wakes) {
            let idle = sessions.info(session).is_some_and(|i| i.idle && i.running);
            let signal = self.events.map.get(&key).and_then(|e| e.signal.clone());
            let (Some(sig), true) = (signal, idle) else {
                continue;
            };
            let text = sig.message.clone().unwrap_or_default();
            let from = sig.from.clone();
            // A wake-up after the engineer's own signal is the engineer's action.
            let user = from == Party::User;
            let _ = self.request_delivery(
                sessions,
                from,
                session,
                text,
                delivery::Purpose::Wake(key.clone()),
                user,
                now,
            );
            changed = true;
        }
        // Agents' tasks nobody approved in time.
        let timeout = std::time::Duration::from_secs(self.settings.request_timeout_s);
        let stale: Vec<u32> = self
            .pending_tasks()
            .filter(|t| now.saturating_duration_since(t.created_at) >= timeout)
            .map(|t| t.id)
            .collect();
        for id in stale {
            self.tasks
                .finish(id, tasks::State::Cancelled("expired".into()));
            self.task_changed(id, "expired");
            if let Some(t) = self.tasks.get(id).cloned() {
                self.tell_task(t.created_by.session(), &t);
            }
            changed = true;
        }
        let late: Vec<u64> = self
            .pending_starts()
            .filter(|s| now >= s.expires)
            .map(|s| s.id)
            .collect();
        for id in late {
            if let Some(s) = self.starts.iter_mut().find(|s| s.id == id) {
                s.state = StartState::Expired;
            }
            self.start_outcome(id);
            changed = true;
        }
        changed |= self.advance(sessions, now);
        let mut progress = Vec::new();
        for w in &mut self.events.waits {
            if let (Some(token), Some(conn)) = (&w.progress, w.conn) {
                if now.saturating_duration_since(w.last_progress) >= events::PROGRESS_EVERY {
                    w.last_progress = now;
                    let secs = now.saturating_duration_since(w.since).as_secs_f64();
                    progress.push((conn, rpc::progress_line(token, secs)));
                }
            }
        }
        for (conn, line) in progress {
            if let Some(c) = self.conns.get(&conn) {
                let _ = c.out.send(Outgoing::Line(line));
            }
        }
        changed
    }

    /// A session ended: its link closes and its waits and interests go. A burst member that
    /// exited without a signal the merge needs from its role is no longer waited for, and the
    /// lead is told (004 run 3).
    pub fn session_ended(&mut self, id: u32) {
        if let Some(i) = self.bursts.iter().position(|b| b.roles.contains_key(&id)) {
            self.bursts[i].exited.insert(id);
            let b = &self.bursts[i];
            let role = b.roles[&id].clone();
            let (mut gone, mut stands): (Vec<String>, bool) = (Vec::new(), false);
            for c in &b.squad.merge_requires {
                if let squad::Condition::Signal { event, role: r } = c {
                    if *r != role || b.tainted.contains_key(&id) {
                        continue;
                    }
                    if publish::objects(b, id, event) {
                        stands = true;
                    } else if publish::dropped(b, id, event) && !gone.contains(event) {
                        gone.push(event.clone());
                    }
                }
            }
            let (project, name) = (b.project, b.project_name.clone());
            // Its work in progress fails and work queued for it never starts: the lead hands it
            // again (FR-042). A next-free task goes back to the queue by itself. The lead's own
            // exit is told to the engineer elsewhere.
            if id != b.lead && role != gates::GATE_ROLE {
                let label = |t: &tasks::Task| {
                    format!(
                        "T{}{}",
                        t.id,
                        t.part
                            .as_ref()
                            .map_or(String::new(), |p| format!(" (part {p})"))
                    )
                };
                let (mut failing, mut stranded) = (Vec::new(), Vec::new());
                for t in &self.tasks.list {
                    if t.created_at < b.started
                        || t.state.finished()
                        || t.target != tasks::Target::Session(id)
                    {
                        continue;
                    }
                    if t.state == tasks::State::InProgress {
                        failing.push(label(t));
                    } else {
                        stranded.push(label(t));
                    }
                }
                let mut what = Vec::new();
                if !failing.is_empty() {
                    what.push(format!(
                        "{} failed (the merge counts it until withdrawn)",
                        failing.join(", ")
                    ));
                }
                if !stranded.is_empty() {
                    what.push(format!("{} will never start", stranded.join(", ")));
                }
                if !what.is_empty() {
                    self.tell_lead(
                        project,
                        format!(
                            "#{id} {role} exited with unfinished work: {}. Hand that work to another session with task_create (with its part), then withdraw the old task(s) with task_report outcome \"cancelled\".",
                            what.join("; ")
                        ),
                    );
                }
            }
            if stands {
                // Another reviewer's approval does not overrule it: it may predate the repair
                // (review). The engineer decides.
                self.alerts.push(format!(
                    "{name}: #{id} {role} exited asking for changes; the merge waits for its objection until you clear review-changes-requested (Queue tab)"
                ));
                self.tell_lead(
                    project,
                    format!(
                        "#{id} {role} exited asking for changes; its objection stands until the user clears it. Have the changes made, and a {role} review them, then bring it to the user."
                    ),
                );
            } else if !gone.is_empty() {
                self.tell_lead(
                    project,
                    format!(
                        "#{id} {role} exited without signalling {}; the merge no longer waits for it. If the merge needs a {role}, start one with session_start.",
                        gone.join(" or ")
                    ),
                );
            }
        }
        self.events.drop_where(|w| w.session == id);
        self.events.recent.remove(&id);
        self.events.interests.remove(&id);
        let to_it: Vec<u64> = self
            .deliveries
            .list
            .iter()
            .filter(|d| d.to == id && !d.state.finished())
            .map(|d| d.id)
            .collect();
        for d in to_it {
            self.deliveries
                .finish(d, delivery::State::Failed("target exited".into()));
            self.delivery_outcome(d);
        }
        // Tasks in progress there whose hand-over was already typed.
        let running: Vec<u32> = self
            .tasks
            .list
            .iter()
            .filter(|t| t.assigned == Some(id) && t.state == tasks::State::InProgress)
            .map(|t| t.id)
            .collect();
        for t in running {
            self.task_handover_ended(t, &delivery::State::Failed("target exited".into()));
        }
        if let Some(l) = self.links.get_mut(&id) {
            if let Some(conn) = l.conn.take() {
                if let Some(c) = self.conns.remove(&conn) {
                    let _ = c.out.send(Outgoing::Close);
                }
            }
        }
    }
}

/// A role session's start: the role, its extra arguments, whether they bypass the agent's
/// checks, and a warning for the engineer.
/// (role, extra arguments, bypass, warning, why the user must approve it).
type RoleStart = (
    Option<String>,
    Vec<String>,
    bool,
    Option<String>,
    Option<String>,
);

impl Hub {
    /// Whether precondition `k` of task `t` holds: the event is signalled and, when the task
    /// names a role for it, the latest signal came from that role (004 FR-012), from a session
    /// whose signals still count (FR-013).
    pub(crate) fn condition_met(&self, t: &tasks::Task, k: &events::Key) -> bool {
        let Some(sig) = self.events.map.get(k).and_then(|e| e.signal.as_ref()) else {
            return false;
        };
        match t.role_conditions.iter().find(|(key, _)| key == k) {
            None => true,
            Some((_, role)) => {
                sig.from.role() == Some(role.as_str())
                    && !sig.from.session().is_some_and(|s| self.tainted(s))
            }
        }
    }

    /// The role, extra arguments and bypass flag for a session an agent asks to start (004
    /// FR-008, FR-009): during a burst a role is required and limited by the squad; outside one
    /// there are no roles.
    fn start_role(
        &self,
        sessions: &dyn Sessions,
        from: &SessionInfo,
        profile: &str,
        project: ProjectId,
        role: Option<&str>,
        assignment: Option<&delegation::Assignment>,
    ) -> Result<RoleStart, Reply> {
        let Some(b) = self.burst_for(from.id) else {
            return match role {
                None => Ok((None, Vec::new(), false, None, None)),
                Some(_) => Err(Reply::error(
                    "invalid",
                    "role: sessions have roles only while the project coordinates",
                )),
            };
        };
        let names: Vec<&str> = b.squad.roles.iter().map(|r| r.name.as_str()).collect();
        let mut warn = None;
        let Some(name) = role else {
            return Err(Reply::error(
                "invalid",
                format!(
                    "during coordination, session_start needs a role: one of {}",
                    names.join(", ")
                ),
            ));
        };
        if project != b.project {
            return Err(Reply::error(
                "not_allowed",
                format!("coordination starts sessions only in {}", b.project_name),
            ));
        }
        let Some(r) = b.squad.role(name) else {
            return Err(Reply::error(
                "invalid",
                format!("no role {name:?}; the squad has {}", names.join(", ")),
            ));
        };
        let have = |name: &str| self.role_count(sessions, b, name);
        if have(name) >= usize::from(r.max) {
            return Err(Reply::error(
                "not_allowed",
                format!(
                    "the squad allows at most {} {name} session(s) at once",
                    r.max
                ),
            ));
        }
        // A reviewer switched off for a recorded run (005 FR-017).
        if b.switched_off.contains(burst::OFF_REVIEWER)
            && roles::may_signal(&b.squad, r, "review-approved")
        {
            return Err(Reply::error(
                "not_allowed",
                "reviewers are switched off for this run",
            ));
        }
        // Builders start only for work the manifest delegates (005 FR-001 to FR-006, FR-010).
        if b.squad.adaptive() && r.level.edits() && name != "lead" {
            if b.switched_off.contains(burst::OFF_PARALLEL) {
                return Err(Reply::error(
                    "not_allowed",
                    "builders are switched off for this run: you build every piece of work yourself",
                ));
            }
            let cap = b.squad.max_parallel_builders.unwrap_or(b.allowance).max(1);
            let builders = b
                .roles
                .iter()
                .filter(|(id, role)| {
                    *role != "lead"
                        && b.squad.role(role).is_some_and(|x| x.level.edits())
                        && sessions.info(**id).is_some_and(|s| s.running)
                })
                .count()
                + self
                    .starts
                    .iter()
                    .filter(|s| {
                        s.project == b.project
                            && s.assignment.is_some()
                            && matches!(s.state, StartState::Pending | StartState::Approved)
                    })
                    .count();
            if builders >= usize::from(cap) {
                return Err(Reply::error(
                    "not_allowed",
                    format!("at most {cap} builder(s) run at once in this squad"),
                ));
            }
            let assigned = |work: &str, part: Option<&str>| {
                b.assigned
                    .iter()
                    .filter(|(id, (w, p))| {
                        w == work
                            && (part.is_none() || p.as_deref() == part)
                            && sessions.info(**id).is_some_and(|s| s.running)
                    })
                    .count()
                    + self
                        .starts
                        .iter()
                        .filter(|s| {
                            s.project == b.project
                                && matches!(s.state, StartState::Pending | StartState::Approved)
                                && s.assignment.as_ref().is_some_and(|(w, p)| {
                                    w == work && (part.is_none() || p.as_deref() == part)
                                })
                        })
                        .count()
            };
            if let Some(why) = self.delegation_refusal(
                b,
                assignment.map(|a| a.0.as_str()),
                assignment.and_then(|a| a.1.as_deref()),
                &assigned,
            ) {
                return Err(Reply::error("not_allowed", why));
            }
        }
        // Beyond the burst's starts, or using a start kept for a role the merge still needs a
        // session of, the start waits for the user instead of failing (004 run 3).
        // Only roles whose merge condition is still unmet count (review).
        let left = self.starts_left(b);
        let needed = self.needed_roles(b);
        let missing: Vec<&str> = needed
            .iter()
            .map(String::as_str)
            .filter(|m| *m != name && have(m) == 0)
            .collect();
        let hold = if left == 0 {
            Some("this burst's session starts are used up".to_string())
        } else if !(needed.iter().any(|n| n == name) && have(name) == 0)
            && usize::from(left) <= missing.len()
        {
            Some(format!(
                "{left} start(s) left, kept for {} (the merge needs it)",
                missing.join(", ")
            ))
        } else {
            None
        };
        // Two reviewers on the same agent and model are less independent (FR-027): a warning.
        if roles::may_signal(&b.squad, r, "review-approved") {
            let same = b.roles.iter().any(|(id, other)| {
                b.squad.role(other).is_some_and(|o| {
                    roles::may_signal(&b.squad, o, "review-approved") && o.model == r.model
                }) && sessions
                    .info(*id)
                    .is_some_and(|s| s.running && s.profile_id == profile)
            });
            if same {
                warn = Some(format!(
                    "A second reviewer on the same agent and model ({profile}{}): its review is less independent",
                    r.model.as_ref().map_or(String::new(), |m| format!(" {m}"))
                ));
            }
        }
        let (args, bypass) = match sessions.profile(profile) {
            Some(p) => {
                let args = roles::role_args(r, &p).map_err(|e| Reply::error("not_allowed", e))?;
                let bypass = roles::bypasses(&args) || roles::bypasses(&p.args);
                (args, bypass)
            }
            None => (Vec::new(), false),
        };
        Ok((Some(name.to_string()), args, bypass, warn, hold))
    }

    /// Sessions the lead may still start without asking: the burst's grant and any other
    /// project grant the user gave it (review).
    pub fn starts_left(&self, b: &burst::Burst) -> u8 {
        self.permissions
            .list
            .iter()
            .filter(|p| {
                p.target == permissions::Scope::Project(b.project)
                    && matches!(p.source, permissions::Scope::Project(x) if x == b.project)
                    || p.target == permissions::Scope::Project(b.project)
                        && p.source == permissions::Scope::Session(b.lead)
            })
            .map(|p| p.start_limit.unwrap_or(0).saturating_sub(p.starts_used))
            .fold(0u8, u8::saturating_add)
    }

    /// Running sessions in `role` plus starts for it under way.
    fn role_count(&self, sessions: &dyn Sessions, b: &burst::Burst, role: &str) -> usize {
        b.roles
            .iter()
            .filter(|(id, r)| *r == role && sessions.info(**id).is_some_and(|s| s.running))
            .count()
            + self
                .starts
                .iter()
                .filter(|s| {
                    s.project == b.project
                        && s.role.as_deref() == Some(role)
                        && matches!(s.state, StartState::Pending | StartState::Approved)
                })
                .count()
    }

    /// Roles (not the lead) with a merge condition not met yet (004 run 3).
    pub(crate) fn needed_roles(&self, b: &burst::Burst) -> Vec<String> {
        let checks = self
            .readiness(b.project)
            .map(|r| r.checks)
            .unwrap_or_default();
        let mut v: Vec<String> = Vec::new();
        for c in &b.squad.merge_requires {
            let (squad::Condition::AllDone(r) | squad::Condition::Signal { role: r, .. }) = c
            else {
                continue;
            };
            let label = publish::condition_label(c);
            let met = checks.iter().any(|k| k.met && k.condition == label);
            if r != "lead" && !met && !v.contains(r) {
                v.push(r.clone());
            }
        }
        v
    }

    /// Needed roles with no running session and no start under way.
    pub(crate) fn missing_roles(&self, sessions: &dyn Sessions, b: &burst::Burst) -> Vec<String> {
        self.needed_roles(b)
            .into_iter()
            .filter(|m| self.role_count(sessions, b, m) == 0)
            .collect()
    }

    /// Whether a session's signals stopped counting (a read-only role's worktree changed,
    /// FR-013).
    pub fn tainted(&self, session: u32) -> bool {
        self.bursts.iter().any(|b| b.tainted.contains_key(&session))
    }

    /// Sessions of read-only roles, for the worktree check (FR-013).
    pub fn read_only_sessions(&self) -> Vec<u32> {
        self.bursts
            .iter()
            .flat_map(|b| {
                b.roles.iter().filter_map(|(id, r)| {
                    let ro = b.squad.role(r).is_some_and(|r| !r.level.edits());
                    (ro && !b.tainted.contains_key(id)).then_some(*id)
                })
            })
            .collect()
    }

    /// A read-only session's worktree changed (FR-013): the engineer is warned, its signals stop
    /// counting, and the events it signalled are cleared.
    pub fn taint(&mut self, session: u32, why: String) {
        let Some(b) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&session))
        else {
            return;
        };
        if b.tainted.contains_key(&session) {
            return;
        }
        let role = b.roles.get(&session).cloned().unwrap_or_default();
        let project = b.project;
        b.tainted.insert(session, why.clone());
        let cleared: Vec<events::Key> = self
            .events
            .map
            .iter()
            .filter(|(k, e)| {
                k.0 == board::BoardId::Project(project)
                    && e.signal
                        .as_ref()
                        .is_some_and(|s| s.from.session() == Some(session))
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in &cleared {
            self.clear_event(k);
        }
        let text = format!(
            "#{session} {role}'s worktree changed ({why}): its signals and verdicts no longer count."
        );
        self.boards.post(
            board::BoardId::Project(project),
            Party::Fallow,
            text.clone(),
            std::time::SystemTime::now(),
            TEXT_BUDGET,
        );
        self.activity
            .add(Party::Fallow, Action::Refused, &text, None);
        self.notices.push(text);
    }
}
