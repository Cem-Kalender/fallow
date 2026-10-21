//! Orchestration in the app (003): creating the hub with the first orchestrated session,
//! routing channel events to it, and its periodic work.

use std::time::Instant;

use super::state::AppState;
use crate::orchestration::channel::{self, Incoming};
use crate::orchestration::identity::Peer;
use crate::orchestration::inject::Bridge;
use crate::orchestration::{Hub, SessionInfo, Sessions, tools};
use crate::session::profiles::Style;
use crate::session::{SessionManager, SessionState};

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
