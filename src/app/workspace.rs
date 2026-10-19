//! The agent workspace (002): the project list, the sessions of the selected project, and a
//! focused session that receives every key but the leave key (FR-003, FR-019).

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::commands::Context;
use super::keymap;
use super::state::{AppState, Overlay, Pane};
use crate::app::events::SessionId;
use crate::editor::selection::LineInput;
use crate::session::profiles;
use crate::session::worktree::{self, Worktree};
use crate::session::{SessionState, StartRequest, Workdir};
use crate::ui::workspace as ws_ui;

/// Which view is on screen (FR-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Agents,
    Editor,
}

/// Focus inside the agent workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentsFocus {
    #[default]
    Projects,
    Sessions,
    /// The active session has focus: keys go to the agent.
    Session,
    /// The LLM process panel (001 keys).
    Processes,
}

impl AgentsFocus {
    /// `Tab` order in the workspace (a focused session is left with the leave key, not `Tab`).
    pub fn next(self, back: bool) -> AgentsFocus {
        use AgentsFocus::*;
        match (self, back) {
            (Projects, false) | (Processes, true) => Sessions,
            (Sessions | Session, false) => Processes,
            (Processes, false) | (Sessions | Session, true) => Projects,
            (Projects, true) => Processes,
        }
    }
}

/// data-model.md "Workspace".
#[derive(Debug, Default)]
pub struct Workspace {
    pub view: View,
    pub focus: AgentsFocus,
    /// The session shown in the pane.
    pub active: Option<SessionId>,
    /// Selected row in the session list of the selected project.
    pub list_selected: usize,
    /// The leave key was pressed once inside the session; a second press sends it to the agent.
    pub leave_armed: Option<Instant>,
    /// A drag that started in the session pane selects text.
    pub drag_from: Option<(usize, u16)>,
    /// The session list shows every session of every project (`Ctrl-T`, FR-008).
    pub overview: bool,
    /// A second session shown beside the active one (FR-011).
    pub split: Option<SessionId>,
    /// The LLM panel shows only this project's processes (`f`, FR-016).
    pub process_filter: Option<crate::projects::ProjectId>,
}

/// Where a new session's agent runs (US6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    NewWorktree,
    ProjectFolder,
    Existing(Worktree),
}

impl Where {
    pub fn label(&self, profile: &str) -> String {
        match self {
            Where::NewWorktree => format!("new worktree ({}{profile}-…)", worktree::PREFIX),
            Where::ProjectFolder => "project folder".into(),
            Where::Existing(w) => format!("{}  (existing worktree)", w.branch),
        }
    }
}

/// The start menu (contracts/ui-commands.md "Start menu").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartMenu {
    pub agents: Vec<MenuItem>,
    pub agent: usize,
    /// Empty for projects that are not git repositories.
    pub wheres: Vec<Where>,
    pub where_sel: usize,
    /// `Tab` moved the selection to "Where".
    pub on_where: bool,
    pub error: Option<String>,
}

/// One entry of the profile menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    /// `Err(reason)` when the command can't be started.
    pub available: Result<(), String>,
}

impl AppState {
    /// The context for hints and bindings in the agent workspace.
    pub fn agents_context(&self) -> Context {
        match self.workspace.focus {
            AgentsFocus::Projects => Context::Projects,
            AgentsFocus::Sessions => Context::Sessions,
            AgentsFocus::Session => Context::Session,
            AgentsFocus::Processes => Context::Processes,
        }
    }

    /// Sessions of the selected project, in start order.
    pub fn project_sessions(&self) -> Vec<SessionId> {
        let Some(p) = self.projects.selected() else {
            return Vec::new();
        };
        self.sessions
            .sessions
            .iter()
            .filter(|s| s.project == p.id)
            .map(|s| s.id)
            .collect()
    }

    /// Keys in the agent workspace. `None`: not handled here (global keys follow).
    pub(super) fn workspace_key(&mut self, key: KeyEvent) -> Option<bool> {
        if key.kind == KeyEventKind::Release {
            return Some(false);
        }
        if self.workspace.focus == AgentsFocus::Session {
            return Some(self.session_key(key));
        }
        let ctx = self.agents_context();
        match keymap::lookup(&key, ctx, false) {
            Some(id) => Some(self.run(id)),
            None => Some(false),
        }
    }

