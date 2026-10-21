//! Orchestration in the app (003): creating the hub with the first orchestrated session,
//! routing channel events to it, and its periodic work.

use std::time::Instant;

use crossterm::event::KeyEvent;

use super::commands::Context;
use super::editing::PromptKind;
use super::keymap;
use super::state::{AppState, Overlay};
use crate::editor::selection::LineInput;
use crate::orchestration::channel::{self, Incoming};
use crate::orchestration::identity::Peer;
use crate::orchestration::inject::Bridge;
use crate::orchestration::{Hub, SessionInfo, Sessions, tools};
use crate::session::profiles::Style;
use crate::session::{SessionManager, SessionState};
use crate::ui::orchestration::{OrchView, Tab};

impl Sessions for SessionManager {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        let s = self.get(id)?;
        Some(SessionInfo {
            id: s.id,
            project: s.project,
            project_name: s.project_name.clone(),
            profile_id: s.profile.id.clone(),
            profile: s.profile.name.clone(),
            state: s.state.label(),
            running: s.state.running(),
            idle: s.state == SessionState::NeedsAttention,
        })
    }

    fn all(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .filter_map(|s| self.info(s.id))
            .collect()
    }

    fn verify(&self, id: u32, token: &str, peer: Peer) -> Result<(), String> {
        let s = self.get(id).ok_or("no such session")?;
        if !s.state.running() {
            return Err("the session is not running".into());
        }
        let t = s
            .orch
            .token
            .as_ref()
            .ok_or("the session runs without orchestration")?;
        if !t.matches(token) {
            return Err("wrong token".into());
        }
        #[cfg(unix)]
        if peer.uid != Some(crate::orchestration::identity::my_uid()) {
            return Err("another user's process".into());
        }
        if !s.contains(peer.pid) {
            return Err(format!("process {} is not part of the session", peer.pid));
        }
        Ok(())
    }
}

/// The hub's view of the app: the sessions, plus the project list for tools that name a project.
pub struct AppView<'a> {
    pub sessions: &'a SessionManager,
    pub projects: &'a crate::projects::ProjectList,
}

impl Sessions for AppView<'_> {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.sessions.info(id)
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.sessions.all()
    }
    fn verify(&self, id: u32, token: &str, peer: Peer) -> Result<(), String> {
        self.sessions.verify(id, token, peer)
    }
    fn projects(&self) -> Vec<(crate::projects::ProjectId, String, bool)> {
        self.projects
            .projects
            .iter()
            .map(|p| (p.id, p.name.clone(), p.present))
            .collect()
    }
}

impl AppState {
    /// Creates the hub and opens the channel when the first session that would use them is
    /// about to start (FR-005, constitution II). Problems are remembered and shown once; the
    /// session then starts without orchestration and says why.
    pub fn ensure_hub(&mut self, style: Style) {
        if style == Style::None || self.hub.is_some() {
            return;
        }
        if !self.orch_settings.enabled {
            self.sessions.orch_unavailable = Some("disabled in settings".into());
            return;
        }
        if self.sessions.orch_unavailable.as_deref() == Some("disabled in settings") {
            self.sessions.orch_unavailable = None;
        }
        if self.sessions.orch_unavailable.is_some() {
            return;
        }
        let Some(tx) = self.events.clone() else {
            return;
        };
        let run_dir = crate::config::run_dir();
        let opened = match &run_dir {
            Some(dir) => {
                #[cfg(windows)]
                let _ = std::fs::create_dir_all(dir);
                channel::open(dir, tools::list(), tx)
            }
            None => Err("no cache folder is known (HOME or LOCALAPPDATA unset)".into()),
        };
        let exe =
            std::env::current_exe().map_err(|e| format!("cannot find Fallow's own program: {e}"));
        match (opened, exe) {
            (Ok(ch), Ok(exe)) => {
                self.sessions.bridge = Some(Bridge {
                    exe,
                    endpoint: ch.endpoint().to_string(),
                    fallow_pid: std::process::id(),
                    run_dir,
                });
                self.hub = Some(Hub::with_channel(self.orch_settings.clone(), ch));
            }
            (Err(e), _) | (_, Err(e)) => {
                self.notices
                    .push(format!("Agent orchestration is unavailable: {e}"));
                self.sessions.orch_unavailable = Some(e);
            }
        }
    }

