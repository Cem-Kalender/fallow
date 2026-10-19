//! Agent sessions (002): agents running in pseudo-terminals inside Fallow (data-model.md
//! "Session").

pub mod keys;
pub mod platform;
pub mod profiles;
pub mod pty;
pub mod scrollback;
pub mod term;
pub mod worktree;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::app::events::{AppEvent, OutputWaker, SessionId};
use crate::projects::ProjectId;
use keys::LeaveKey;
use profiles::AgentProfile;
use pty::{Pty, SpawnSpec};
use term::{Signal, Term};

/// data-model.md "SessionState" (the US2 states `NeedsAttention` and `Stopping` are driven by
/// the attention and control logic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Working,
    NeedsAttention,
    Exited(Option<u32>),
    Failed(String),
    Stopping,
}

impl SessionState {
    pub fn label(&self) -> String {
        match self {
            SessionState::Starting => "starting".into(),
            SessionState::Working => "working".into(),
            SessionState::NeedsAttention => "needs attention".into(),
            SessionState::Exited(Some(c)) => format!("exited ({c})"),
            SessionState::Exited(None) => "exited".into(),
            SessionState::Failed(why) => format!("failed: {why}"),
            SessionState::Stopping => "stopping".into(),
        }
    }

    pub fn running(&self) -> bool {
        matches!(
            self,
            SessionState::Starting
                | SessionState::Working
                | SessionState::NeedsAttention
                | SessionState::Stopping
        )
    }
}

/// Where a session's agent runs (US6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Workdir {
    ProjectFolder,
    Worktree(worktree::Worktree),
}

impl Workdir {
    pub fn branch(&self) -> Option<&str> {
        match self {
            Workdir::Worktree(w) => Some(&w.branch),
            Workdir::ProjectFolder => None,
        }
    }
}

/// One agent in a terminal pane.
#[derive(Debug)]
pub struct Session {
    pub id: SessionId,
    pub project: ProjectId,
    pub project_name: String,
    pub cwd: PathBuf,
    pub workdir: Workdir,
    pub profile: AgentProfile,
    pub term: Arc<Mutex<Term>>,
    pub pty: Option<Pty>,
    pub state: SessionState,
    pub state_since: Instant,
    pub last_output: Instant,
    /// The agent rang the bell; cleared when the session is shown with focus.
    pub bell: bool,
    /// The agent's latest window title.
    pub title: Option<String>,
    /// Lines scrolled back from the live screen (0 = following output).
    pub scroll: usize,
    /// Selection as (virtual line, column) start and end, inclusive start, exclusive end.
    pub selection: Option<((usize, u16), (usize, u16))>,
    output_pending: Option<Arc<AtomicBool>>,
}

impl Session {
    pub fn label(&self) -> String {
        match self.workdir.branch() {
            Some(b) => format!(
                "#{} {} · {} · {b}",
                self.id, self.profile.name, self.project_name
            ),
            None => format!("#{} {} · {}", self.id, self.profile.name, self.project_name),
        }
    }

    fn set_state(&mut self, s: SessionState) {
        if self.state != s {
            self.state = s;
            self.state_since = Instant::now();
        }
    }

    /// Sends bytes to the agent, if it runs.
    pub fn write(&self, bytes: &[u8]) {
        if let Some(p) = &self.pty {
            p.write(bytes);
        }
    }

    /// Keeps the agent's terminal at the pane's size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let changed = match self.term.lock() {
            Ok(mut t) => {
                let before = t.size();
                t.resize(rows, cols);
                before != t.size()
            }
            Err(_) => false,
        };
        if changed {
            if let Some(p) = &self.pty {
                p.resize(rows, cols);
            }
        }
    }

    /// The agent's input modes needed to encode keys and pastes.
    pub fn modes(&self) -> (bool, bool, bool) {
        self.term.lock().map_or((false, false, false), |t| {
            let s = t.screen();
            (
                s.application_cursor(),
                s.bracketed_paste(),
                t.hooks().focus_reporting,
            )
        })
    }
}

/// Settings that shape sessions (`[sessions]`).
#[derive(Debug, Clone)]
pub struct Limits {
    pub scrollback_lines: usize,
    pub max_sessions: usize,
    pub leave_key: LeaveKey,
    /// A new worktree by default for git projects (FR-031).
    pub worktrees: bool,
    pub worktree_root: Option<PathBuf>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            scrollback_lines: 10_000,
            max_sessions: 8,
            leave_key: LeaveKey::default(),
            worktrees: true,
            worktree_root: None,
        }
    }
}

/// All sessions of this run.
#[derive(Debug, Default)]
pub struct SessionManager {
    pub sessions: Vec<Session>,
    next_id: SessionId,
    pub limits: Limits,
    /// Sessions started in this run (for `--bench-stats`).
    pub started: u64,
}

/// Where and how big to start.
#[derive(Debug, Clone)]
pub struct StartRequest {
    pub project: ProjectId,
    pub project_name: String,
    /// The project folder, or the worktree's folder.
    pub cwd: PathBuf,
    pub workdir: Workdir,
    pub profile: AgentProfile,
    pub rows: u16,
    pub cols: u16,
}