    /// A key while a session has focus: everything goes to the agent except the leave key.
    fn session_key(&mut self, key: KeyEvent) -> bool {
        let Some(id) = self.workspace.active else {
            self.workspace.focus = AgentsFocus::Sessions;
            return true;
        };
        let leave = self.sessions.limits.leave_key;
        if leave.matches(&key) {
            // The first press leaves at once; an immediate second press (handled in
            // `after_leave_key`) goes back and sends the key to the agent.
            self.set_session_focus(false);
            self.workspace.leave_armed = Some(Instant::now());
            return true;
        }
        let Some(s) = self.sessions.get_mut(id) else {
            return false;
        };
        if !s.state.running() {
            // An exited session: keys act on the pane, not the agent.
            self.workspace.focus = AgentsFocus::Sessions;
            return true;
        }
        let (app_cursor, _, _) = s.modes();
        if let Some(bytes) = crate::session::keys::encode(&key, app_cursor) {
            s.scroll = 0;
            s.selection = None;
            crate::session::attention::on_input(s, Instant::now());
            s.write(&bytes);
        }
        false
    }

    /// A paste into the focused session (FR-002: as a paste when the agent asked for it).
    pub(super) fn session_paste(&mut self, text: &str) -> bool {
        if self.workspace.view != View::Agents || self.workspace.focus != AgentsFocus::Session {
            return false;
        }
        let Some(s) = self
            .workspace
            .active
            .and_then(|id| self.sessions.get_mut(id))
        else {
            return false;
        };
        let (_, bracketed, _) = s.modes();
        s.scroll = 0;
        s.write(&crate::session::keys::paste(text, bracketed));
        false
    }

    /// Gives or takes the session focus and tells the agent if it asked (focus reporting).
    /// A key right after leaving a session. The leave key again, with nothing in between and
    /// within 600 ms, goes back into the session and sends the leave key to the agent (FR-003).
    /// Returns `Some` if it handled the key.
    pub(super) fn after_leave_key(&mut self, key: &KeyEvent) -> Option<bool> {
        let armed = self.workspace.leave_armed.take()?;
        let leave = self.sessions.limits.leave_key;
        if !leave.matches(key) || armed.elapsed() >= std::time::Duration::from_millis(600) {
            return None;
        }
        let id = self.workspace.active?;
        self.set_session_focus(true);
        if let Some(s) = self.sessions.get(id) {
            s.write(&leave.bytes());
        }
        Some(true)
    }

    fn set_session_focus(&mut self, on: bool) {
        self.workspace.leave_armed = None;
        self.workspace.focus = if on {
            AgentsFocus::Session
        } else {
            AgentsFocus::Sessions
        };
        if let Some(s) = self
            .workspace
            .active
            .and_then(|id| self.sessions.get_mut(id))
        {
            if on {
                s.bell = false;
            }
            let (_, _, reporting) = s.modes();
            if reporting {
                s.write(if on { b"\x1b[I" } else { b"\x1b[O" });
            }
        }
    }

    /// US1/US2/US3 workspace commands. `None` if `id` is not one.
    pub(super) fn run_workspace(&mut self, id: &str) -> Option<bool> {
        let agents = self.workspace.view == View::Agents;
        Some(match id {
            "pane.next" | "pane.prev" if agents => {
                self.workspace.focus = self.workspace.focus.next(id == "pane.prev");
                true
            }
            "pane.files" if agents => {
                self.workspace.focus = AgentsFocus::Projects;
                true
            }
            "pane.editor" if agents => {
                self.workspace.focus = AgentsFocus::Sessions;
                true
            }
            "pane.processes" if agents => {
                self.workspace.focus = AgentsFocus::Processes;
                true
            }
            "view.toggle" => self.toggle_view(),
            "projects.up" => self.move_project(-1),
            "projects.down" => self.move_project(1),
            "projects.open" => {
                self.workspace.focus = AgentsFocus::Sessions;
                self.workspace.list_selected = 0;
                self.show_listed_session();
                true
            }
            "project.add" => self.run("project.open"),
            "session.new" | "session.new_here" => self.open_profile_menu(),
            "sessions.up" => self.move_session(-1),
            "sessions.down" => self.move_session(1),
            "session.focus" => self.focus_listed_session(),
            "session.restart" => self.restart_listed(),
            "session.close" => self.close_listed(),
            "session.scroll_up" => self.scroll_active(true),
            "session.scroll_down" => self.scroll_active(false),
            "session.search" => {
                if self.workspace.active.is_none() {
                    return Some(false);
                }
                self.overlay = Overlay::Prompt {
                    kind: super::editing::PromptKind::SearchSession,
                    input: LineInput::new(""),
                    error: None,
                };
                true
            }
            "session.copy" => self.copy_session_selection(),
            "sessions.back" => {
                if self.workspace.overview {
                    self.workspace.overview = false;
                    self.workspace.list_selected = 0;
                } else {
                    self.workspace.focus = AgentsFocus::Projects;
                }
                true
            }
            "sessions.overview" => {
                self.workspace.overview = !self.workspace.overview;
                self.workspace.view = View::Agents;
                self.workspace.focus = AgentsFocus::Sessions;
                self.workspace.list_selected = self
                    .workspace
                    .active
                    .and_then(|a| self.listed_sessions().iter().position(|s| *s == a))
                    .unwrap_or(0);
                true
            }
            "session.next" => self.cycle_session(1),
            "session.prev" => self.cycle_session(-1),
            "session.split" => self.toggle_split(),
            "session.stop" => self.ask_stop(),
            "session.process" => self.jump_to_process(),
            "proc.jump_session" => self.jump_to_session(),
            "session.leave" => {
                self.set_session_focus(false);
                true
            }
            _ => return None,
        })
    }

