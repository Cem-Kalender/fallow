//! The task queue (003 US4, FR-018 to FR-020, data-model.md "Task").
//!
//! Tasks are handed to a session, or to the next free session of a profile in a project, in
//! queue order. A task waits while any of its preconditions (events) is unsignalled, and is
//! handed over through a delivery under the US3 rules. The agent reports it done or failed.

use std::time::{Instant, SystemTime};

use super::Party;
use super::events::Key;
use crate::projects::ProjectId;

pub const TEXT_MAX: usize = 8 * 1024;
pub const NOTE_MAX: usize = 2 * 1024;
pub const PRECONDITIONS_MAX: usize = 8;
pub const TASKS_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Session(u32),
    /// The next free session of this profile (id) in this project.
    NextFree {
        profile: String,
        project: ProjectId,
        project_name: String,
    },
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Session(id) => format!("#{id}"),
            Target::NextFree {
                profile,
                project_name,
                ..
            } => format!("next free {profile} in {project_name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Created by an agent; waits for the engineer (US4 scenario 5).
    PendingApproval,
    Queued,
    Blocked,
    InProgress,
    Done,
    Failed(String),
    Cancelled(String),
}

impl State {
    pub fn label(&self) -> String {
        match self {
            State::PendingApproval => "waiting for approval".into(),
            State::Queued => "queued".into(),
            State::Blocked => "blocked".into(),
            State::InProgress => "in progress".into(),
            State::Done => "done".into(),
            State::Failed(_) => "failed".into(),
            State::Cancelled(why) => format!("cancelled ({why})"),
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self, State::Done | State::Failed(_) | State::Cancelled(_))
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    /// Shown as `T1`.
    pub id: u32,
    pub text: String,
    pub target: Target,
    pub preconditions: Vec<Key>,
    pub state: State,
    pub paused: bool,
    pub assigned: Option<u32>,
    /// The hand-over delivery, while it is pending or being typed.
    pub delivery: Option<u64>,
    pub note: Option<String>,
    pub created_by: Party,
    pub created: SystemTime,
    pub created_at: Instant,
    pub started: Option<SystemTime>,
    pub finished: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct Tasks {
    pub list: Vec<Task>,
    next: u32,
}

impl Tasks {
    pub fn add(
        &mut self,
        text: String,
        target: Target,
        preconditions: Vec<Key>,
        created_by: Party,
        approved: bool,
        now: Instant,
    ) -> Result<u32, String> {
        if self.list.iter().filter(|t| !t.state.finished()).count() >= TASKS_MAX {
            return Err(format!("{TASKS_MAX} tasks are open"));
        }
        self.next += 1;
        self.list.push(Task {
            id: self.next,
            text,
            target,
            preconditions,
            state: if approved {
                State::Queued
            } else {
                State::PendingApproval
            },
            paused: false,
            assigned: None,
            delivery: None,
            note: None,
            created_by,
            created: SystemTime::now(),
            created_at: now,
            started: None,
            finished: None,
        });
        // Keep at most TASKS_MAX in all: the oldest finished ones go first.
        while self.list.len() > TASKS_MAX {
            match self.list.iter().position(|t| t.state.finished()) {
                Some(i) => {
                    self.list.remove(i);
                }
                None => break,
            }
        }
        Ok(self.next)
    }

    pub fn get(&self, id: u32) -> Option<&Task> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Task> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    /// Moves a task one place up (`-1`) or down (`1`) in the queue.
    pub fn move_by(&mut self, id: u32, step: isize) {
        let Some(i) = self.list.iter().position(|t| t.id == id) else {
            return;
        };
        let j = i as isize + step;
        if j >= 0 && (j as usize) < self.list.len() {
            self.list.swap(i, j as usize);
        }
    }

    pub fn finish(&mut self, id: u32, state: State) {
        if let Some(t) = self.get_mut(id) {
            if !t.state.finished() {
                t.state = state;
                t.finished = Some(SystemTime::now());
                t.delivery = None;
            }
        }
    }
}
