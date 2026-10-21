//! Standing permissions (003 FR-015 to FR-017; constitution IV).
//!
//! A permission lets requests of certain kinds from a source to a target through without a
//! prompt, for a limited time:
//! - **Scope**: source and target are each one session, or any session of a project.
//! - **Actions**: deliver messages, create tasks, start sessions up to a limit. Stop, pause,
//!   resume, kill and re-prioritize have no variant: no permission can cover them.
//! - **Time**: at most 8 hours.
//!
//! Every use is announced; revoking takes effect before the next request (SC-005).

use std::time::{Duration, Instant};

use super::SessionInfo;
use crate::projects::ProjectId;

pub const MAX_DURATION: Duration = Duration::from_secs(8 * 3600);
pub const MIN_DURATION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Session(u32),
    Project(ProjectId),
}

impl Scope {
    pub fn matches(self, s: &SessionInfo) -> bool {
        match self {
            Scope::Session(id) => s.id == id,
            Scope::Project(p) => s.project == p,
        }
    }
}

/// What a permission can allow. There is deliberately no variant for any destructive action
/// (FR-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Deliver,
    CreateTask,
    StartSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// Shown as `P1`, `P2`, …
    pub id: u32,
    pub source: Scope,
    pub target: Scope,
    pub deliver: bool,
    pub create_task: bool,
    /// Sessions it may start; `None`: none.
    pub start_limit: Option<u8>,
    pub granted: Instant,
    pub expires: Instant,
    pub uses: u32,
    pub starts_used: u8,
}

impl Permission {
    pub fn allows(&self, act: Act) -> bool {
        match act {
            Act::Deliver => self.deliver,
            Act::CreateTask => self.create_task,
            Act::StartSession => self.start_limit.is_some_and(|l| self.starts_used < l),
        }
    }

    /// "deliver, tasks, start ≤2"
    pub fn actions_label(&self) -> String {
        let mut v = Vec::new();
        if self.deliver {
            v.push("deliver".to_string());
        }
        if self.create_task {
            v.push("tasks".to_string());
        }
        if let Some(l) = self.start_limit {
            v.push(format!("start {}/{l}", self.starts_used));
        }
        v.join(", ")
    }
}

/// What the engineer grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub source: Scope,
    pub target: Scope,
    pub deliver: bool,
    pub create_task: bool,
    pub start_limit: Option<u8>,
    pub duration: Duration,
}

#[derive(Debug, Default)]
pub struct Permissions {
    pub list: Vec<Permission>,
    next: u32,
}

impl Permissions {
    /// Grants a permission. The duration must be 1 minute ..= 8 hours and at least one action
    /// must be allowed.
    pub fn grant(&mut self, g: &Grant, now: Instant) -> Result<u32, String> {
        if !(MIN_DURATION..=MAX_DURATION).contains(&g.duration) {
            return Err("a permission lasts 1 minute to 8 hours".into());
        }
        if !g.deliver && !g.create_task && g.start_limit.is_none_or(|l| l == 0) {
            return Err("allow at least one action".into());
        }
        self.next += 1;
        self.list.push(Permission {
            id: self.next,
            source: g.source,
            target: g.target,
            deliver: g.deliver,
            create_task: g.create_task,
            start_limit: g.start_limit.filter(|l| *l > 0),
            granted: now,
            expires: now + g.duration,
            uses: 0,
            starts_used: 0,
        });
        Ok(self.next)
    }

    /// The first permission covering `act` from `source` to `target` now, used (its count goes
    /// up). For starts, `target` is a session-like view of the project the session would start
    /// in, with id 0.
    pub fn cover(
        &mut self,
        source: &SessionInfo,
        target: &SessionInfo,
        act: Act,
        now: Instant,
    ) -> Option<u32> {
        let p = self.list.iter_mut().find(|p| {
            now < p.expires
                && p.allows(act)
                && p.source.matches(source)
                && match (act, p.target) {
                    // A start has no target session yet: only a project target can cover it.
                    (Act::StartSession, Scope::Session(_)) => false,
                    _ => p.target.matches(target),
                }
        })?;
        p.uses += 1;
        if act == Act::StartSession {
            p.starts_used += 1;
        }
        Some(p.id)
    }

    pub fn revoke(&mut self, id: u32) -> Option<Permission> {
        let i = self.list.iter().position(|p| p.id == id)?;
        Some(self.list.remove(i))
    }

    pub fn revoke_all(&mut self) -> Vec<Permission> {
        std::mem::take(&mut self.list)
    }

    /// Removes and returns the permissions that expired.
    pub fn expire(&mut self, now: Instant) -> Vec<Permission> {
        let (gone, keep): (Vec<_>, Vec<_>) = self.list.drain(..).partition(|p| now >= p.expires);
        self.list = keep;
        gone
    }
}