    /// An event from the channel.
    pub fn hub_event(&mut self, ev: Incoming) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            // No hub: refuse a hello so the bridge does not wait.
            if let Incoming::Hello { verdict, .. } = ev {
                let _ = verdict.send(Err("orchestration is off".into()));
            }
            return false;
        };
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
        };
        let changed = hub.handle(ev, &view, Instant::now());
        self.take_hub_notices();
        changed
    }

    /// The hub's periodic work, from the main loop's tick.
    pub fn tick_hub(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
        };
        let mut changed = hub.tick(&view, Instant::now());
        changed |= self.type_deliveries();
        self.take_hub_notices();
        changed
    }

    /// Types approved deliveries into their sessions, except while the engineer is typing in
    /// the target (FR-014): then it waits until they leave or pause for the delivery delay.
    fn type_deliveries(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let delay = std::time::Duration::from_millis(self.orch_settings.delivery_delay_ms);
        let mut changed = false;
        for id in hub.ready() {
            let Some(d) = hub.deliveries.get(id).cloned() else {
                continue;
            };
            let focused = self.workspace.view == super::workspace::View::Agents
                && self.workspace.focus == super::workspace::AgentsFocus::Session
                && self.workspace.active == Some(d.to);
            let result = match self.sessions.get(d.to) {
                None => Err("target exited".to_string()),
                Some(s) => {
                    if focused && s.last_key.is_some_and(|t| t.elapsed() < delay) {
                        continue;
                    }
                    match hub.take_grant(id) {
                        Some(g) => s.deliver(g, &d),
                        None => continue,
                    }
                }
            };
            hub.delivered(id, result);
            changed = true;
        }
        changed
    }

    fn take_hub_notices(&mut self) {
        let Some(hub) = self.hub.as_mut() else {
            return;
        };
        if let Some(last) = hub.notices.last().cloned() {
            hub.notices.clear();
            self.say(last);
        }
    }
}