    pub fn toggle_view(&mut self) -> bool {
        self.workspace.view = match self.workspace.view {
            View::Agents => {
                self.sync_editor_project();
                View::Editor
            }
            View::Editor => View::Agents,
        };
        true
    }

    /// The editor view shows the selected project: swap its lazy tree into the editor.
    fn sync_editor_project(&mut self) {
        let wt = self
            .workspace
            .active
            .filter(|_| self.workspace.focus != AgentsFocus::Projects)
            .and_then(|id| self.sessions.get(id))
            .and_then(|s| match &s.workdir {
                Workdir::Worktree(w) => Some(w.path.clone()),
                Workdir::ProjectFolder => None,
            });
        if let Some(path) = wt {
            if self.project.as_ref().is_some_and(|p| p.root == path) {
                return;
            }
            if let Some(cur) = self.project.take() {
                if let Some(i) = self.projects.index_of(&cur.root) {
                    self.projects.projects[i].editor = Some(cur);
                }
            }
            match crate::editor::Project::open(&path) {
                Ok(p) => {
                    self.project = Some(p);
                    self.focus = Pane::Files;
                    self.watch_dirty = true;
                }
                Err(e) => self.say(format!("{}: {e}", path.display())),
            }
            return;
        }
        let Some(sel) = self.projects.selected else {
            return;
        };
        let want = self.projects.projects[sel].path.clone();
        if self.project.as_ref().is_some_and(|p| p.root == want) {
            return;
        }
        // Put the current editor tree back into its entry, then take the selected one's.
        if let Some(cur) = self.project.take() {
            if let Some(i) = self.projects.index_of(&cur.root) {
                self.projects.projects[i].editor = Some(cur);
            }
        }
        match self.projects.projects[sel].editor() {
            Ok(_) => {
                self.project = self.projects.projects[sel].editor.take();
                self.focus = Pane::Files;
                self.watch_dirty = true;
            }
            Err(e) => self.say(format!("{}: {e}", want.display())),
        }
    }

    fn move_project(&mut self, by: isize) -> bool {
        let n = self.projects.len();
        if n == 0 {
            return false;
        }
        let cur = self.projects.selected.unwrap_or(0) as isize;
        let new = (cur + by).clamp(0, n as isize - 1) as usize;
        let changed = Some(new) != self.projects.selected;
        self.projects.selected = Some(new);
        if changed {
            self.workspace.list_selected = 0;
            self.show_listed_session();
        }
        changed
    }

    /// The sessions in the list: the selected project's, or all of them in the overview.
    pub fn listed_sessions(&self) -> Vec<SessionId> {
        if self.workspace.overview {
            self.sessions.sessions.iter().map(|s| s.id).collect()
        } else {
            self.project_sessions()
        }
    }

    fn move_session(&mut self, by: isize) -> bool {
        let n = self.listed_sessions().len();
        if n == 0 {
            return false;
        }
        let new = (self.workspace.list_selected as isize + by).clamp(0, n as isize - 1) as usize;
        self.workspace.list_selected = new;
        self.show_listed_session();
        true
    }

    /// Shows the selected list entry in the pane (without focusing it).
    pub(super) fn show_listed_session(&mut self) {
        let list = self.listed_sessions();
        self.workspace.active = list
            .get(
                self.workspace
                    .list_selected
                    .min(list.len().saturating_sub(1)),
            )
            .copied();
    }

    fn focus_listed_session(&mut self) -> bool {
        self.show_listed_session();
        if let Some(id) = self.workspace.active {
            self.focus_session(id);
        }
        true
    }

