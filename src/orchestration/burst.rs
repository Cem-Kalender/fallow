//! Coordination bursts (004 US1, research R-1, FR-001 to FR-004).
//!
//! A burst is 003's standing permission scoped to one project (deliver, create tasks, start up to
//! the allowance; at most 8 hours), a lead, and the squad frozen at its start. Ending it revokes
//! the permission, so every 003 path and its tests stay in force.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use super::activity::Action;
use super::board::BoardId;
use super::permissions::{Grant, MAX_DURATION, Scope};
use super::squad::Squad;
use super::{Hub, Party, Sessions, TEXT_BUDGET};
use crate::projects::ProjectId;

/// What the engineer chose when starting a burst.
#[derive(Debug, Clone)]
pub struct BurstRequest {
    pub lead: u32,
    pub duration: Duration,
    /// Sessions the burst may start; default 3 (FR-001).
    pub allowance: Option<u8>,
    pub squad: Squad,
    pub start_commit: Option<String>,
    pub target_branch: Option<String>,
    pub has_remote: bool,
    /// The project uses Spec Kit (a `.specify` folder).
    pub spec_kit: bool,
    /// 002's `sessions.max_sessions`.
    pub max_sessions: u8,
}

#[derive(Debug, Clone)]
pub struct Burst {
    pub project: ProjectId,
    pub project_name: String,
    pub lead: u32,
    pub permission: u32,
    pub allowance: u8,
    pub started: Instant,
    pub ends: Instant,
    pub squad: Squad,
    pub start_commit: Option<String>,
    pub target_branch: Option<String>,
    pub has_remote: bool,
    pub spec_kit: bool,
    /// Roles of the sessions in the burst, fixed for their life (FR-009).
    pub roles: HashMap<u32, String>,
    /// Read-only sessions whose worktree changed, and how (FR-013): their signals do not count.
    pub tainted: HashMap<u32, String>,
    /// Gate runs Fallow observed (FR-020, FR-021).
    pub gate_runs: Vec<super::gates::GateRun>,
    /// Who signalled which event during the burst, for `signal:` merge conditions.
    pub signed: HashMap<String, BTreeSet<u32>>,
    /// Each reviewer's latest verdict and notes (FR-027).
    pub verdicts: HashMap<u32, (String, String)>,
    /// The draft pull request the lead opened (draft-PR mode, FR-028).
    pub published: Option<String>,
    /// (gated commit, merge commit) Fallow merged on the engineer's press (local merge).
    pub merged: Vec<(String, String)>,
    /// Commits on the target branch made outside Fallow's merge, already reported (FR-028a).
    pub foreign_seen: BTreeSet<String>,
    /// Repair loops per part (FR-025).
    pub repairs: Vec<super::repair::RepairLoop>,
    /// Since when every session has waited, and when the engineer was last told (FR-026).
    pub waiting_since: Option<Instant>,
    pub deadlock_notice: Option<Instant>,
    /// The commit each session started from (its worktree's base), for evidence checks.
    pub bases: HashMap<u32, String>,
    /// Lines for the lead's next squad update (US7, FR-032).
    pub lead_updates: Vec<String>,
    /// When the lead was last given an update or status, and when one was last typed.
    pub last_update: Instant,
    pub last_sent: Instant,
    /// Members whose permission prompt the lead was told about.
    pub prompt_told: BTreeSet<u32>,
    /// The lead and the engineer were told the merge is ready.
    pub ready_told: bool,
    /// The last quiet-time status, sent once until it changes; and what the squad waits for
    /// that only the user can clear, once told (004 run 3).
    pub last_status: String,
    pub user_told: Option<String>,
    /// Sessions of the burst that exited: a `signal:` condition stops waiting for one that
    /// exited without that signal (004 run 3).
    pub exited: BTreeSet<u32>,
    /// The work manifest at the lead's latest commit, when it has one (005 R-1).
    pub manifest: Option<super::manifest::Manifest>,
    /// Reviewers' focuses (005 FR-031): session → focus from the manifest.
    pub focus: HashMap<u32, String>,
    /// What the burst is for: a recorded run's fixed description (005 R-13).
    pub description: Option<String>,
    /// The lead's commit the manifest was last read at, and the files tracked there.
    pub manifest_seen: Option<String>,
    pub manifest_files: Vec<String>,
    /// What each builder session is for: a piece of work and its part (005 R-2).
    pub assigned: HashMap<u32, super::delegation::Assignment>,
    /// The latest refused builder starts, for the Squad tab (005 FR-007).
    pub refused: Vec<String>,
    /// Components switched off for a recorded run (005 FR-017).
    pub switched_off: BTreeSet<&'static str>,
    pub(crate) warned: bool,
}

