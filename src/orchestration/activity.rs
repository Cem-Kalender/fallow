//! The bounded, memory-only activity log (003 FR-022, FR-023, data-model.md "ActivityEntry").

use std::collections::VecDeque;
use std::time::SystemTime;

use super::Party;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Connected,
    RefusedConnection,
    Post,
    Read,
    Signal,
    Wait,
    WaitEnded,
    DeliveryRequested,
    DeliveryOutcome,
    TaskChanged,
    PermissionGranted,
    PermissionUsed,
    PermissionRevoked,
    StartRequested,
    StartOutcome,
    Refused,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Connected => "connected",
            Action::RefusedConnection => "refused connection",
            Action::Post => "post",
            Action::Read => "read",
            Action::Signal => "signal",
            Action::Wait => "wait",
            Action::WaitEnded => "wait ended",
            Action::DeliveryRequested => "delivery requested",
            Action::DeliveryOutcome => "delivery",
            Action::TaskChanged => "task",
            Action::PermissionGranted => "permission granted",
            Action::PermissionUsed => "permission used",
            Action::PermissionRevoked => "permission revoked",
            Action::StartRequested => "start requested",
            Action::StartOutcome => "start",
            Action::Refused => "refused",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at: SystemTime,
    pub actor: Party,
    pub action: Action,
    /// At most 200 characters.
    pub subject: String,
    pub outcome: Option<String>,
}

/// The newest `cap` entries.
#[derive(Debug, Clone)]
pub struct Activity {
    entries: VecDeque<Entry>,
    cap: usize,
}

pub const SUBJECT_MAX: usize = 200;

impl Activity {
    pub fn new(cap: usize) -> Activity {
        Activity {
            entries: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        while self.entries.len() > self.cap {
            self.entries.pop_front();
        }
    }

    pub fn add(&mut self, actor: Party, action: Action, subject: &str, outcome: Option<String>) {
        if self.entries.len() >= self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            at: SystemTime::now(),
            actor,
            action,
            subject: clip(subject, SUBJECT_MAX),
            outcome: outcome.map(|o| clip(&o, SUBJECT_MAX)),
        });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Newest first.
    pub fn newest(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev()
    }

    /// Newest first, only entries whose actor is that session, or whose actor belongs to that
    /// project (by name).
    pub fn filtered<'a>(
        &'a self,
        session: Option<u32>,
        project: Option<&'a str>,
    ) -> impl Iterator<Item = &'a Entry> {
        self.newest()
            .filter(move |e| match (&e.actor, session, project) {
                (_, None, None) => true,
                (Party::Agent { session: s, .. }, Some(want), _) => *s == want,
                (Party::Agent { project: p, .. }, None, Some(want)) => p == want,
                (Party::User, ..) => false,
            })
    }
}

/// The first line of `text`, cut to `max` characters with "…".
pub fn clip(text: &str, max: usize) -> String {
    let first = text.lines().next().unwrap_or("");
    if first.chars().count() <= max && first.len() == text.len() {
        return first.to_string();
    }
    let mut s: String = first.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(session: u32, project: &str) -> Party {
        Party::Agent {
            session,
            profile: "Claude Code".into(),
            project: project.into(),
        }
    }

    #[test]
    fn keeps_the_newest_and_filters() {
        let mut a = Activity::new(3);
        a.add(agent(1, "api"), Action::Post, "one", None);
        a.add(agent(2, "web"), Action::Post, "two", None);
        a.add(Party::User, Action::Signal, "three", None);
        a.add(agent(1, "api"), Action::Read, "four", None);
        assert_eq!(a.len(), 3);
        let subjects: Vec<&str> = a.newest().map(|e| e.subject.as_str()).collect();
        assert_eq!(subjects, ["four", "three", "two"]);
        assert_eq!(a.filtered(Some(1), None).count(), 1);
        assert_eq!(a.filtered(None, Some("web")).count(), 1);
        assert_eq!(a.filtered(None, None).count(), 3);
    }

    #[test]
    fn subjects_are_clipped_to_one_line() {
        assert_eq!(clip("short", 200), "short");
        assert_eq!(clip("line one\nline two", 200), "line one…");
        let long = "x".repeat(300);
        let c = clip(&long, 200);
        assert_eq!(c.chars().count(), 200);
        assert!(c.ends_with('…'));
    }
}