    /// Shows and focuses session `id`, selecting its project (FR-010).
    pub fn focus_session(&mut self, id: SessionId) {
        let Some(project) = self.sessions.get(id).map(|s| s.project) else {
            return;
        };
        if let Some(i) = self.projects.projects.iter().position(|p| p.id == project) {
            self.projects.selected = Some(i);
        }
        if self.workspace.split == Some(id) {
            self.workspace.split = self.workspace.active;
        }
        self.workspace.active = Some(id);
        self.workspace.view = View::Agents;
        self.workspace.list_selected = self
            .listed_sessions()
            .iter()
            .position(|s| *s == id)
            .unwrap_or(0);
        self.set_session_focus(true);
    }

    /// Next or previous session across all projects (FR-010).
    fn cycle_session(&mut self, by: isize) -> bool {
        let all: Vec<SessionId> = self.sessions.sessions.iter().map(|s| s.id).collect();
        if all.is_empty() {
            self.say("No sessions: n starts one");
            return true;
        }
        let cur = self
            .workspace
            .active
            .and_then(|a| all.iter().position(|s| *s == a))
            .unwrap_or(0) as isize;
        let next = all[(cur + by).rem_euclid(all.len() as isize) as usize];
        self.focus_session(next);
        true
    }

    /// `s`: shows the selected session beside the active one, or ends the split.
    fn toggle_split(&mut self) -> bool {
        if self.workspace.split.take().is_some() {
            return true;
        }
        let list = self.listed_sessions();
        let pick = list.get(self.workspace.list_selected).copied();
        let other = match (pick, self.workspace.active) {
            (Some(p), Some(a)) if p != a => Some(p),
            (_, Some(a)) => list.iter().copied().find(|s| *s != a),
            _ => None,
        };
        match other {
            Some(o) => self.workspace.split = Some(o),
            None => self.say("Split needs two sessions"),
        }
        true
    }

    /// `Shift-K`: asks before stopping the selected session (FR-012, 001 FR-032).
    fn ask_stop(&mut self) -> bool {
        let Some(id) = self.workspace.active else {
            return false;
        };
        let Some(s) = self.sessions.get(id) else {
            return false;
        };
        if !s.state.running() {
            self.say("The session is not running");
            return true;
        }
        let procs = self.session_metrics(id).map_or(1, |m| m.2);
        let how = if cfg!(windows) {
            "The agent's console is closed so it can clean up."
        } else {
            "The agent's process group gets SIGTERM so it can clean up."
        };
        let d = crate::ui::confirm::Dialog::new(
            "Stop session",
            &crate::session::stop_subject(id),
            vec![
                format!(
                    "{}  ({} process{})",
                    s.label(),
                    procs,
                    if procs == 1 { "" } else { "es" }
                ),
                how.into(),
                "If it is still running after 5 s, you will be asked whether to force it.".into(),
            ],
            "Stop",
        );
        self.overlay = Overlay::Confirm {
            dialog: d,
            action: super::editing::Pending::StopSession(id),
        };
        true
    }

    /// Sessions that were asked to stop 5 s ago and still run get the force dialog (FR-012),
    /// and a pending quit completes once every agent is gone.
    pub(super) fn tick_stops(&mut self) -> bool {
        let mut changed = false;
        if let Some(at) = self.quitting {
            let all_gone = self.sessions.running() == 0;
            if all_gone || at.elapsed() >= std::time::Duration::from_secs(5) {
                self.sessions.kill_all();
                self.quit = true;
                return true;
            }
        }
        if !matches!(self.overlay, Overlay::None) {
            return false;
        }
        let late = self.sessions.sessions.iter().find(|s| {
            s.state == SessionState::Stopping
                && s.stop_requested
                    .is_some_and(|at| at.elapsed() >= std::time::Duration::from_secs(5))
        });
        if let Some(s) = late {
            let id = s.id;
            let label = s.label();
            let d = crate::ui::confirm::Dialog::new(
                "Force stop",
                &crate::session::force_subject(id),
                vec![
                    format!("{label} is still running 5 s after the stop request."),
                    "Force stop ends it and everything it started immediately. It cannot clean up."
                        .into(),
                ],
                "Force stop",
            );
            if let Some(s) = self.sessions.get_mut(id) {
                s.stop_requested = None;
            }
            self.overlay = Overlay::Confirm {
                dialog: d,
                action: super::editing::Pending::ForceStopSession(id),
            };
            changed = true;
        }
        changed
    }

    /// `p`: shows the session's agent in the LLM panel (FR-013).
    fn jump_to_process(&mut self) -> bool {
        let Some(id) = self.workspace.active else {
            return false;
        };
        let key = self
            .snapshot
            .as_deref()
            .and_then(|s| s.sessions.iter().find(|(_, v)| **v == id).map(|(k, _)| *k));
        let Some(key) = key else {
            self.say("The session's agent is not in the process list yet");
            return true;
        };
        self.workspace.focus = AgentsFocus::Processes;
        if let Some(i) = self.panel_index_of(key) {
            self.select(i);
        }
        true
    }

