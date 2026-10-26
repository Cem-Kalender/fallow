//! Merge conditions, readiness and publishing (004 US3, US6, FR-027, FR-028).
//!
//! Readiness is derived from the hub's state each time it is asked for; nothing an agent says
//! makes a condition hold except signals from the right role, tasks reported done and gate runs
//! Fallow observed itself (SC-003).

use std::collections::BTreeSet;

use super::burst::Burst;
use super::gates::Kind;
use super::squad::{Condition, Publish};
use super::{Hub, tasks};
use crate::projects::ProjectId;

/// How the work is published, resolved for a burst (FR-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DraftPr,
    LocalMerge,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::DraftPr => "draft-pr",
            Mode::LocalMerge => "local-merge",
        }
    }
}

pub fn mode(b: &Burst) -> Mode {
    match (b.squad.publish, b.has_remote) {
        (Publish::DraftPr, _) | (Publish::Auto, true) => Mode::DraftPr,
        _ => Mode::LocalMerge,
    }
}

/// One merge condition now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub condition: String,
    pub met: bool,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub checks: Vec<Check>,
    /// Every condition holds.
    pub ready: bool,
    /// The commit the final gates ran on, if any.
    pub commit: Option<String>,
    pub mode: Mode,
}

pub fn condition_label(c: &Condition) -> String {
    match c {
        Condition::Gate(g) => format!("gate:{g}"),
        Condition::AllDone(r) => format!("done:{r}"),
        Condition::Signal { event, role } => format!("signal:{event}@{role}"),
    }
}

/// Sessions in `role` whose signals count (FR-013).
fn counting(b: &Burst, role: &str) -> BTreeSet<u32> {
    b.roles
        .iter()
        .filter(|(id, r)| *r == role && !b.tainted.contains_key(id))
        .map(|(id, _)| *id)
        .collect()
}

impl Hub {
    /// The merge readiness of a project's burst (data-model "MergeReadiness").
    pub fn readiness(&self, project: ProjectId) -> Option<Readiness> {
        let b = self.burst_of(project)?;
        let mut checks = Vec::new();
        for c in &b.squad.merge_requires {
            let (met, why) = match c {
                Condition::Gate(g) => {
                    match b
                        .gate_runs
                        .iter()
                        .rev()
                        .find(|r| &r.gate == g && r.kind == Kind::Final)
                    {
                        None => (false, "the final run has not happened yet".to_string()),
                        Some(r) => match r.class {
                            None => (false, r.state.label()),
                            Some(c) => (c.opens(), format!("{}: {}", r.state.label(), c.label())),
                        },
                    }
                }
                Condition::AllDone(role) => {
                    let members: BTreeSet<u32> = b
                        .roles
                        .iter()
                        .filter(|(_, r)| *r == role)
                        .map(|(id, _)| *id)
                        .collect();
                    let handed: Vec<&tasks::Task> = self
                        .tasks
                        .list
                        .iter()
                        .filter(|t| {
                            t.created_at >= b.started
                                && t.assigned.is_some_and(|a| members.contains(&a))
                        })
                        .collect();
                    let open: Vec<String> = handed
                        .iter()
                        .filter(|t| t.state != tasks::State::Done)
                        .map(|t| format!("T{} {}", t.id, t.state.label()))
                        .collect();
                    if handed.is_empty() {
                        (false, format!("no task was handed to a {role} yet"))
                    } else if open.is_empty() {
                        (true, format!("{} task(s) done", handed.len()))
                    } else {
                        (false, open.join(", "))
                    }
                }
                Condition::Signal { event, role } => {
                    let who = counting(b, role);
                    let signed = b.signed.get(event).cloned().unwrap_or_default();
                    let missing: Vec<String> = who
                        .iter()
                        .filter(|id| !signed.contains(id))
                        .map(|id| format!("#{id}"))
                        .collect();
                    if who.is_empty() {
                        (false, format!("no {role} session whose signals count"))
                    } else if missing.is_empty() {
                        (true, format!("signalled by every {role}"))
                    } else {
                        (false, format!("waiting for {}", missing.join(", ")))
                    }
                }
            };
            checks.push(Check {
                condition: condition_label(c),
                met,
                why,
            });
        }
        let commit = b
            .gate_runs
            .iter()
            .rev()
            .find(|r| r.kind == Kind::Final)
            .and_then(|r| r.commit.clone());
        Some(Readiness {
            ready: checks.iter().all(|c| c.met),
            checks,
            commit,
            mode: mode(b),
        })
    }

    /// Records who signalled what during a burst, for `signal:` conditions. A reviewer's latest
    /// verdict replaces its earlier one (review-approved / review-changes-requested).
    pub(crate) fn record_signal(&mut self, session: u32, event: &str) {
        let Some(b) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&session))
        else {
            return;
        };
        let opposite = match event {
            "review-approved" => Some("review-changes-requested"),
            "review-changes-requested" => Some("review-approved"),
            _ => None,
        };
        if let Some(o) = opposite {
            if let Some(s) = b.signed.get_mut(o) {
                s.remove(&session);
            }
        }
        b.signed
            .entry(event.to_string())
            .or_default()
            .insert(session);
    }
}
