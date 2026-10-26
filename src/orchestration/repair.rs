//! Capped repair loops and the "everyone waits" notice (004 US5, FR-025 to FR-026, research R-7).
//!
//! A failure goes back to the session the part was handed to (FR-025a), or to the lead when it is
//! not tied to one part, as a task Fallow creates. After the squad's `repair_cap` rounds, Fallow
//! stops the loop and asks the engineer.

use std::time::{Duration, Instant};

use super::board::BoardId;
use super::gates::GATE_ROLE;
use super::{Hub, Party, Sessions, TEXT_BUDGET, tasks};
use crate::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairLoop {
    /// The part (a task's `part`); `None`: failures not tied to one part.
    pub part: Option<String>,
    pub owner: u32,
    pub rounds: u8,
    /// Rounds the engineer allowed beyond the cap ("hand it back once more").
    pub extra: u8,
    /// The cap was reached: waiting for the engineer.
    pub stopped: bool,
    /// The engineer took it over or cancelled the part: no more repair tasks.
    pub closed: Option<String>,
    pub last_reason: String,
}

impl RepairLoop {
    pub fn label(&self) -> String {
        self.part.clone().unwrap_or_else(|| "(whole work)".into())
    }
}

/// The engineer's decision for a stopped loop (contracts/ui-commands.md: r / t / Delete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Again,
    TakeOver,
    Cancel,
}

/// The repair task's text: what failed, and how to report the fix.
fn repair_text(part: &str, round: u8, cap: u8, reason: &str) -> String {
    format!(
        "Repair round {round} of {cap} for {part}: {reason}\nFix it in your worktree, commit, and report this task done with evidence (the commit, the commands you ran, the files changed)."
    )
}

const DEADLOCK_REPEAT: Duration = Duration::from_secs(30 * 60);

impl Hub {
    /// The session a part was handed to first in a burst (FR-025a).
    fn owner_of(&self, project: ProjectId, part: &str) -> Option<u32> {
        let b = self.burst_of(project)?;
        self.tasks
            .list
            .iter()
            .filter(|t| t.created_at >= b.started && t.part.as_deref() == Some(part))
            .find_map(|t| t.assigned)
    }

    /// A failure in a burst (a review asking for changes, a failed gate): a repair task for the
    /// owner, or the engineer's decision after the cap (FR-025).
    pub fn repair(
        &mut self,
        sessions: &dyn Sessions,
        project: ProjectId,
        part: Option<String>,
        reason: &str,
        now: Instant,
    ) {
        let owner = part.as_deref().and_then(|p| self.owner_of(project, p));
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        // A part nobody was handed goes to the lead, as a failure of the whole work.
        let part = part.filter(|_| owner.is_some());
        let owner = owner.unwrap_or(b.lead);
        let cap = b.squad.repair_cap;
        let i = match b.repairs.iter().position(|l| l.part == part) {
            Some(i) => i,
            None => {
                b.repairs.push(RepairLoop {
                    part: part.clone(),
                    owner,
                    rounds: 0,
                    extra: 0,
                    stopped: false,
                    closed: None,
                    last_reason: String::new(),
                });
                b.repairs.len() - 1
            }
        };
        let l = &mut b.repairs[i];
        l.last_reason = reason.to_string();
        if l.closed.is_some() {
            return;
        }
        let label = l.label();
        if l.rounds >= cap.saturating_add(l.extra) {
            l.stopped = true;
            let text = format!(
                "Repair cap reached for {label} ({} rounds): your decision in the Squad tab (r hand it back once more, t take it over, Delete cancel the part). Last failure: {reason}",
                l.rounds
            );
            self.boards.post(
                BoardId::Project(project),
                Party::Fallow,
                text.clone(),
                std::time::SystemTime::now(),
                TEXT_BUDGET,
            );
            // The user's decision: the lead brings it to them, and the Squad tab shows it.
            self.tell_lead(project, text.clone());
            self.alerts.push(text);
            return;
        }
        l.rounds += 1;
        let (round, owner, part) = (l.rounds, l.owner, l.part.clone());
        let text = repair_text(&label, round, cap.saturating_add(l.extra), reason);
        if let Ok((id, _)) = self.create_task(
            sessions,
            Party::Fallow,
            text,
            tasks::Target::Session(owner),
            Vec::new(),
            now,
        ) {
            if let Some(t) = self.tasks.get_mut(id) {
                t.approved_handover = true;
                t.part = part;
                t.repair_round = round;
            }
            self.tell_lead(
                project,
                format!("Repair round {round} for {label} goes to #{owner} (T{id}): {reason}"),
            );
        }
    }

