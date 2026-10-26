//! The lead drives the squad (004 US7, FR-032 to FR-038, research R-10).
//!
//! Everything that needs action in a burst is told to the lead: Fallow queues a line per event and
//! types them as one "Squad update" into the lead's session while it is idle, at most one every
//! ten seconds, ending with what the merge still needs. Without a running lead the lines go to the
//! engineer instead. The engineer is told only what is theirs to decide (`alerts`).

use std::time::{Duration, Instant};

use super::gates::GATE_ROLE;
use super::{Hub, Party, Sessions, WAITING_FOR_APPROVAL, delivery, tasks};
use crate::projects::ProjectId;

/// At most one squad update every this long (FR-033).
pub const GROUP_EVERY: Duration = Duration::from_secs(10);
/// A status for the lead after this long without an update while the merge is not ready (FR-037).
pub const QUIET_STATUS: Duration = Duration::from_secs(10 * 60);

impl Hub {
    /// Queues a line for the lead of `project`'s burst (FR-032).
    pub(crate) fn tell_lead(&mut self, project: ProjectId, line: String) {
        if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
            b.lead_updates.push(line);
        }
    }

    /// What the merge still needs, in one line.
    fn merge_summary(&self, project: ProjectId) -> String {
        let Some(r) = self.readiness(project) else {
            return String::new();
        };
        if r.ready {
            return match &r.commit {
                Some(c) => format!("Merge: ready ({}).", &c[..c.len().min(10)]),
                None => "Merge: ready.".into(),
            };
        }
        let unmet: Vec<String> = r
            .checks
            .iter()
            .filter(|c| !c.met)
            .map(|c| format!("{} ({})", c.condition, c.why))
            .collect();
        format!("Merge still needs: {}.", unmet.join("; "))
    }

    /// Open work in a burst, for the quiet-time status.
    fn open_work(&self, project: ProjectId) -> Vec<String> {
        let Some(b) = self.burst_of(project) else {
            return Vec::new();
        };
        self.tasks
            .list
            .iter()
            .filter(|t| t.created_at >= b.started && !t.state.finished())
            .map(|t| {
                format!(
                    "T{} {} ({})",
                    t.id,
                    t.state.label(),
                    t.assigned
                        .or(match t.target {
                            tasks::Target::Session(s) => Some(s),
                            tasks::Target::NextFree { .. } => None,
                        })
                        .map_or("unassigned".into(), |s| format!("#{s}"))
                )
            })
            .collect()
    }

    /// Prompts seen in members' sessions, merge readiness, the quiet-time status, and the
    /// grouped update itself (called from `tick`).
    pub(crate) fn tick_lead(&mut self, sessions: &dyn Sessions, now: Instant) -> bool {
        let mut changed = false;
        for i in 0..self.bursts.len() {
            let (project, lead) = (self.bursts[i].project, self.bursts[i].lead);
            // Members stuck at their agent's own permission prompt (FR-035).
            let members: Vec<(u32, String)> = self.bursts[i]
                .roles
                .iter()
                .filter(|(id, r)| **id != lead && *r != GATE_ROLE)
                .map(|(id, r)| (*id, r.clone()))
                .collect();
            for (id, role) in members {
                let stuck = sessions
                    .info(id)
                    .is_some_and(|s| s.running && s.state == WAITING_FOR_APPROVAL);
                let b = &mut self.bursts[i];
                if stuck && b.prompt_told.insert(id) {
                    b.lead_updates.push(format!(
                        "#{id} {role} is stuck at its agent's own permission prompt; only the user can answer it in that session. Ask the user, or give the work to another session."
                    ));
                } else if !stuck {
                    b.prompt_told.remove(&id);
                }
            }
            // The merge became ready (FR-038): the lead and the engineer.
            let ready = self.readiness(project).is_some_and(|r| r.ready);
            if ready && !self.bursts[i].ready_told {
                self.bursts[i].ready_told = true;
                let summary = self.merge_summary(project);
                self.bursts[i].lead_updates.push(format!(
                    "Every merge condition holds. {summary} The work is finished: tell the user it is ready to merge."
                ));
                let name = self.bursts[i].project_name.clone();
                self.alerts
                    .push(format!("{name}: ready to merge (Squad tab, m)"));
            } else if !ready {
                self.bursts[i].ready_told = false;
            }
            // Nothing for a long while and work open: a status (FR-037).
            let quiet = now.saturating_duration_since(self.bursts[i].last_update) >= QUIET_STATUS;
            if quiet && !ready && self.bursts[i].lead_updates.is_empty() {
                let open = self.open_work(project);
                let line = if open.is_empty() {
                    "Status: no task is open. Hand out the next part, or integrate and ask for a review.".to_string()
                } else {
                    format!("Status: open tasks: {}.", open.join(", "))
                };
                self.bursts[i].lead_updates.push(line);
                self.bursts[i].last_update = now;
            }
            // The grouped update (FR-033).
            if self.bursts[i].lead_updates.is_empty()
                || now.saturating_duration_since(self.bursts[i].last_sent) < GROUP_EVERY
            {
                continue;
            }
            let lead_info = sessions.info(lead).filter(|s| s.running);
            match lead_info {
                None => {
                    // No lead to tell: the engineer hears it (they can name a new lead).
                    let lines = std::mem::take(&mut self.bursts[i].lead_updates);
                    self.notices.extend(lines);
                    changed = true;
                }
                Some(s) if s.idle && s.state != WAITING_FOR_APPROVAL => {
                    let lines = std::mem::take(&mut self.bursts[i].lead_updates);
                    let mut text = String::from("Squad update:");
                    for l in &lines {
                        text.push_str("\n- ");
                        text.push_str(l);
                    }
                    let summary = self.merge_summary(project);
                    if !summary.is_empty() {
                        text.push('\n');
                        text.push_str(&summary);
                    }
                    let _ = self.request_delivery(
                        sessions,
                        Party::Fallow,
                        lead,
                        text,
                        delivery::Purpose::Message,
                        true,
                        now,
                    );
                    self.bursts[i].last_sent = now;
                    self.bursts[i].last_update = now;
                    changed = true;
                }
                Some(_) => {}
            }
        }
        changed
    }
}