impl AppState {
    /// Orchestration commands (contracts/ui-commands.md). `None`: not one of them.
    pub(crate) fn run_orch(&mut self, id: &str) -> Option<bool> {
        let pending = self.hub.as_ref().map_or(0, |h| h.pending_requests());
        match id {
            // Opens on Requests when requests wait (contracts/ui-commands.md).
            "orch.open" | "orch.requests" => {
                let tab = if id == "orch.requests" || pending > 0 {
                    Tab::Requests
                } else {
                    Tab::Board
                };
                self.overlay = Overlay::Orch(OrchView {
                    tab,
                    ..OrchView::default()
                });
                return Some(true);
            }
            // Reachable from the palette anywhere (SC-005: one command).
            "orch.revoke_all" => {
                let n = self.hub.as_mut().map_or(0, |h| h.revoke_all());
                self.say(format!(
                    "Revoked {n} standing permission{}",
                    if n == 1 { "" } else { "s" }
                ));
                return Some(true);
            }
            "orch.task_new" if !matches!(self.overlay, Overlay::Orch(_)) => {
                return Some(self.new_task_prompt(OrchView {
                    tab: Tab::Queue,
                    ..OrchView::default()
                }));
            }
            "orch.grant_new" if !matches!(self.overlay, Overlay::Orch(_)) => {
                self.overlay = Overlay::Orch(OrchView {
                    tab: Tab::Permissions,
                    ..OrchView::default()
                });
                return Some(self.open_grant(None));
            }
            _ => {}
        }
        if id == "session.details" {
            return Some(self.session_details());
        }
        let Overlay::Orch(v) = &mut self.overlay else {
            return None;
        };
        let page = self.area.height.saturating_sub(8).max(1) as usize;
        match id {
            "orch.close" => self.overlay = Overlay::None,
            "orch.tab_next" | "orch.tab_prev" => {
                v.tab = v.tab.next(if id == "orch.tab_next" { 1 } else { -1 });
                v.scroll = 0;
                v.selected = 0;
            }
            "orch.board_toggle" if v.tab == Tab::Board => {
                v.shared = !v.shared;
                v.scroll = 0;
            }
            // Lists: Up/Down move the selection.
            "orch.up" if v.tab != Tab::Board && v.tab != Tab::Activity => {
                v.selected = v.selected.saturating_sub(1);
            }
            "orch.down" if v.tab != Tab::Board && v.tab != Tab::Activity => v.selected += 1,
            "orch.approve" | "orch.reject" | "orch.edit" | "orch.grant"
                if v.tab == Tab::Requests =>
            {
                let (sel, view) = (v.selected, v.clone());
                let ids = crate::ui::orchestration::requests_shown(self);
                let Some(&r) = ids.get(sel.min(ids.len().saturating_sub(1))) else {
                    return Some(false);
                };
                use crate::ui::orchestration::RequestRef;
                match (id, r) {
                    ("orch.approve", RequestRef::Delivery(rid)) => {
                        if let Some(h) = self.hub.as_mut() {
                            if let Err(e) = h.approve(rid, None) {
                                self.say(e);
                            }
                        }
                    }
                    ("orch.approve" | "orch.reject", RequestRef::Task(t)) => {
                        if let Some(h) = self.hub.as_mut() {
                            h.decide_task(t, id == "orch.approve");
                        }
                    }
                    ("orch.reject", RequestRef::Delivery(rid)) => {
                        if let Some(h) = self.hub.as_mut() {
                            h.reject(rid);
                        }
                    }
                    ("orch.edit", RequestRef::Delivery(rid)) => {
                        let text = self
                            .hub
                            .as_ref()
                            .and_then(|h| h.deliveries.get(rid))
                            .map(|d| d.text.clone())
                            .unwrap_or_default();
                        self.overlay = Overlay::Prompt {
                            kind: PromptKind::EditDelivery(rid, view),
                            input: LineInput::selected(&text),
                            error: None,
                        };
                    }
                    ("orch.edit", RequestRef::Task(t)) => {
                        return Some(self.edit_task_prompt(t, true, view));
                    }
                    (_, r) => return Some(self.open_grant(Some(r))),
                }
            }
            "orch.edit" if v.tab == Tab::Queue => {
                let (sel, view) = (v.selected, v.clone());
                let tasks = crate::ui::orchestration::tasks_shown(self);
                let Some(&t) = tasks.get(sel) else {
                    return Some(false);
                };
                return Some(self.edit_task_prompt(t, false, view));
            }
            "orch.task_new" => {
                let view = OrchView {
                    tab: Tab::Queue,
                    ..v.clone()
                };
                return Some(self.new_task_prompt(view));
            }
            "orch.move_up" | "orch.move_down" | "orch.pause" if v.tab == Tab::Queue => {
                let sel = v.selected;
                let tasks = crate::ui::orchestration::tasks_shown(self);
                let Some(&t) = tasks.get(sel) else {
                    return Some(false);
                };
                if let Some(h) = self.hub.as_mut() {
                    match id {
                        "orch.pause" => h.toggle_pause(t),
                        _ => {
                            let step = if id == "orch.move_up" { -1 } else { 1 };
                            h.tasks.move_by(t, step);
                            if let Overlay::Orch(v) = &mut self.overlay {
                                v.selected = (sel as isize + step)
                                    .clamp(0, tasks.len() as isize - 1)
                                    as usize;
                            }
                        }
                    }
                }
            }
            "orch.delete" if v.tab == Tab::Queue => {
                let sel = v.selected;
                let tasks = crate::ui::orchestration::tasks_shown(self);
                if let (Some(&t), Some(h)) = (tasks.get(sel), self.hub.as_mut()) {
                    h.cancel_task(t);
                }
            }
            "orch.grant_new" => return Some(self.open_grant(None)),
            "orch.delete" if v.tab == Tab::Permissions => {
                let i = v.selected;
                if let Some(h) = self.hub.as_mut() {
                    if let Some(p) = h.permissions.list.get(i).map(|p| p.id) {
                        h.revoke(p);
                    }
                }
            }
            // Board: newest at the bottom, so "up" scrolls back in time.
            "orch.up" => v.scroll += 1,
            "orch.down" => v.scroll = v.scroll.saturating_sub(1),
            "orch.signal" => {
                let view = v.clone();
                let scope = self
                    .projects
                    .selected()
                    .map_or(crate::orchestration::board::BoardId::Shared, |p| {
                        crate::orchestration::board::BoardId::Project(p.id)
                    });
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::SignalEvent(scope, view),
                    input: LineInput::default(),
                    error: None,
                };
            }
            "orch.clear" if v.tab == Tab::Queue => {
                let i = v
                    .selected
                    .checked_sub(crate::ui::orchestration::tasks_shown(self).len());
                let keys = crate::ui::orchestration::events_shown(self);
                if let (Some(k), Some(h)) = (i.and_then(|i| keys.get(i)), self.hub.as_mut()) {
                    h.clear_event(k);
                }
            }
            "orch.page_up" => v.scroll += page,
            "orch.page_down" => v.scroll = v.scroll.saturating_sub(page),
            "orch.write" => {
                let view = v.clone();
                let (board, _) = crate::ui::orchestration::board_shown(self, &view);
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::BoardPost(board, view),
                    input: LineInput::default(),
                    error: None,
                };
            }
            _ => return Some(false),
        }
        Some(true)
    }

    /// Keys while the orchestration view is open.
    pub(crate) fn orch_key(&mut self, key: KeyEvent) -> bool {
        // Help, palette and quit stay reachable.
        if let Some(id) = keymap::lookup(&key, Context::Global, true) {
            if matches!(id, "palette" | "quit") {
                self.overlay = Overlay::None;
                return self.run(id);
            }
        }
        match keymap::lookup(&key, Context::Orchestration, false) {
            Some(id) => self.run(id),
            None => false,
        }
    }

    /// The selected session's link: state, client, reason (contracts/ui-commands.md
    /// "Session details").
    fn session_details(&mut self) -> bool {
        let Some(s) = self.workspace.active.and_then(|id| self.sessions.get(id)) else {
            return false;
        };
        let mut lines = vec![
            format!("Profile: {}", s.profile.name),
            format!("Project: {}", s.project_name),
            format!("Folder:  {}", s.cwd.display()),
            format!("State:   {}", s.state.label()),
        ];
        let link = self.hub.as_ref().and_then(|h| h.links.get(&s.id));
        let orch = match (&s.orch.unsupported, s.orch.token.is_some(), link) {
            (Some(why), ..) => format!("no orchestration ({why})"),
            (None, true, Some(l)) if l.conn.is_some() => match &l.client {
                Some((name, version)) => format!("connected ({name} {version})"),
                None => "connected".into(),
            },
            (None, true, _) => "offered; the agent has not connected yet".into(),
            (None, false, _) => "not running".into(),
        };
        lines.push(format!("Orchestration: {orch}"));
        self.overlay = Overlay::Info {
            title: s.label(),
            lines,
        };
        true
    }
}