/// Components a recorded run can switch off (005 FR-017, research R-17).
pub const OFF_REVIEWER: &str = "reviewer";
pub const OFF_GATES: &str = "gates";
pub const OFF_CONTRACTS: &str = "contracts";
pub const OFF_PLAN_COMMIT: &str = "plan_commit_check";
pub const OFF_CRITERIA: &str = "criteria_precondition";
pub const OFF_OWNERSHIP: &str = "ownership_check";
pub const OFF_VERDICT: &str = "verdict_check";
pub const OFF_LEAD_UPDATES: &str = "lead_updates";
pub const OFF_PARALLEL: &str = "parallel_builders";
pub const COMPONENTS: &[&str] = &[
    OFF_REVIEWER,
    OFF_GATES,
    OFF_CONTRACTS,
    OFF_PLAN_COMMIT,
    OFF_CRITERIA,
    OFF_OWNERSHIP,
    OFF_VERDICT,
    OFF_LEAD_UPDATES,
    OFF_PARALLEL,
];

pub const DEFAULT_ALLOWANCE: u8 = 3;
const WARN_BEFORE: Duration = Duration::from_secs(600);

impl Hub {
    pub fn burst_of(&self, project: ProjectId) -> Option<&Burst> {
        self.bursts.iter().find(|b| b.project == project)
    }

    /// The burst a session takes part in, if any.
    pub fn burst_for(&self, session: u32) -> Option<&Burst> {
        self.bursts.iter().find(|b| b.roles.contains_key(&session))
    }

    /// The role Fallow assigned to a session (FR-009).
    pub fn role_of(&self, session: u32) -> Option<&str> {
        self.burst_for(session)
            .and_then(|b| b.roles.get(&session))
            .map(String::as_str)
    }

    fn grant_for(project: ProjectId, allowance: u8, d: Duration) -> Grant {
        Grant {
            source: Scope::Project(project),
            target: Scope::Project(project),
            deliver: true,
            create_task: true,
            start_limit: Some(allowance),
            duration: d,
        }
    }

    /// Starts a burst for the lead's project (FR-001, FR-002, FR-003).
    pub fn start_burst(
        &mut self,
        sessions: &dyn Sessions,
        r: BurstRequest,
        now: Instant,
    ) -> Result<(), String> {
        if r.duration > MAX_DURATION {
            return Err("A burst lasts at most 8 hours".into());
        }
        let lead = sessions
            .info(r.lead)
            .filter(|s| s.running)
            .ok_or("The lead must be a running session")?;
        if self.burst_of(lead.project).is_some() {
            return Err(format!("{} is already coordinating", lead.project_name));
        }
        let max = r.max_sessions.max(1);
        let allowance = r.allowance.unwrap_or(DEFAULT_ALLOWANCE.min(max));
        if !(1..=max).contains(&allowance) {
            return Err(format!(
                "Sessions to start: 1..={max} (sessions.max_sessions)"
            ));
        }
        let permission = self.grant(&Self::grant_for(lead.project, allowance, r.duration), now)?;
        let target_branch = r.squad.target_branch.clone().or(r.target_branch);
        let mut roles = HashMap::new();
        roles.insert(r.lead, "lead".to_string());
        let burst = Burst {
            project: lead.project,
            project_name: lead.project_name.clone(),
            lead: r.lead,
            permission,
            allowance,
            started: now,
            ends: now + r.duration,
            squad: r.squad,
            start_commit: r.start_commit,
            target_branch,
            has_remote: r.has_remote,
            spec_kit: r.spec_kit,
            roles,
            tainted: HashMap::new(),
            gate_runs: Vec::new(),
            signed: HashMap::new(),
            verdicts: HashMap::new(),
            published: None,
            merged: Vec::new(),
            foreign_seen: BTreeSet::new(),
            repairs: Vec::new(),
            waiting_since: None,
            deadlock_notice: None,
            bases: HashMap::new(),
            lead_updates: Vec::new(),
            last_update: now,
            last_sent: now.checked_sub(super::lead::GROUP_EVERY).unwrap_or(now),
            prompt_told: BTreeSet::new(),
            ready_told: false,
            last_status: String::new(),
            user_told: None,
            exited: BTreeSet::new(),
            manifest: None,
            focus: HashMap::new(),
            description: None,
            manifest_seen: None,
            manifest_files: Vec::new(),
            assigned: HashMap::new(),
            refused: Vec::new(),
            switched_off: BTreeSet::new(),
            warned: false,
        };
        // Reviewers that run commands can write outside their worktree, unseen (005 FR-030).
        if burst
            .squad
            .roles
            .iter()
            .any(|r| r.level == super::squad::Level::Check)
        {
            self.notices.push(format!(
                "{}: reviewers here may run commands; a change in their worktree voids their verdicts, but writes outside it are not seen",
                burst.project_name
            ));
        }
        // A squad whose merge needs more sessions than the burst may start (004 run 3).
        let needs = burst.squad.merge_roles();
        if usize::from(allowance) < needs.len() {
            self.alerts.push(format!(
                "{}: the merge needs a {} session, but this burst may start only {allowance}: further starts wait for your approval",
                burst.project_name,
                needs.join(" and a ")
            ));
        }
        let note = super::briefing::common_note(&burst);
        let project_name = burst.project_name.clone();
        let project = burst.project;
        self.bursts.push(burst);
        // The lead builds the work until a manifest delegates a piece (005 FR-001).
        self.apply_manifest(project, None);
        // Sessions that connected before the burst got no briefing with their connection: tell
        // them, as the engineer's own action (R-3).
        let connected: Vec<u32> = sessions
            .all()
            .into_iter()
            .filter(|s| s.running && s.project == lead.project && self.connected(s.id))
            .map(|s| s.id)
            .collect();
        for id in connected {
            let role = self
                .role_of(id)
                .map_or(String::new(), |r| format!(", role {r}"));
            let text = format!(
                "Coordination started in {project_name}: you are session #{id}{role}. Call Fallow's whoami tool now for your briefing (roles, rules, how to report work)."
            );
            let _ = self.request_delivery(
                sessions,
                Party::Fallow,
                id,
                text,
                super::delivery::Purpose::Message,
                true,
                now,
            );
        }
        self.boards.post(
            BoardId::Project(lead.project),
            Party::Fallow,
            note,
            std::time::SystemTime::now(),
            TEXT_BUDGET,
        );
        self.activity.add(
            Party::User,
            Action::PermissionGranted,
            &format!(
                "coordination in {} for {} min, lead #{}",
                lead.project_name,
                r.duration.as_secs() / 60,
                r.lead
            ),
            None,
        );
        self.notices.push(format!(
            "Coordinating {} for {} with #{} as lead",
            lead.project_name,
            hm(r.duration),
            r.lead
        ));
        Ok(())
    }

