//! Capped repair loops and the "everyone waits" notice (004 US5, FR-025 to FR-026, research R-7).
//!
//! A failure (a review asking for changes, a failed final gate) counts a round for its part and
//! goes to the lead, who rules on what the reviewer left open and hands the repair to the part's
//! owner with `task_create` (004 run 2: repairs Fallow handed out itself crossed the lead's
//! rulings). After the squad's `repair_cap` rounds, Fallow stops the loop: tasks for that part are
//! refused until the engineer decides.

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
    /// A round was counted and its repair task is not created yet.
    pub pending: bool,
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

    /// A failure in a burst: a round for its part, told to the lead, or the engineer's decision
    /// after the cap (FR-025).
    pub fn repair(
        &mut self,
        _sessions: &dyn Sessions,
        project: ProjectId,
        part: Option<String>,
        reason: &str,
        _now: Instant,
    ) {
        let owner = part.as_deref().and_then(|p| self.owner_of(project, p));
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        // A part nobody was handed is a failure of the whole work.
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
                    pending: false,
                });
                b.repairs.len() - 1
            }
        };
        let l = &mut b.repairs[i];
        l.last_reason = super::lead::one_line(reason).chars().take(300).collect();
        if l.closed.is_some() {
            return;
        }
        let label = l.label();
        // A failure while a repair is still to be handed out (a second reviewer, a repeated
        // verdict) belongs to the same round; checked before the cap (verification A).
        if l.pending {
            return;
        }
        if l.rounds >= cap.saturating_add(l.extra) {
            l.stopped = true;
            l.pending = false;
            let text = format!(
                "Repair cap reached for {label} ({} rounds): the user decides in the Squad tab (r once more, t take it over, Delete cancel the part); tasks for it wait until then. Last failure: {reason}",
                l.rounds
            );
            self.boards.post(
                BoardId::Project(project),
                Party::Fallow,
                text.clone(),
                std::time::SystemTime::now(),
                TEXT_BUDGET,
            );
            self.tell_lead(project, text.clone());
            self.alerts.push(text);
            return;
        }
        l.rounds += 1;
        l.pending = true;
        let (round, of) = (l.rounds, cap.saturating_add(l.extra));
        let how = match &l.part {
            Some(p) => format!(
                "hand it to #{owner} with task_create and part \"{p}\", with your ruling on anything left open"
            ),
            None => "decide who fixes it and hand it out with task_create".to_string(),
        };
        self.tell_lead(
            project,
            format!(
                "Repair needed (round {round} of {of}) for {label}: {reason} Next: {how}; then ask for a re-review with a task for the reviewer."
            ),
        );
    }

    /// A task the lead creates for a part with a failure counted: it is that round's repair.
    /// Past the cap, or after the user took the part over or cancelled it, it is refused.
    pub(crate) fn repair_for_task(&self, caller: u32, part: Option<&str>) -> Result<u8, String> {
        let Some(part) = part else {
            return Ok(0);
        };
        let Some(b) = self.bursts.iter().find(|b| b.roles.contains_key(&caller)) else {
            return Ok(0);
        };
        let Some(l) = b.repairs.iter().find(|l| l.part.as_deref() == Some(part)) else {
            return Ok(0);
        };
        if l.stopped {
            return Err(format!(
                "the repair cap for {part} is reached: the user decides first (Squad tab)"
            ));
        }
        if let Some(why) = &l.closed {
            return Err(format!("{part}: {why}"));
        }
        Ok(if l.pending { l.rounds } else { 0 })
    }

    /// The lead's repair task exists: the round's repair is handed out.
    pub(crate) fn repair_handed(&mut self, caller: u32, part: Option<&str>) {
        let Some(part) = part else {
            return;
        };
        if let Some(l) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&caller))
            .and_then(|b| {
                b.repairs
                    .iter_mut()
                    .find(|l| l.part.as_deref() == Some(part))
            })
        {
            l.pending = false;
        }
    }

    /// The engineer decides a stopped loop.
    pub fn repair_decide(
        &mut self,
        _sessions: &dyn Sessions,
        project: ProjectId,
        index: usize,
        d: Decision,
        _now: Instant,
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
        let part = l.part.clone();
        let label = l.label();
        l.stopped = false;
        let line = match d {
            Decision::Again => {
                l.extra += 1;
                l.rounds += 1;
                l.pending = true;
                format!(
                    "The user allows one more repair round for {label}: hand it out with task_create{}. Last failure: {}",
                    part.as_ref()
                        .map_or(String::new(), |p| format!(" and part \"{p}\"")),
                    l.last_reason
                )
            }
            Decision::TakeOver => {
                l.closed = Some("taken over by the user".into());
                format!("The user takes over {label}: hand out nothing more for it.")
            }
            Decision::Cancel => {
                l.closed = Some("cancelled by the user".into());
                format!("The user cancelled {label}: its open tasks are cancelled.")
            }
        };
        if d == Decision::Cancel {
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
        }
        self.tell_lead(project, line.clone());
        self.notices.push(line);
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
            // Waiting for the user is told to the user (lead.rs), not as a deadlock (004 run 3).
            let user_blocked = self
                .user_blocker(sessions, self.bursts[i].project)
                .is_some();
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
            if gate_running || user_blocked || all == 0 || members.len() < all {
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
            // A nudge, not news: it does not make the quiet-time status new again (review).
            if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
                b.lead_updates.push(n);
            }
        }
        changed
    }
}
