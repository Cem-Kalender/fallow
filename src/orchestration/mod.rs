//! Agent orchestration (feature 003): boards, events, deliveries, permissions, a task queue and
//! session-start requests, offered to agents in Fallow sessions as MCP tools.
//!
//! Everything lives in one in-memory `Hub`, owned by the main loop and created when the first
//! session with orchestration starts (FR-005). Nothing here is ever written to disk or logged
//! (FR-023, constitution VII).

pub mod activity;
pub mod board;
pub mod bridge;
pub mod channel;
pub mod delivery;
pub mod events;
pub mod identity;
pub mod inject;
pub mod permissions;
pub mod rpc;
pub mod tasks;
pub mod tools;

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use serde_json::{Value, json};

use crate::config::schema::OrchestrationSettings;
use crate::projects::ProjectId;
use activity::{Action, Activity};
use channel::{Channel, ConnId, Incoming, Outgoing};
use identity::Peer;

/// Who did something: the engineer, or an agent in a session. Always set by Fallow from the
/// connection, never from tool arguments (FR-003, FR-007, SC-004). The project and profile are
/// a snapshot at the time of the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    User,
    Agent {
        session: u32,
        profile: String,
        project: String,
    },
}

impl Party {
    /// `#3 Claude Code · api`, or `you`.
    pub fn label(&self) -> String {
        match self {
            Party::User => "you".into(),
            Party::Agent {
                session,
                profile,
                project,
            } => format!("#{session} {profile} · {project}"),
        }
    }

    /// The `from` object agents receive (research R-8).
    pub fn json(&self) -> Value {
        match self {
            Party::User => json!({ "kind": "user" }),
            Party::Agent {
                session,
                profile,
                project,
            } => {
                json!({ "kind": "agent", "session": session, "profile": profile, "project": project })
            }
        }
    }

    pub fn session(&self) -> Option<u32> {
        match self {
            Party::Agent { session, .. } => Some(*session),
            Party::User => None,
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
}

impl SessionInfo {
    pub fn party(&self) -> Party {
        Party::Agent {
            session: self.id,
            profile: self.profile.clone(),
            project: self.project_name.clone(),
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
            next_start: 0,
            settings,
            channel: None,
            conns: HashMap::new(),
            links: HashMap::new(),
            refusals: HashMap::new(),
            notices: Vec::new(),
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
                        let _ = verdict.send(Ok(()));
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
        now: Instant,
    ) -> Result<(u64, StartState), Reply> {
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
        self.next_start += 1;
        let id = self.next_start;
        let state = match sessions.start_allowed() {
            Err(why) => StartState::Refused(why),
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
        });
        if matches!(state, StartState::Refused(_)) {
            self.start_outcome(id);
        }
        Ok((id, state))
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
        let (from, task) = (s.from.clone(), s.first_task.clone());
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

    /// The assigned agent reports a task done or failed (FR-020).
    pub fn report_task(
        &mut self,
        caller: u32,
        id: u32,
        done: bool,
        note: Option<String>,
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
        let state = if done {
            tasks::State::Done
        } else {
            tasks::State::Failed(note.unwrap_or_default())
        };
        self.tasks.finish(id, state.clone());
        self.task_changed(id, if done { "done" } else { "failed" });
        if let Some(t) = self.tasks.get(id).cloned() {
            self.tell_task(t.created_by.session(), &t);
        }
        Ok(state)
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
            .filter(|t| t.state == tasks::State::InProgress)
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
            let unmet = t.preconditions.iter().any(|k| {
                self.events
                    .map
                    .get(k)
                    .and_then(|e| e.signal.as_ref())
                    .is_none()
            });
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
                    && !busy.contains(&s.id)
                    && self.events.waiting(s.id, now).is_none()
            };
            let pick = match &t.target {
                tasks::Target::Session(sid) => all.iter().find(|s| s.id == *sid && free(s)),
                tasks::Target::NextFree {
                    profile, project, ..
                } => all
                    .iter()
                    .find(|s| &s.profile_id == profile && s.project == *project && free(s)),
            }
            .map(|s| s.id);
            let Some(to) = pick else {
                continue;
            };
            let by_user = t.created_by == Party::User || t.approved_handover;
            if let Ok((did, _)) = self.request_delivery(
                sessions,
                t.created_by.clone(),
                to,
                t.text.clone(),
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
        let _ = sessions;
        let mut changed = false;
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

    /// A session ended: its link closes and its waits and interests go.
    pub fn session_ended(&mut self, id: u32) {
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