    /// `j` in the LLM panel: from a session's agent to its session (FR-013).
    fn jump_to_session(&mut self) -> bool {
        let Some(key) = self.selected_process() else {
            return false;
        };
        let id = self
            .snapshot
            .as_deref()
            .and_then(|s| s.sessions.get(&key).copied());
        match id {
            Some(id) => {
                self.focus_session(id);
                self.set_session_focus(false);
            }
            None => self.say("This process was not started as a Fallow session"),
        }
        true
    }

    fn restart_listed(&mut self) -> bool {
        let Some(id) = self.workspace.active else {
            return false;
        };
        let Some(tx) = self.events.clone() else {
            return false;
        };
        if let Err(e) = self.sessions.restart(id, &tx) {
            self.say(e);
        } else {
            self.track(id);
            self.set_session_focus(true);
        }
        true
    }

    fn close_listed(&mut self) -> bool {
        let Some(id) = self.workspace.active else {
            return false;
        };
        let wt = self.sessions.get(id).and_then(|s| match &s.workdir {
            Workdir::Worktree(w) if !s.state.running() => Some(w.clone()),
            _ => None,
        });
        if let Some(w) = wt {
            let clean = w.path.is_dir() && worktree::is_clean(&w.path).unwrap_or(false);
            let dialog = if clean {
                let mut d = crate::ui::confirm::Dialog::new(
                    "Close session",
                    &w.path.display().to_string(),
                    vec![
                        w.path.display().to_string(),
                        format!(
                            "Keep the worktree, or remove it? The branch {} is kept either way.",
                            w.branch
                        ),
                    ],
                    "Remove worktree",
                );
                d.cancel_label = "Keep worktree".into();
                d
            } else {
                crate::ui::confirm::Dialog::new(
                    "Close session",
                    &w.path.display().to_string(),
                    vec![
                        w.path.display().to_string(),
                        "The worktree is kept: it has uncommitted changes or untracked files (or is missing).".into(),
                    ],
                    "Close session",
                )
            };
            self.overlay = Overlay::Confirm {
                dialog,
                action: super::editing::Pending::CloseSession {
                    id,
                    offer_remove: clean,
                },
            };
            return true;
        }
        self.close_session(id)
    }

    /// Closes an exited session (the worktree, if any, stays).
    pub(super) fn close_session(&mut self, id: SessionId) -> bool {
        match self.sessions.close(id) {
            Ok(()) => {
                self.workspace.list_selected = self.workspace.list_selected.saturating_sub(1);
                self.show_listed_session();
            }
            Err(e) => self.say(e),
        }
        true
    }

    fn scroll_active(&mut self, back: bool) -> bool {
        let page = ws_ui::session_rect(self).height.max(1) as usize;
        let Some(s) = self
            .workspace
            .active
            .and_then(|id| self.sessions.get_mut(id))
        else {
            return false;
        };
        let max = s.term.lock().map(|mut t| t.scrollback_len()).unwrap_or(0);
        s.scroll = if back {
            (s.scroll + page).min(max)
        } else {
            s.scroll.saturating_sub(page)
        };
        true
    }

    /// Opens the start menu for the selected project: agents (availability resolved now, never
    /// at startup, FR-023) and, for git projects, where the agent runs (FR-031, FR-032).
    fn open_profile_menu(&mut self) -> bool {
        let Some(p) = self.projects.selected() else {
            self.say("Add a project first: Ctrl-O");
            return true;
        };
        let path = p.path.clone();
        let agents: Vec<MenuItem> = self
            .profiles
            .iter()
            .map(|p| MenuItem {
                id: p.id.clone(),
                label: p.name.clone(),
                available: profiles::resolve(&p.command).map(|_| ()),
            })
            .collect();
        let mut wheres = Vec::new();
        let mut error = None;
        if worktree::is_git_project(&path) {
            if self.sessions.limits.worktrees {
                wheres.push(Where::NewWorktree);
                wheres.push(Where::ProjectFolder);
            } else {
                wheres.push(Where::ProjectFolder);
                wheres.push(Where::NewWorktree);
            }
            match worktree::list(&path) {
                Ok(list) => wheres.extend(list.into_iter().map(Where::Existing)),
                Err(e) => error = Some(format!("Existing worktrees not listed: {e}")),
            }
        }
        let agent = agents.iter().position(|a| a.available.is_ok()).unwrap_or(0);
        self.overlay = Overlay::Menu(StartMenu {
            agents,
            agent,
            wheres,
            where_sel: 0,
            on_where: false,
            error,
        });
        true
    }

