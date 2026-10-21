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
        let changed = hub.handle(ev, &self.sessions, Instant::now());
        self.take_hub_notices();
        changed
    }

    /// The hub's periodic work, from the main loop's tick.
    pub fn tick_hub(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let changed = hub.tick(&self.sessions, Instant::now());
        self.take_hub_notices();
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
        if id == "orch.open" {
            self.overlay = Overlay::Orch(OrchView::default());
            return Some(true);
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
            "orch.up" if v.tab == Tab::Queue => v.selected = v.selected.saturating_sub(1),
            "orch.down" if v.tab == Tab::Queue => v.selected += 1,
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
                let i = v.selected;
                let keys = crate::ui::orchestration::events_shown(self);
                if let (Some(k), Some(h)) = (keys.get(i), self.hub.as_mut()) {
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