impl SessionManager {
    pub fn running(&self) -> usize {
        self.sessions.iter().filter(|s| s.state.running()).count()
    }

    pub fn get(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Starts a session (FR-001). Refuses beyond `max_sessions` (FR-007). A failure to start is
    /// kept as a session in state `Failed`, so the pane can show why (US5 scenario 3).
    pub fn start(
        &mut self,
        req: StartRequest,
        events: &Sender<AppEvent>,
    ) -> Result<SessionId, String> {
        if self.running() >= self.limits.max_sessions {
            return Err(format!(
                "{} sessions are running, the limit is {} (sessions.max_sessions)",
                self.running(),
                self.limits.max_sessions
            ));
        }
        self.next_id += 1;
        let id = self.next_id;
        let term = Arc::new(Mutex::new(Term::new(
            req.rows,
            req.cols,
            self.limits.scrollback_lines,
        )));
        let now = Instant::now();
        let mut s = Session {
            id,
            project: req.project,
            project_name: req.project_name.clone(),
            cwd: req.cwd.clone(),
            workdir: req.workdir.clone(),
            profile: req.profile.clone(),
            term,
            pty: None,
            state: SessionState::Starting,
            state_since: now,
            last_output: now,
            bell: false,
            title: None,
            scroll: 0,
            selection: None,
            output_pending: None,
        };
        self.spawn_into(&mut s, req.rows, req.cols, events);
        self.sessions.push(s);
        self.started += 1;
        Ok(id)
    }

    fn spawn_into(&self, s: &mut Session, rows: u16, cols: u16, events: &Sender<AppEvent>) {
        let resolved = match profiles::resolve(&s.profile.command) {
            Ok(r) => r,
            Err(e) => {
                s.set_state(SessionState::Failed(e));
                return;
            }
        };
        let mut args = resolved.prefix_args;
        args.extend(s.profile.args.iter().cloned());
        let spec = SpawnSpec {
            program: resolved.program,
            args,
            cwd: s.cwd.clone(),
            env: s.profile.env.clone(),
            rows,
            cols,
        };
        let waker = OutputWaker::new(s.id, events.clone());
        s.output_pending = Some(waker.flag());
        match pty::spawn(&spec, s.id, s.term.clone(), waker, events.clone()) {
            Ok(p) => {
                s.pty = Some(p);
                s.set_state(SessionState::Starting);
            }
            Err(e) => s.set_state(SessionState::Failed(e)),
        }
    }

    /// Restarts an exited or failed session with the same profile in the same project.
    pub fn restart(&mut self, id: SessionId, events: &Sender<AppEvent>) -> Result<(), String> {
        if self.running() >= self.limits.max_sessions {
            return Err(format!("the session limit is {}", self.limits.max_sessions));
        }
        let scrollback = self.limits.scrollback_lines;
        let Some(i) = self.sessions.iter().position(|s| s.id == id) else {
            return Err("no such session".into());
        };
        if self.sessions[i].state.running() {
            return Err("the session is still running".into());
        }
        let mut s = self.sessions.remove(i);
        s.pty = None;
        let (rows, cols) = s.term.lock().map(|t| t.size()).unwrap_or((24, 80));
        s.term = Arc::new(Mutex::new(Term::new(rows, cols, scrollback)));
        s.scroll = 0;
        s.selection = None;
        s.title = None;
        self.spawn_into(&mut s, rows, cols, events);
        self.sessions.insert(i, s);
        self.started += 1;
        Ok(())
    }

    /// Closes an exited or failed session (a running one must be stopped first, US2).
    pub fn close(&mut self, id: SessionId) -> Result<(), String> {
        match self.sessions.iter().position(|s| s.id == id) {
            Some(i) if !self.sessions[i].state.running() => {
                self.sessions.remove(i);
                Ok(())
            }
            Some(_) => Err("the session is still running; stop it first".into()),
            None => Err("no such session".into()),
        }
    }

    /// New output arrived. Returns the signals the agent sent since the last call.
    pub fn output(&mut self, id: SessionId) -> Vec<Signal> {
        let Some(s) = self.get_mut(id) else {
            return Vec::new();
        };
        if let Some(f) = &s.output_pending {
            f.store(false, Ordering::Release);
        }
        s.last_output = Instant::now();
        let signals = s
            .term
            .lock()
            .map(|mut t| t.take_signals())
            .unwrap_or_default();
        for sig in &signals {
            match sig {
                Signal::Bell => s.bell = true,
                Signal::Title(t) => s.title = Some(t.clone()),
                _ => {}
            }
        }
        if matches!(s.state, SessionState::Starting) {
            s.set_state(SessionState::Working);
        }
        signals
    }

    /// The agent exited.
    pub fn exited(&mut self, id: SessionId, code: Option<u32>) {
        if let Some(s) = self.get_mut(id) {
            s.set_state(SessionState::Exited(code));
            // Keep the terminal (last output); release the pseudo-terminal.
            s.pty = None;
        }
    }

    /// Ends every agent (quit after confirmation; dropping does the same).
    pub fn stop_all(&mut self) {
        for s in &mut self.sessions {
            if let Some(p) = &mut s.pty {
                p.terminate();
            }
        }
    }
}