    pub(super) fn menu_key(&mut self, key: KeyEvent) -> bool {
        let Overlay::Menu(m) = &mut self.overlay else {
            return false;
        };
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Tab | KeyCode::BackTab if !m.wheres.is_empty() => m.on_where = !m.on_where,
            KeyCode::Up if m.on_where => m.where_sel = m.where_sel.saturating_sub(1),
            KeyCode::Down if m.on_where => {
                m.where_sel = (m.where_sel + 1).min(m.wheres.len().saturating_sub(1));
            }
            KeyCode::Up => m.agent = m.agent.saturating_sub(1),
            KeyCode::Down => m.agent = (m.agent + 1).min(m.agents.len().saturating_sub(1)),
            KeyCode::Enter => {
                let item = m.agents.get(m.agent).cloned();
                let place = m
                    .wheres
                    .get(m.where_sel)
                    .cloned()
                    .unwrap_or(Where::ProjectFolder);
                let Some(item) = item else {
                    return true;
                };
                if let Err(e) = item.available {
                    m.error = Some(e);
                    return true;
                }
                match self.start_session_at(&item.id, place) {
                    Ok(()) => {}
                    Err(e) => {
                        // US6 scenario 7: keep the menu, offer the project folder.
                        if let Overlay::Menu(m) = &mut self.overlay {
                            m.error =
                                Some(format!("{e}. Choose \"project folder\" to start there."));
                            m.where_sel = m
                                .wheres
                                .iter()
                                .position(|w| *w == Where::ProjectFolder)
                                .unwrap_or(0);
                            m.on_where = true;
                        }
                        return true;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Starts a session of profile `id` in the project folder (tests and plain folders).
    pub fn start_session(&mut self, id: &str) {
        if let Err(e) = self.start_session_at(id, Where::ProjectFolder) {
            self.say(e);
        }
    }

    /// Starts a session of profile `id` in the selected project, in `place`, and focuses it.
    /// A worktree is created first when asked for (FR-031). Errors before the agent starts are
    /// returned; a failure to start the agent itself shows in the session.
    pub fn start_session_at(&mut self, id: &str, place: Where) -> Result<(), String> {
        let profile = self
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("no such profile")?;
        let p = self.projects.selected().ok_or("no project selected")?;
        let tx = self
            .events
            .clone()
            .ok_or("Sessions are not available in this mode")?;
        let (project, project_name, project_path) = (p.id, p.name.clone(), p.path.clone());
        let root = self.sessions.limits.worktree_root.clone();
        let (cwd, workdir) = match place {
            Where::ProjectFolder => (project_path.clone(), Workdir::ProjectFolder),
            Where::Existing(w) => (w.path.clone(), Workdir::Worktree(w)),
            Where::NewWorktree => {
                let branch = worktree::next_branch(&project_path, &profile.id, root.as_deref())?;
                let w = worktree::create(&project_path, &branch, root.as_deref())
                    .map_err(|e| format!("Could not create a worktree: {e}"))?;
                (w.path.clone(), Workdir::Worktree(w))
            }
        };
        let area = ws_ui::session_rect(self);
        let req = StartRequest {
            project,
            project_name,
            cwd,
            workdir,
            profile,
            rows: area.height.max(1),
            cols: area.width.max(1),
        };
        let sid = self.sessions.start(req, &tx)?;
        self.track(sid);
        self.overlay = Overlay::None;
        self.workspace.active = Some(sid);
        self.workspace.list_selected = self
            .project_sessions()
            .iter()
            .position(|s| *s == sid)
            .unwrap_or(0);
        self.workspace.view = View::Agents;
        self.set_session_focus(true);
        if let Some(SessionState::Failed(e)) = self.sessions.get(sid).map(|s| s.state.clone()) {
            self.say(e);
        }
        Ok(())
    }

    /// New output from a session. Returns true if it is on screen.
    /// New output from a session. Returns true only if the screen must be redrawn: the session
    /// is on screen, or its state (shown in the lists) changed. Output of hidden sessions does
    /// not redraw the screen (002 SC-006 investigation: 7 flooding hidden sessions caused ~28
    /// full redraws per second).
    pub fn session_output(&mut self, id: SessionId) -> bool {
        let before = self.sessions.get(id).map(|s| s.state.clone());
        let (_signals, entered) = self.sessions.output(id);
        if entered {
            self.attention_notice(id);
        }
        let after = self.sessions.get(id).map(|s| s.state.clone());
        let visible = self.workspace.view == View::Agents
            && (self.workspace.active == Some(id) || self.workspace.split == Some(id));
        visible || before != after
    }

    /// FR-009: tell the user when a session they are not looking at needs them.
    fn attention_notice(&mut self, id: SessionId) {
        let watching = self.workspace.view == View::Agents
            && self.workspace.focus == AgentsFocus::Session
            && self.workspace.active == Some(id);
        if watching {
            return;
        }
        if let Some(s) = self.sessions.get(id) {
            let label = s.label();
            self.say(format!("{label} needs attention (Ctrl-T: all sessions)"));
        }
    }

    /// The agent group's memory (bytes), CPU (% of one core) and process count from the latest
    /// snapshot (R-8).
    pub fn session_metrics(&self, id: SessionId) -> Option<(u64, f32, usize)> {
        let snap = self.snapshot.as_deref()?;
        let key = snap
            .sessions
            .iter()
            .find(|(_, s)| **s == id)
            .map(|(k, _)| *k)?;
        let g = snap.groups.iter().find(|g| g.root == key)?;
        Some((g.total_footprint, g.total_cpu, g.members.len() + 1))
    }

    /// The quiet-fallback check for every running session (R-4). Returns true if a session
    /// changed state.
    pub fn tick_sessions(&mut self) -> bool {
        let delay = self.sessions.limits.attention_delay;
        let now = Instant::now();
        let cpus: Vec<(SessionId, Option<f32>)> = self
            .sessions
            .sessions
            .iter()
            .map(|s| (s.id, self.session_metrics(s.id).map(|m| m.1)))
            .collect();
        let mut changed = Vec::new();
        for (id, cpu) in cpus {
            if let Some(s) = self.sessions.get_mut(id) {
                if crate::session::attention::on_tick(s, cpu, delay, now) {
                    changed.push(id);
                }
            }
        }
        for id in &changed {
            self.attention_notice(*id);
        }
        !changed.is_empty()
    }

    /// Asks procmon to track a session's agent (R-8).
    fn track(&self, id: SessionId) {
        if let (Some(tx), Some(pid)) = (&self.requests, self.sessions.get(id).and_then(|s| s.pid)) {
            let _ = tx.send(crate::procmon::worker::Request::Track { pid, session: id });
        }
    }

    pub fn session_exited(&mut self, id: SessionId, code: Option<u32>) -> bool {
        if let Some(pid) = self.sessions.exited(id, code) {
            if let Some(tx) = &self.requests {
                let _ = tx.send(crate::procmon::worker::Request::Untrack { pid });
            }
        }
        if self.workspace.active == Some(id) && self.workspace.focus == AgentsFocus::Session {
            self.workspace.focus = AgentsFocus::Sessions;
        }
        true
    }

    /// Copies the selection in the active session (001 clipboard).
    fn copy_session_selection(&mut self) -> bool {
        let Some(s) = self.workspace.active.and_then(|id| self.sessions.get(id)) else {
            return false;
        };
        let Some(((l0, c0), (l1, c1))) = s.selection else {
            self.say("Quit: Ctrl-Q");
            return true;
        };
        let text = match s.term.lock() {
            Ok(mut t) => selection_text(&mut t, (l0, c0), (l1, c1)),
            Err(_) => return false,
        };
        if self.clipboard.copy(&text).failed() {
            self.say("Copy failed: clipboard unavailable");
        }
        true
    }

    /// Scrolls the active session so virtual line `v` is visible.
    pub(super) fn search_session(&mut self, query: &str) -> Result<(), String> {
        let rows = ws_ui::session_rect(self).height.max(1) as usize;
        let s = self
            .workspace
            .active
            .and_then(|id| self.sessions.get_mut(id))
            .ok_or("No session")?;
        let mut t = s.term.lock().map_err(|_| "busy")?;
        let total = t.total_lines();
        let top = total.saturating_sub(rows + s.scroll);
        let hit = t
            .search_up(query, top)
            .or_else(|| t.search_up(query, total))
            .ok_or_else(|| format!("{query:?} not found in the scrollback"))?;
        s.scroll = total.saturating_sub(rows).saturating_sub(hit.min(total));
        let len = query.chars().count() as u16;
        let col = t
            .line_text(hit)
            .find(query)
            .map_or(0, |b| t.line_text(hit)[..b].chars().count() as u16);
        s.selection = Some(((hit, col), (hit, col + len)));
        Ok(())
    }

    /// Mouse over the agent workspace. `None`: not handled here.
    pub(super) fn workspace_mouse(&mut self, m: MouseEvent) -> Option<bool> {
        if self.workspace.view != View::Agents || !matches!(self.overlay, Overlay::None) {
            return None;
        }
        let g = ws_ui::geometry(self);
        let (x, y) = (m.column, m.row);
        let inside =
            |r: ratatui::layout::Rect| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
        let pane = ws_ui::session_rect(self);
        // The agent asked for mouse reports: forward them over its pane.
        if inside(pane) && self.workspace.focus == AgentsFocus::Session {
            if let Some(s) = self.workspace.active.and_then(|id| self.sessions.get(id)) {
                let bytes = s
                    .term
                    .lock()
                    .ok()
                    .and_then(|t| ws_ui::mouse_report(t.screen(), &m, x - pane.x, y - pane.y));
                if let Some(b) = bytes {
                    s.write(&b);
                    return Some(false);
                }
            }
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside(g.projects) {
                    self.workspace.focus = AgentsFocus::Projects;
                    if let Some(i) = ws_ui::list_row(g.projects, y) {
                        self.select_project(i);
                    }
                    return Some(true);
                }
                if inside(g.list) {
                    if let Some(i) = ws_ui::list_row(g.list, y) {
                        if i < self.project_sessions().len() {
                            self.workspace.list_selected = i;
                            self.show_listed_session();
                        }
                    }
                    self.workspace.focus = AgentsFocus::Sessions;
                    return Some(true);
                }
                if inside(g.procs) || inside(g.system) {
                    self.workspace.focus = AgentsFocus::Processes;
                    if let Some(i) = crate::ui::processes::row_at(&g.procs, self.panel.selected, y)
                    {
                        self.select(i);
                    }
                    return Some(true);
                }
                if inside(pane) {
                    let v = self.virtual_line_at(y - pane.y);
                    if let Some(s) = self
                        .workspace
                        .active
                        .and_then(|id| self.sessions.get_mut(id))
                    {
                        s.selection = None;
                        self.workspace.drag_from = v.map(|l| (l, x - pane.x));
                    }
                    if self.workspace.active.is_some() {
                        self.set_session_focus(true);
                    }
                    return Some(true);
                }
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let from = self.workspace.drag_from?;
                let row = y.clamp(pane.y, pane.bottom().saturating_sub(1)) - pane.y;
                let to = self.virtual_line_at(row)?;
                let col = x.saturating_sub(pane.x).min(pane.width);
                let s = self
                    .workspace
                    .active
                    .and_then(|id| self.sessions.get_mut(id))?;
                let (a, b) = if (to, col) < from {
                    ((to, col), from)
                } else {
                    (from, (to, col + 1))
                };
                s.selection = Some((a, b));
                Some(true)
            }
            MouseEventKind::Up(_) => {
                self.workspace.drag_from = None;
                Some(false)
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if inside(pane) => {
                let s = self
                    .workspace
                    .active
                    .and_then(|id| self.sessions.get_mut(id))?;
                let max = s.term.lock().map(|mut t| t.scrollback_len()).unwrap_or(0);
                s.scroll = if m.kind == MouseEventKind::ScrollUp {
                    (s.scroll + 3).min(max)
                } else {
                    s.scroll.saturating_sub(3)
                };
                Some(true)
            }
            _ => None,
        }
    }

    /// Before drawing: the shown session's terminal has the pane's size (FR-002).
    pub fn prepare_workspace(&mut self) {
        if self.workspace.view != View::Agents {
            return;
        }
        let r = ws_ui::session_rect(self);
        if let Some(s) = self
            .workspace
            .active
            .and_then(|id| self.sessions.get_mut(id))
        {
            s.resize(r.height.max(1), r.width.max(1));
        }
        if let (Some(r), Some(id)) = (ws_ui::split_rect(self), self.workspace.split) {
            if let Some(s) = self.sessions.get_mut(id) {
                s.resize(r.height.max(1), r.width.max(1));
            }
        }
    }

    /// The virtual line under pane row `row` of the active session.
    fn virtual_line_at(&self, row: u16) -> Option<usize> {
        let s = self.workspace.active.and_then(|id| self.sessions.get(id))?;
        let mut t = s.term.lock().ok()?;
        let rows = t.size().0 as usize;
        let total = t.total_lines();
        Some(total.saturating_sub(rows + s.scroll) + row as usize)
    }
}

/// The text between two (virtual line, column) positions.
pub fn selection_text(
    t: &mut crate::session::term::Term,
    (l0, c0): (usize, u16),
    (l1, c1): (usize, u16),
) -> String {
    let mut out = String::new();
    for l in l0..=l1 {
        let line = t.line_text(l);
        let chars: Vec<char> = line.chars().collect();
        let from = if l == l0 { c0 as usize } else { 0 };
        let to = if l == l1 {
            (c1 as usize).min(chars.len())
        } else {
            chars.len()
        };
        if from < to {
            out.extend(&chars[from..to]);
        }
        if l != l1 {
            out.push('\n');
        }
    }
    out
}