    /// The engineer decides a stopped loop.
    pub fn repair_decide(
        &mut self,
        sessions: &dyn Sessions,
        project: ProjectId,
        index: usize,
        d: Decision,
        now: Instant,
    ) -> Result<(), String> {
        let b = self
            .bursts
            .iter_mut()
            .find(|b| b.project == project)
            .ok_or("No coordination in this project")?;
        let l = b.repairs.get_mut(index).ok_or("No such repair loop")?;
        if !l.stopped {
            return Err("This loop has not reached its cap".into());
        }
        let (part, reason) = (l.part.clone(), l.last_reason.clone());
        l.stopped = false;
        match d {
            Decision::Again => {
                l.extra += 1;
                self.repair(sessions, project, part, &reason, now);
            }
            Decision::TakeOver => {
                l.closed = Some("taken over by the user".into());
                self.notices.push(format!("You took over {}", l.label()));
            }
            Decision::Cancel => {
                l.closed = Some("cancelled by the user".into());
                let label = l.label();
                let open: Vec<u32> = self
                    .tasks
                    .list
                    .iter()
                    .filter(|t| t.part == part && !t.state.finished())
                    .map(|t| t.id)
                    .collect();
                for t in open {
                    self.cancel_task(t);
                }
                self.notices.push(format!("Cancelled {label}"));
            }
        }
        Ok(())
    }

    /// Everyone in a burst waits (an event, a blocked task, or idle with nothing to do) and no
    /// gate runs: after the squad's `wait_notice_min`, one notice says who waits for what,
    /// repeated at most every 30 minutes (FR-026, research R-7).
    pub(crate) fn tick_deadlock(&mut self, sessions: &dyn Sessions, now: Instant) -> bool {
        let mut notices = Vec::new();
        let busy: Vec<u32> = self
            .tasks
            .list
            .iter()
            .filter(|t| t.state == tasks::State::InProgress)
            .filter_map(|t| t.assigned)
            .collect();
        for i in 0..self.bursts.len() {
            let b = &self.bursts[i];
            let gate_running = b
                .gate_runs
                .iter()
                .any(|r| r.state == super::gates::RunState::Running);
            let mut members: Vec<(u32, String)> = b
                .roles
                .iter()
                .filter(|(_, r)| *r != GATE_ROLE)
                .filter_map(|(id, r)| {
                    sessions
                        .info(*id)
                        .filter(|s| s.running)
                        .map(|s| (*id, r.clone(), s.idle))
                })
                .filter_map(|(id, r, idle)| {
                    let what = match self.events.waiting(id, now) {
                        Some((event, _)) => Some(format!("#{id} {r} waits for {event}")),
                        None if idle && !busy.contains(&id) => Some(format!("#{id} {r} is idle")),
                        None => None,
                    };
                    what.map(|w| (id, w))
                })
                .collect();
            let all = b
                .roles
                .iter()
                .filter(|(id, r)| *r != GATE_ROLE && sessions.info(**id).is_some_and(|s| s.running))
                .count();
            members.sort_unstable();
            let limit = Duration::from_secs(u64::from(b.squad.wait_notice_min) * 60);
            let b = &mut self.bursts[i];
            if gate_running || all == 0 || members.len() < all {
                b.waiting_since = None;
                continue;
            }
            let since = *b.waiting_since.get_or_insert(now);
            let due = now.saturating_duration_since(since) >= limit
                && b.deadlock_notice
                    .is_none_or(|t| now.saturating_duration_since(t) >= DEADLOCK_REPEAT);
            if due {
                b.deadlock_notice = Some(now);
                let list: Vec<String> = members.into_iter().map(|(_, w)| w).collect();
                notices.push((
                    b.project,
                    format!(
                        "All {all} sessions in {} have waited {} min: {}",
                        b.project_name,
                        b.squad.wait_notice_min,
                        list.join("; ")
                    ),
                ));
            }
        }
        let changed = !notices.is_empty();
        for (project, n) in notices {
            self.tell_lead(project, n);
        }
        changed
    }
}