    /// Ends a project's burst (FR-004): its permission is revoked, so requests prompt again.
    pub fn end_burst(&mut self, project: ProjectId, why: &str) -> bool {
        let Some(i) = self.bursts.iter().position(|b| b.project == project) else {
            return false;
        };
        let b = self.bursts.remove(i);
        self.revoke(b.permission);
        let text = format!(
            "Coordination ended ({why}). Requests to deliver, create tasks or start sessions wait for the user again."
        );
        self.boards.post(
            BoardId::Project(project),
            Party::Fallow,
            text,
            std::time::SystemTime::now(),
            TEXT_BUDGET,
        );
        self.notices
            .push(format!("Coordination ended in {}: {why}", b.project_name));
        true
    }

    /// Replaces the burst's grant with a new one of at most 8 hours from now (FR-004).
    pub fn extend_burst(
        &mut self,
        project: ProjectId,
        d: Duration,
        now: Instant,
    ) -> Result<(), String> {
        if d > MAX_DURATION {
            return Err("A burst lasts at most 8 hours".into());
        }
        let (old, allowance) = {
            let b = self
                .bursts
                .iter()
                .find(|b| b.project == project)
                .ok_or("No coordination in this project")?;
            let used = self
                .permissions
                .list
                .iter()
                .find(|p| p.id == b.permission)
                .map_or(0, |p| p.starts_used);
            (b.permission, b.allowance.saturating_sub(used).max(1))
        };
        self.permissions.revoke(old);
        let p = self.grant(&Self::grant_for(project, allowance, d), now)?;
        if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
            b.permission = p;
            b.ends = now + d;
            b.warned = false;
        }
        self.notices
            .push(format!("Coordination extended by {}", hm(d)));
        Ok(())
    }

    /// Records the commit a session of the burst started from (its worktree's base).
    pub fn set_base(&mut self, session: u32, commit: String) {
        if let Some(b) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&session))
        {
            b.bases.insert(session, commit);
        }
    }

    /// The engineer names another lead (FR-003).
    pub fn set_lead(&mut self, project: ProjectId, session: u32) -> Result<(), String> {
        let b = self
            .bursts
            .iter_mut()
            .find(|b| b.project == project)
            .ok_or("No coordination in this project")?;
        b.roles.retain(|_, r| r != "lead");
        b.roles.insert(session, "lead".into());
        b.lead = session;
        // The new lead learns where things stand from Fallow and the repository (005 FR-033).
        let text = format!(
            "You are now the lead of the coordination in {}. Call Fallow's whoami for your briefing and squad_status for where the work stands: the manifest commit, each piece of work with its builder and state, open signals and what the merge needs. Everything decided is in the repository.",
            b.project_name
        );
        self.pending_reports.push((session, text));
        Ok(())
    }

    /// Ends bursts whose time is up and warns ten minutes before (called from `tick`).
    pub(crate) fn tick_bursts(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let due: Vec<ProjectId> = self
            .bursts
            .iter()
            .filter(|b| now >= b.ends)
            .map(|b| b.project)
            .collect();
        for p in due {
            changed |= self.end_burst(p, "time is up");
        }
        let mut warn = Vec::new();
        for b in &mut self.bursts {
            if !b.warned && b.ends.saturating_duration_since(now) <= WARN_BEFORE {
                b.warned = true;
                warn.push(b.project_name.clone());
            }
        }
        for name in warn {
            self.notices.push(format!(
                "Coordination in {name} ends in 10 minutes (+ extends)"
            ));
            changed = true;
        }
        changed
    }
}

/// "4 h", "1 h 30 min", "45 min".
pub fn hm(d: Duration) -> String {
    let m = d.as_secs() / 60;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}
