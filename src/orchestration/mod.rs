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
        if let Some(l) = self.links.get_mut(&id) {
            if let Some(conn) = l.conn.take() {
                if let Some(c) = self.conns.remove(&conn) {
                    let _ = c.out.send(Outgoing::Close);
                }
            }
        }
    }
}