impl AppState {
    fn new_task_prompt(&mut self, view: OrchView) -> bool {
        self.overlay = Overlay::Prompt {
            kind: PromptKind::NewTask(view),
            input: LineInput::default(),
            error: None,
        };
        true
    }

    fn edit_task_prompt(&mut self, t: u32, approve: bool, view: OrchView) -> bool {
        let text = self
            .hub
            .as_ref()
            .and_then(|h| h.tasks.get(t))
            .map(|t| t.text.clone())
            .unwrap_or_default();
        self.overlay = Overlay::Prompt {
            kind: PromptKind::EditTask(t, approve, view),
            input: LineInput::selected(&text),
            error: None,
        };
        true
    }

    /// The engineer's new task: `#2 text…` or `claude@api text…`, with optional `after:a,b`
    /// (events of the target's project).
    pub(crate) fn submit_new_task(&mut self, line: &str) -> Result<u32, String> {
        use crate::orchestration::tasks::Target;
        let mut words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 2 {
            return Err("Type the target, then the task: #2 merge feature/x after:pr-1".into());
        }
        let target_word = words.remove(0);
        let mut after = Vec::new();
        words.retain(|w| match w.strip_prefix("after:") {
            Some(list) => {
                after.extend(
                    list.split(',')
                        .filter(|n| !n.is_empty())
                        .map(str::to_string),
                );
                false
            }
            None => true,
        });
        let text = words.join(" ");
        let (target, project) = if let Some(n) = target_word.strip_prefix('#') {
            let id: u32 = n
                .parse()
                .map_err(|_| format!("{target_word}: not a session"))?;
            let s = self
                .sessions
                .get(id)
                .filter(|s| s.state.running())
                .ok_or(format!("session #{id} is not running"))?;
            (Target::Session(id), s.project)
        } else if let Some((profile, project)) = target_word.split_once('@') {
            let p = self
                .projects
                .projects
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(project))
                .ok_or(format!("no project named {project}"))?;
            (
                Target::NextFree {
                    profile: profile.to_string(),
                    project: p.id,
                    project_name: p.name.clone(),
                },
                p.id,
            )
        } else {
            return Err(format!(
                "{target_word}: a session (#2) or profile@project (claude@api)"
            ));
        };
        for n in &after {
            if !crate::orchestration::events::valid_name(n) {
                return Err(format!("after:{n}: letters, digits and . _ : / # - only"));
            }
        }
        let text =
            crate::orchestration::board::clean_text(&text, crate::orchestration::tasks::TEXT_MAX)
                .map_err(|(_, m)| m)?;
        let pre = after
            .into_iter()
            .map(|n| (crate::orchestration::board::BoardId::Project(project), n))
            .collect();
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
        };
        let hub = self
            .hub
            .as_mut()
            .ok_or("Start an agent session with orchestration first")?;
        let (id, _) = hub.create_task(
            &view,
            crate::orchestration::Party::User,
            text,
            target,
            pre,
            Instant::now(),
        )?;
        Ok(id)
    }

    /// Opens the grant dialog, pre-filled from a request or from the first running sessions.
    fn open_grant(&mut self, request: Option<crate::ui::orchestration::RequestRef>) -> bool {
        use crate::ui::orchestration::GrantForm;
        use crate::ui::orchestration::RequestRef;
        let back = match &self.overlay {
            Overlay::Orch(v) => v.clone(),
            _ => OrchView::default(),
        };
        let running: Vec<u32> = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state.running())
            .map(|s| s.id)
            .collect();
        let from_request = match request {
            Some(RequestRef::Delivery(r)) => self.hub.as_ref().and_then(|h| {
                let d = h.deliveries.get(r)?;
                Some((d.from.session().unwrap_or(d.to), d.to, false))
            }),
            Some(RequestRef::Task(t)) => self.hub.as_ref().and_then(|h| {
                let t = h.tasks.get(t)?;
                let from = t.created_by.session()?;
                let to = match &t.target {
                    crate::orchestration::tasks::Target::Session(id) => *id,
                    crate::orchestration::tasks::Target::NextFree { project, .. } => running
                        .iter()
                        .copied()
                        .find(|s| self.sessions.get(*s).is_some_and(|s| s.project == *project))
                        .unwrap_or(from),
                };
                Some((from, to, true))
            }),
            None => None,
        };
        let (from, to, tasks) = match from_request {
            Some(v) => v,
            None => {
                let Some(&a) = running.first() else {
                    self.say("Start agent sessions first");
                    return true;
                };
                (a, *running.get(1).unwrap_or(&a), false)
            }
        };
        self.overlay = Overlay::Grant(GrantForm {
            from,
            from_any: false,
            to,
            to_any: false,
            deliver: !tasks,
            tasks,
            starts: 0,
            minutes: self.orch_settings.permission_default_min,
            field: 0,
            request,
            back,
            error: None,
        });
        true
    }

    /// Keys in the grant dialog.
    pub(crate) fn grant_key(&mut self, key: KeyEvent) -> bool {
        use crate::orchestration::permissions::{Grant, Scope};
        use crossterm::event::KeyCode;
        let running: Vec<u32> = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state.running())
            .map(|s| s.id)
            .collect();
        let project_of = |id: u32| self.sessions.get(id).map(|s| s.project);
        let Overlay::Grant(g) = &mut self.overlay else {
            return false;
        };
        let cycle = |cur: u32, step: isize| -> u32 {
            match running.iter().position(|s| *s == cur) {
                Some(i) if !running.is_empty() => {
                    running[(i as isize + step).rem_euclid(running.len() as isize) as usize]
                }
                _ => running.first().copied().unwrap_or(cur),
            }
        };
        g.error = None;
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::Orch(g.back.clone());
                return true;
            }
            KeyCode::Up => g.field = g.field.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                g.field = (g.field + 1).min(crate::ui::orchestration::GRANT_FIELDS - 1)
            }
            KeyCode::Left | KeyCode::Right => {
                let step: isize = if key.code == KeyCode::Left { -1 } else { 1 };
                match g.field {
                    0 => g.from = cycle(g.from, step),
                    1 => g.to = cycle(g.to, step),
                    4 => g.starts = (g.starts as isize + step).clamp(0, 9) as u8,
                    5 => {
                        let unit = if g.minutes < 10 || (g.minutes == 10 && step < 0) {
                            1
                        } else {
                            5
                        };
                        g.minutes = (g.minutes as i64 + step as i64 * unit).clamp(1, 480) as u64;
                    }
                    _ => {}
                }
            }
            KeyCode::Char(' ') => match g.field {
                0 => g.from_any = !g.from_any,
                1 => g.to_any = !g.to_any,
                2 => g.deliver = !g.deliver,
                3 => g.tasks = !g.tasks,
                4 => g.starts = if g.starts == 0 { 1 } else { 0 },
                _ => {}
            },
            KeyCode::Enter => {
                let scope = |id: u32, any: bool| match (any, project_of(id)) {
                    (true, Some(p)) => Scope::Project(p),
                    _ => Scope::Session(id),
                };
                let grant = Grant {
                    source: scope(g.from, g.from_any),
                    target: scope(g.to, g.to_any),
                    deliver: g.deliver,
                    create_task: g.tasks,
                    start_limit: (g.starts > 0).then_some(g.starts),
                    duration: std::time::Duration::from_secs(g.minutes * 60),
                };
                let (request, back) = (g.request, g.back.clone());
                let request: Option<crate::ui::orchestration::RequestRef> = request;
                let Some(hub) = self.hub.as_mut() else {
                    self.overlay = Overlay::Orch(back);
                    return true;
                };
                match hub.grant(&grant, Instant::now()) {
                    Ok(p) => {
                        match request {
                            Some(crate::ui::orchestration::RequestRef::Delivery(r)) => {
                                let _ = hub.approve(r, None);
                            }
                            Some(crate::ui::orchestration::RequestRef::Task(t)) => {
                                hub.decide_task(t, true);
                            }
                            None => {}
                        }
                        self.overlay = Overlay::Orch(back);
                        self.say(format!("Granted P{p}"));
                    }
                    Err(e) => {
                        if let Overlay::Grant(g) = &mut self.overlay {
                            g.error = Some(e);
                        }
                    }
                }
                return true;
            }
            _ => return false,
        }
        true
    }
}
