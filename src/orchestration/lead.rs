//! The lead drives the squad (004 US7, FR-032 to FR-038, research R-10).
//!
//! Everything that needs action in a burst is told to the lead: Fallow queues a line per event and
//! types them as one "Squad update" into the lead's session while it is idle, at most one every
//! ten seconds, ending with what the merge still needs. Without a running lead the lines go to the
//! engineer instead. The engineer is told only what is theirs to decide (`alerts`).

use std::time::{Duration, Instant};

use super::gates::GATE_ROLE;
use super::{Hub, Party, Sessions, StartState, WAITING_FOR_APPROVAL, delivery, tasks};
use crate::projects::ProjectId;

/// At most one squad update every this long (FR-033).
pub const GROUP_EVERY: Duration = Duration::from_secs(10);
/// A status for the lead after this long without an update while the merge is not ready (FR-037).
pub const QUIET_STATUS: Duration = Duration::from_secs(10 * 60);

/// A line of an update: agents' text (notes, reasons) on one line, so it cannot start a line
/// that looks like Fallow's own (review).
pub fn one_line(s: &str) -> String {
    s.split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// The update typed into the lead and the lines left for the next one: as many lines as fit in
/// a delivery with the merge summary (a single line longer than that is cut); the rest wait
/// (verification D: nothing is dropped).
pub fn update_text(lines: &[String], summary: &str) -> (String, Vec<String>) {
    let max = super::delivery::TEXT_MAX - 200 - summary.len();
    let mut text = String::from("Squad update:");
    let mut rest = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let mut l = l.clone();
        if i == 0 && text.len() + l.len() + 3 > max {
            let mut cut = max.saturating_sub(text.len() + 10);
            while !l.is_char_boundary(cut) {
                cut -= 1;
            }
            l.truncate(cut);
            l.push_str(" ...");
        } else if text.len() + l.len() + 3 > max {
            rest = lines[i..].to_vec();
            break;
        }
        text.push_str("\n- ");
        text.push_str(&l);
    }
    if !rest.is_empty() {
        text.push_str(&format!("\n- ({} more in the next update)", rest.len()));
    }
    if !summary.is_empty() {
        text.push('\n');
        text.push_str(summary);
    }
    (text, rest)
}

impl Hub {
    /// Queues a line for the lead of `project`'s burst (FR-032).
    pub(crate) fn tell_lead(&mut self, project: ProjectId, line: String) {
        if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
            b.lead_updates.push(one_line(&line));
            // Something happened: the next quiet-time status is news again (review).
            b.last_status.clear();
        }
    }

    /// `tell_lead` for the app (a burst's start after a restart, 005 FR-036).
    pub fn tell_lead_pub(&mut self, project: ProjectId, line: String) {
        self.tell_lead(project, line);
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

    /// What only the user can clear: a start waiting for their approval, of a role the merge
    /// still needs and that has no running session (004 run 3). A missing role with no starts
    /// left is the lead's to request: its start then waits for the user (review).
    pub(crate) fn user_blocker(
        &self,
        sessions: &dyn Sessions,
        project: ProjectId,
    ) -> Option<String> {
        let b = self.burst_of(project)?;
        let needed = self.needed_roles(b);
        let running = |role: &str| {
            b.roles
                .iter()
                .any(|(id, r)| r == role && sessions.info(*id).is_some_and(|s| s.running))
        };
        self.starts
            .iter()
            .find(|s| {
                s.project == project
                    && s.state == StartState::Pending
                    && s.role
                        .as_ref()
                        .is_some_and(|r| needed.contains(r) && !running(r))
            })
            .map(|s| {
                format!(
                    "starting a {} session the merge needs waits for the user's approval (o)",
                    s.role.as_deref().unwrap_or_default()
                )
            })
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
            // Nothing for a long while and work open: a status (FR-037). Not in an adaptive squad:
            // the lead hears only what needs a decision (005 FR-035).
            let quiet = !self.bursts[i].squad.adaptive()
                && now.saturating_duration_since(self.bursts[i].last_update) >= QUIET_STATUS;
            if quiet && !ready && self.bursts[i].lead_updates.is_empty() {
                self.bursts[i].last_update = now;
                if let Some(why) = self.user_blocker(sessions, project) {
                    // Only the user can clear it: they are told, and the lead once, instead of
                    // nudging the lead every ten minutes (004 run 3).
                    if self.bursts[i].user_told.as_deref() != Some(why.as_str()) {
                        let name = self.bursts[i].project_name.clone();
                        self.alerts.push(format!("{name} waits for you: {why}"));
                        self.bursts[i].lead_updates.push(format!(
                            "Status: the squad waits for the user: {why}. Fallow told the user."
                        ));
                        self.bursts[i].user_told = Some(why);
                    }
                } else {
                    self.bursts[i].user_told = None;
                    let open = self.open_work(project);
                    let missing = self.missing_roles(sessions, &self.bursts[i]);
                    let line = if !missing.is_empty() {
                        format!(
                            "Status: the merge needs a {} session and none is running: start one with session_start{}.",
                            missing.join(" and a "),
                            if self.starts_left(&self.bursts[i]) == 0 {
                                " (no starts are left, so it waits for the user's approval)"
                            } else {
                                ""
                            }
                        )
                    } else if open.is_empty() {
                        "Status: no task is open. Hand out the next part, or integrate and ask for a review.".to_string()
                    } else {
                        format!("Status: open tasks: {}.", open.join(", "))
                    };
                    // The same status again is a nudge, not news: sent once until it changes.
                    if line != self.bursts[i].last_status {
                        self.bursts[i].last_status = line.clone();
                        self.bursts[i].lead_updates.push(line);
                    }
                }
            }
            // Lead updates switched off for a recorded run (005 FR-017): nothing is typed.
            if self.bursts[i]
                .switched_off
                .contains(super::burst::OFF_LEAD_UPDATES)
            {
                self.bursts[i].lead_updates.clear();
                continue;
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
                    // No lead to tell: the engineer hears it as one notice (only the last
                    // notice is shown), and every line goes to the activity log.
                    let lines = std::mem::take(&mut self.bursts[i].lead_updates);
                    for l in &lines {
                        self.activity
                            .add(Party::Fallow, super::activity::Action::Post, l, None);
                    }
                    let name = self.bursts[i].project_name.clone();
                    let all = lines.join(" | ");
                    self.alerts.push(format!(
                        "{name} has no running lead (L in the Squad tab names one): {}",
                        all.chars().take(1500).collect::<String>()
                    ));
                    changed = true;
                }
                Some(s) if s.idle && s.state != WAITING_FOR_APPROVAL => {
                    let lines = std::mem::take(&mut self.bursts[i].lead_updates);
                    let summary = self.merge_summary(project);
                    let (text, rest) = update_text(&lines, &summary);
                    let sent = self.request_delivery(
                        sessions,
                        Party::Fallow,
                        lead,
                        text,
                        delivery::Purpose::Message,
                        true,
                        now,
                    );
                    // Lines that did not fit, or all of them if typing was refused, wait for the
                    // next update: none is lost (FR-033).
                    let keep = if sent.is_err() { lines } else { rest };
                    if !keep.is_empty() {
                        let b = &mut self.bursts[i];
                        let newer = std::mem::take(&mut b.lead_updates);
                        b.lead_updates = keep;
                        b.lead_updates.extend(newer);
                    }
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
