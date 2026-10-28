//! Signals (005 research R-4, FR-021, FR-022): builders' structured questions and proposed
//! changes to the lead, and the lead's answers that point to commits. Kept in memory only
//! (constitution VII); the decisions themselves live in the repository.

use std::time::Instant;

/// What a builder raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A question the lead decides.
    Question,
    /// A proposed change to the plan, the manifest or a contract, with its diff.
    Change,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "question" => Kind::Question,
            "change" => Kind::Change,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Question => "question",
            Kind::Change => "change",
        }
    }
}

/// The lead's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    /// The commit that records the decision; `None` when the answer changes nothing.
    pub commit: Option<String>,
    /// For a signal Fallow raised about a non-lead change: the lead accepted the work.
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub id: u64,
    /// The raising session; for a signal Fallow raised (R-19), the session whose commit it is
    /// about.
    pub from: u32,
    /// Raised by Fallow itself about a non-lead change to the manifest, docs or a contract.
    pub by_fallow: bool,
    pub project: crate::projects::ProjectId,
    pub kind: Kind,
    pub work: Option<String>,
    pub part: Option<String>,
    pub text: String,
    pub diff: Option<String>,
    /// The commit a Fallow signal is about.
    pub commit: Option<String>,
    pub raised: Instant,
    pub answer: Option<Answer>,
    /// The engineer was told it is unanswered (FR-035).
    pub told_engineer: bool,
}

impl Signal {
    pub fn open(&self) -> bool {
        self.answer.is_none()
    }
}

#[derive(Debug, Default)]
pub struct Signals {
    pub list: Vec<Signal>,
    next: u64,
}

impl Signals {
    pub fn add(&mut self, mut s: Signal) -> u64 {
        self.next += 1;
        s.id = self.next;
        self.list.push(s);
        self.next
    }

    pub fn get(&self, id: u64) -> Option<&Signal> {
        self.list.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Signal> {
        self.list.iter_mut().find(|s| s.id == id)
    }

    /// Whether `session` waits on an open signal it raised.
    pub fn open_from(&self, session: u32) -> bool {
        self.list
            .iter()
            .any(|s| !s.by_fallow && s.from == session && s.open())
    }

    /// Signals raised per session (for run records, R-15).
    pub fn count_by_session(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for s in self.list.iter().filter(|s| !s.by_fallow) {
            match out.iter_mut().find(|(id, _)| *id == s.from) {
                Some(e) => e.1 += 1,
                None => out.push((s.from, 1)),
            }
        }
        out
    }

    /// Open signals of a project.
    pub fn open_in(&self, project: crate::projects::ProjectId) -> impl Iterator<Item = &Signal> {
        self.list
            .iter()
            .filter(move |s| s.project == project && s.open())
    }
}
