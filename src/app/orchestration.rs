//! Orchestration in the app (003): creating the hub with the first orchestrated session,
//! routing channel events to it, and its periodic work.

use std::time::Instant;

use crossterm::event::KeyEvent;

use super::commands::Context;
use super::editing::PromptKind;
use super::keymap;
use super::state::{AppState, Overlay};
use crate::editor::selection::LineInput;
use crate::orchestration::burst::{BurstRequest, DEFAULT_ALLOWANCE};
use crate::orchestration::channel::{self, Incoming};
use crate::orchestration::identity::Peer;
use crate::orchestration::inject::Bridge;
use crate::orchestration::{Hub, SessionInfo, Sessions, tools};
use crate::orchestration::{git, squad};
use crate::session::profiles::Style;
use crate::session::{SessionManager, SessionState};
use crate::ui::orchestration::{BurstForm, OrchView, Tab};

impl Sessions for SessionManager {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        let s = self.get(id)?;
        Some(SessionInfo {
            id: s.id,
            project: s.project,
            project_name: s.project_name.clone(),
            profile_id: s.profile.id.clone(),
            profile: s.profile.name.clone(),
            state: if s.state == SessionState::NeedsAttention && asks_permission(s) {
                crate::orchestration::WAITING_FOR_APPROVAL.into()
            } else {
                s.state.label()
            },
            running: s.state.running(),
            idle: s.state == SessionState::NeedsAttention,
            role: s.role.clone(),
        })
    }

    fn all(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .filter_map(|s| self.info(s.id))
            .collect()
    }

    fn verify(&self, id: u32, token: &str, peer: Peer) -> Result<(), String> {
        let s = self.get(id).ok_or("no such session")?;
        if !s.state.running() {
            return Err("the session is not running".into());
        }
        let t = s
            .orch
            .token
            .as_ref()
            .ok_or("the session runs without orchestration")?;
        if !t.matches(token) {
            return Err("wrong token".into());
        }
        #[cfg(unix)]
        if peer.uid != Some(crate::orchestration::identity::my_uid()) {
            return Err("another user's process".into());
        }
        if !s.contains(peer.pid) {
            return Err(format!("process {} is not part of the session", peer.pid));
        }
        Ok(())
    }
}

/// What agents' permission prompts say (Claude Code, Codex CLI, Gemini CLI).
const PROMPTS: &[&str] = &[
    "Do you want to proceed?",
    "requires approval",
    "Do you want to make this edit",
    "Do you want to create",
    "Allow command?",
    "Allow execution",
    "Apply this change?",
];

/// Whether an idle session's screen shows its agent asking for permission: its last lines.
fn asks_permission(s: &crate::session::Session) -> bool {
    let Ok(mut t) = s.term.lock() else {
        return false;
    };
    let n = t.total_lines();
    let lines: Vec<String> = (n.saturating_sub(15)..n).map(|v| t.line_text(v)).collect();
    looks_like_prompt(&lines)
}

/// A permission prompt at the bottom of a screen: a prompt's question, not in echoed input
/// (a line starting with `>` or `❯`), together with its choices (review: a typed message
/// quoting "requires approval" must not count).
pub fn looks_like_prompt(lines: &[String]) -> bool {
    let question = lines.iter().any(|l| {
        let t = l.trim_start();
        !t.starts_with('>') && !t.starts_with('❯') && PROMPTS.iter().any(|p| t.contains(p))
    });
    let choices = lines.iter().any(|l| {
        l.contains("1. Yes") || l.contains("Yes (y)") || l.to_ascii_lowercase().contains("(y/n)")
    });
    question && choices
}

/// The hub's view of the app: the sessions, plus the project list for tools that name a project.
pub struct AppView<'a> {
    pub sessions: &'a SessionManager,
    pub projects: &'a crate::projects::ProjectList,
    pub profiles: &'a [crate::session::profiles::AgentProfile],
    /// Why no session may start now, if so (session limit, memory).
    pub guard: Option<String>,
}

/// Fallow's own memory above which agents may not start more sessions (FR-021: never beyond
/// FR-029's 150 MB budget; the margin is for the session about to start).
pub const START_MEMORY_LIMIT: u64 = 140 * 1024 * 1024;

/// The read-only worktree checks (004 FR-013): run off the main loop by `git`, every 2 minutes
/// and whenever a read-only session signals.
#[derive(Debug, Default)]
pub struct ReadOnlyChecks {
    /// Each session's branch tip and uncommitted files at its first check.
    base: std::collections::HashMap<u32, (String, Vec<String>)>,
    running: std::collections::HashSet<u32>,
    last: Option<Instant>,
    results: ReadOnlyResults,
    /// When gate runs were last looked for (they may ask git for the lead's commit).
    gates_at: Option<Instant>,
    /// When the target branches were last watched (FR-028a).
    watched_at: Option<Instant>,
}

type ReadOnlyResults = std::sync::Arc<
    std::sync::Mutex<Vec<(u32, Result<(String, Vec<String>, Vec<String>), String>)>>,
>;

const READ_ONLY_EVERY: std::time::Duration = std::time::Duration::from_secs(120);
const GATES_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

impl Sessions for AppView<'_> {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.sessions.info(id)
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.sessions.all()
    }
    fn verify(&self, id: u32, token: &str, peer: Peer) -> Result<(), String> {
        self.sessions.verify(id, token, peer)
    }
    fn profiles(&self) -> Vec<(String, String)> {
        self.profiles
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect()
    }
    fn start_allowed(&self) -> Result<(), String> {
        match &self.guard {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }
    fn profile(&self, id: &str) -> Option<crate::session::profiles::AgentProfile> {
        self.profiles.iter().find(|p| p.id == id).cloned()
    }
    fn workdir(&self, id: u32) -> Option<(std::path::PathBuf, Option<String>)> {
        self.sessions
            .get(id)
            .map(|s| (s.cwd.clone(), s.workdir.branch().map(str::to_string)))
    }
    fn projects(&self) -> Vec<(crate::projects::ProjectId, String, bool)> {
        self.projects
            .projects
            .iter()
            .map(|p| (p.id, p.name.clone(), p.present))
            .collect()
    }
}

impl AppState {
    /// Creates the hub and opens the channel when the first session that would use them is
    /// about to start (FR-005, constitution II). Problems are remembered and shown once; the
    /// session then starts without orchestration and says why.
    pub fn ensure_hub(&mut self, style: Style) {
        if style == Style::None || self.hub.is_some() {
            return;
        }
        if !self.orch_settings.enabled {
            self.sessions.orch_unavailable = Some("disabled in settings".into());
            return;
        }
        if self.sessions.orch_unavailable.as_deref() == Some("disabled in settings") {
            self.sessions.orch_unavailable = None;
        }
        if self.sessions.orch_unavailable.is_some() {
            return;
        }
        let Some(tx) = self.events.clone() else {
            return;
        };
        let run_dir = crate::config::run_dir();
        let opened = match &run_dir {
            Some(dir) => {
                #[cfg(windows)]
                let _ = std::fs::create_dir_all(dir);
                channel::open(dir, tools::list(), tx)
            }
            None => Err("no cache folder is known (HOME or LOCALAPPDATA unset)".into()),
        };
        let exe =
            std::env::current_exe().map_err(|e| format!("cannot find Fallow's own program: {e}"));
        match (opened, exe) {
            (Ok(ch), Ok(exe)) => {
                self.sessions.bridge = Some(Bridge {
                    exe,
                    endpoint: ch.endpoint().to_string(),
                    fallow_pid: std::process::id(),
                    run_dir,
                });
                self.hub = Some(Hub::with_channel(self.orch_settings.clone(), ch));
            }
            (Err(e), _) | (_, Err(e)) => {
                self.notices
                    .push(format!("Agent orchestration is unavailable: {e}"));
                self.sessions.orch_unavailable = Some(e);
            }
        }
    }

    /// An event from the channel.
    pub fn hub_event(&mut self, ev: Incoming) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            // No hub: refuse a hello so the bridge does not wait.
            if let Incoming::Hello { verdict, .. } = ev {
                let _ = verdict.send(Err("orchestration is off".into()));
            }
            return false;
        };
        let guard = start_guard(&self.sessions, self.snapshot.as_deref());
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
            profiles: &self.profiles,
            guard,
        };
        let changed = hub.handle(ev, &view, Instant::now());
        self.take_hub_notices();
        changed
    }

    /// The hub's periodic work, from the main loop's tick.
    pub fn tick_hub(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let guard = start_guard(&self.sessions, self.snapshot.as_deref());
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
            profiles: &self.profiles,
            guard,
        };
        let mut changed = hub.tick(&view, Instant::now());
        // Roles live in the hub's bursts; sessions show them (004 FR-009).
        for s in &mut self.sessions.sessions {
            let role = hub.role_of(s.id).map(str::to_string);
            if s.role != role {
                s.role = role;
                changed = true;
            }
        }
        changed |= self.carry_out_starts();
        changed |= self.check_read_only();
        changed |= self.run_gates();
        changed |= self.publish_checks();
        changed |= self.type_deliveries();
        self.take_hub_notices();
        changed
    }

    /// Starts the sessions agents' approved requests asked for (US5), checking the limits again
    /// (FR-021). The new sessions do not take focus.
    fn carry_out_starts(&mut self) -> bool {
        let starts = match self.hub.as_ref() {
            Some(h) => h.approved_starts(),
            None => return false,
        };
        if starts.is_empty() {
            return false;
        }
        let mut pending_bases = Vec::new();
        for s in starts {
            let result = match start_guard(&self.sessions, self.snapshot.as_deref()) {
                Some(why) => Err(why),
                None => {
                    let place = self.default_where(s.project);
                    // In a burst a new session starts from the requester's commit (the lead's
                    // integrated work, or its committed plan), not from the project's HEAD.
                    let base = s
                        .role
                        .as_ref()
                        .and_then(|_| s.from.session())
                        .and_then(|from| self.sessions.get(from))
                        .and_then(|from| git::head(&from.cwd).ok());
                    // A builder's branch names its work and part (005 R-18).
                    let branch_name = s.assignment.as_ref().map(|(w, p)| match p {
                        Some(p) => format!("{w}-{p}"),
                        None => w.clone(),
                    });
                    let started = self.start_role_session(
                        s.project,
                        &s.profile,
                        place,
                        s.role.clone(),
                        s.args.clone(),
                        base.clone(),
                        branch_name,
                    );
                    if let (Ok(sid), Some(b)) = (&started, base) {
                        pending_bases.push((*sid, b));
                    }
                    started
                }
            };
            if let Ok(sid) = &result {
                self.say(format!(
                    "Started #{sid} {} in {} for {}",
                    s.profile_name,
                    s.project_name,
                    s.from.label()
                ));
            }
            let guard = start_guard(&self.sessions, self.snapshot.as_deref());
            let view = AppView {
                sessions: &self.sessions,
                projects: &self.projects,
                profiles: &self.profiles,
                guard,
            };
            if let Some(h) = self.hub.as_mut() {
                h.start_done(&view, s.id, result, Instant::now());
                for (sid, b) in pending_bases.drain(..) {
                    h.set_base(sid, b);
                }
            }
        }
        true
    }

    /// Types approved deliveries into their sessions, except while the engineer is typing in
    /// the target (FR-014): then it waits until they leave or pause for the delivery delay.
    fn type_deliveries(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let delay = std::time::Duration::from_millis(self.orch_settings.delivery_delay_ms);
        let mut changed = false;
        for id in hub.ready() {
            let Some(d) = hub.deliveries.get(id).cloned() else {
                continue;
            };
            let focused = self.workspace.view == super::workspace::View::Agents
                && self.workspace.focus == super::workspace::AgentsFocus::Session
                && self.workspace.active == Some(d.to);
            let result = match self.sessions.get(d.to) {
                None => Err("target exited".to_string()),
                Some(s) => {
                    if focused && s.last_key.is_some_and(|t| t.elapsed() < delay) {
                        continue;
                    }
                    match hub.take_grant(id) {
                        Some(g) => s.deliver(g, &d),
                        None => continue,
                    }
                }
            };
            hub.delivered(id, result);
            changed = true;
        }
        changed
    }

    /// Starts due gate runs and ends timed-out ones (004 research R-6). Gate sessions take no
    /// focus and count against `sessions.max_sessions`.
    fn run_gates(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let mut changed = false;
        for id in std::mem::take(&mut hub.end_gates) {
            let _ = self.sessions.end_gate(id);
            changed = true;
        }
        let now = Instant::now();
        if self
            .ro_checks
            .gates_at
            .is_some_and(|t| now.saturating_duration_since(t) < GATES_EVERY)
        {
            return changed;
        }
        self.ro_checks.gates_at = Some(now);
        let leads: std::collections::HashMap<crate::projects::ProjectId, std::path::PathBuf> = hub
            .bursts
            .iter()
            .filter_map(|b| {
                self.sessions
                    .get(b.lead)
                    .map(|s| (b.project, s.cwd.clone()))
            })
            .collect();
        let jobs = hub.gates_due(&|p, needed| {
            let dir = leads.get(&p)?;
            let head = git::head(dir).ok()?;
            needed
                .iter()
                .all(|c| git::on_branch(dir, c, &head))
                .then_some(head)
        });
        for job in jobs {
            let Some(path) = self.projects.get(job.project).map(|p| p.path.clone()) else {
                continue;
            };
            // A run's baseline checkout stays next to its copy (005 R-11).
            let root = if self.projects.get(job.project).is_some_and(|p| p.temporary) {
                None
            } else {
                self.sessions.limits.worktree_root.clone()
            };
            let (cwd, commit, dir) = match job.kind {
                crate::orchestration::gates::Kind::Baseline => {
                    let commit = job.commit.clone().unwrap_or_default();
                    let name = format!(
                        "fallow-gate-{}-baseline-{}",
                        job.gate.name,
                        std::process::id()
                    );
                    match crate::session::worktree::create_detached(
                        &path,
                        &commit,
                        root.as_deref(),
                        &name,
                    ) {
                        Ok(d) => (d.clone(), Some(commit), Some(d)),
                        Err(e) => {
                            if let Some(h) = self.hub.as_mut() {
                                h.gate_started(&job, Err(e), Some(commit), None, Instant::now());
                            }
                            changed = true;
                            continue;
                        }
                    }
                }
                crate::orchestration::gates::Kind::Final => {
                    let cwd = self
                        .sessions
                        .get(job.lead)
                        .map_or(path.clone(), |s| s.cwd.clone());
                    let commit = git::head(&cwd).ok();
                    (cwd, commit, None)
                }
            };
            let result = self.start_gate_session(&job, cwd);
            if result.is_ok() {
                self.gates_started += 1;
            }
            if result.is_err() {
                if let Some(d) = &dir {
                    let _ = crate::session::worktree::remove_temporary(&path, d);
                }
            }
            let dir = if result.is_ok() { dir } else { None };
            if let Some(h) = self.hub.as_mut() {
                h.gate_started(&job, result, commit, dir, Instant::now());
            }
            changed = true;
        }
        if changed {
            self.take_hub_notices();
        }
        changed
    }

    /// Starts the command session for a gate run: the shell with the gate's command for this
    /// OS, no orchestration, no focus.
    fn start_gate_session(
        &mut self,
        job: &crate::orchestration::gates::Job,
        cwd: std::path::PathBuf,
    ) -> Result<u32, String> {
        use crate::orchestration::gates;
        let (program, args) = gates::shell_command(job.gate.command_for(std::env::consts::OS));
        let profile = crate::session::profiles::AgentProfile {
            id: gates::GATE_PROFILE.into(),
            name: format!("gate {} ({})", job.gate.name, job.kind.label()),
            command: program,
            args,
            env: Vec::new(),
            builtin: false,
            orchestration: Style::None,
        };
        let p = self
            .projects
            .get(job.project)
            .ok_or("the project is not in the list")?;
        let tx = self
            .events
            .clone()
            .ok_or("Sessions are not available in this mode")?;
        let area = crate::ui::workspace::session_rect(self);
        let req = crate::session::StartRequest {
            project: p.id,
            project_name: p.name.clone(),
            cwd,
            workdir: crate::session::Workdir::ProjectFolder,
            profile,
            rows: area.height.max(1),
            cols: area.width.max(1),
        };
        let sid = self
            .sessions
            .start_with(req, Some(gates::GATE_ROLE.into()), Vec::new(), &tx)?;
        self.track(sid);
        Ok(sid)
    }

    /// The commit a local merge takes: the final gates' commit, or (a squad without gates) the
    /// lead's commit now.
    fn gated_commit(&self, project: crate::projects::ProjectId) -> Option<String> {
        let hub = self.hub.as_ref()?;
        let r = hub.readiness(project)?;
        r.commit.or_else(|| {
            let lead = hub.burst_of(project)?.lead;
            git::head(&self.sessions.get(lead)?.cwd).ok()
        })
    }

    /// `m` in the Squad tab: asks to merge exactly the gated commit (FR-028, SC-009).
    fn ask_merge(&mut self, project: crate::projects::ProjectId) -> bool {
        use crate::orchestration::publish::Mode;
        let Some(r) = self.hub.as_ref().and_then(|h| h.readiness(project)) else {
            self.say("No coordination in this project");
            return true;
        };
        if r.mode != Mode::LocalMerge {
            self.say(
                "This project has a remote: the lead opens a draft pull request, which you merge",
            );
            return true;
        }
        if !r.ready {
            self.say("Not ready: every merge condition must hold (Squad tab)");
            return true;
        }
        let Some(commit) = self.gated_commit(project) else {
            self.say("No gated commit is known yet");
            return true;
        };
        let target = self
            .hub
            .as_ref()
            .and_then(|h| h.burst_of(project))
            .and_then(|b| b.target_branch.clone())
            .unwrap_or_else(|| "the target branch".into());
        let d = crate::ui::confirm::Dialog::new(
            "Merge the squad's work",
            &format!("merge {commit}"),
            vec![
                format!(
                    "Fallow runs git merge --no-ff {} on {target}",
                    &commit[..commit.len().min(10)]
                ),
                "in the project folder, which must be on that branch and clean.".into(),
            ],
            "Merge",
        );
        self.overlay = Overlay::Confirm {
            dialog: d,
            action: super::editing::Pending::Merge { project, commit },
        };
        true
    }

    /// The confirmed (or the squad's automatic) local merge.
    pub(super) fn merge_now(&mut self, project: crate::projects::ProjectId, commit: &str) {
        let Some(path) = self.projects.get(project).map(|p| p.path.clone()) else {
            return;
        };
        let target = self
            .hub
            .as_ref()
            .and_then(|h| h.burst_of(project))
            .and_then(|b| b.target_branch.clone());
        let Some(target) = target else {
            self.say("No target branch is known (the project was not on a branch when coordination started)");
            return;
        };
        match crate::orchestration::publish::local_merge(&path, &target, commit) {
            Ok(merge) => {
                if let Some(h) = self.hub.as_mut() {
                    h.merged(project, commit, &merge);
                }
                self.take_hub_notices();
            }
            Err(e) => self.say(format!("Not merged: {e}")),
        }
    }

    /// The squad's automatic local merge once ready, and the target-branch watcher every two
    /// minutes (FR-028, FR-028a).
    fn publish_checks(&mut self) -> bool {
        use crate::orchestration::publish::Mode;
        use crate::orchestration::squad::Merge;
        let Some(hub) = self.hub.as_ref() else {
            return false;
        };
        let auto: Vec<crate::projects::ProjectId> = hub
            .bursts
            .iter()
            .filter(|b| b.squad.merge == Merge::Automatic && b.merged.is_empty())
            .filter(|b| {
                hub.readiness(b.project)
                    .is_some_and(|r| r.ready && r.mode == Mode::LocalMerge)
            })
            .map(|b| b.project)
            .collect();
        let mut changed = false;
        for p in auto {
            if let Some(c) = self.gated_commit(p) {
                self.merge_now(p, &c);
                changed = true;
            }
        }
        let now = Instant::now();
        if self
            .ro_checks
            .watched_at
            .is_some_and(|t| now.saturating_duration_since(t) < READ_ONLY_EVERY)
        {
            return changed;
        }
        self.ro_checks.watched_at = Some(now);
        let Some(hub) = self.hub.as_ref() else {
            return changed;
        };
        let watch: Vec<_> = hub
            .bursts
            .iter()
            .filter_map(|b| {
                let path = self.projects.get(b.project)?.path.clone();
                Some((
                    b.project,
                    path,
                    b.start_commit.clone()?,
                    b.target_branch.clone()?,
                    b.merged.clone(),
                ))
            })
            .collect();
        for (p, path, start, target, merged) in watch {
            if let Ok(foreign) =
                crate::orchestration::publish::foreign(&path, &start, &target, &merged)
            {
                if let Some(h) = self.hub.as_mut() {
                    h.foreign_commits(p, foreign);
                }
            }
        }
        self.take_hub_notices();
        changed
    }

    /// Starts due read-only checks and applies finished ones (FR-013).
    fn check_read_only(&mut self) -> bool {
        let Some(hub) = self.hub.as_mut() else {
            return false;
        };
        let mut changed = false;
        let done = self
            .ro_checks
            .results
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default();
        for (id, result) in done {
            self.ro_checks.running.remove(&id);
            let Ok((tip, files, commits)) = result else {
                continue;
            };
            let Some((_, before)) = self.ro_checks.base.get(&id).cloned() else {
                self.ro_checks.base.insert(id, (tip, files));
                continue;
            };
            let new_files: Vec<&String> = files.iter().filter(|f| !before.contains(f)).collect();
            let why = if !new_files.is_empty() {
                let shown: Vec<&str> = new_files.iter().take(3).map(|f| f.as_str()).collect();
                Some(format!(
                    "{} changed file(s): {}",
                    new_files.len(),
                    shown.join(", ")
                ))
            } else if !commits.is_empty() {
                Some(format!("{} new commit(s)", commits.len()))
            } else {
                None
            };
            if let Some(why) = why {
                hub.taint(id, why);
                changed = true;
            }
        }
        let now = Instant::now();
        let mut want = std::mem::take(&mut hub.check_worktrees);
        if self
            .ro_checks
            .last
            .is_none_or(|t| now.saturating_duration_since(t) >= READ_ONLY_EVERY)
        {
            self.ro_checks.last = Some(now);
            want.extend(hub.read_only_sessions());
        }
        let read_only = hub.read_only_sessions();
        want.sort_unstable();
        want.dedup();
        for id in want {
            if !read_only.contains(&id) || self.ro_checks.running.contains(&id) {
                continue;
            }
            let Some(s) = self.sessions.get(id) else {
                continue;
            };
            let dir = s.cwd.clone();
            let branch = s.workdir.branch().map(str::to_string);
            let base = self.ro_checks.base.get(&id).map(|(tip, _)| tip.clone());
            self.ro_checks.running.insert(id);
            let out = self.ro_checks.results.clone();
            crate::orchestration::git::spawn(
                move || {
                    crate::orchestration::git::read_only_check(
                        &dir,
                        branch.as_deref(),
                        base.as_deref(),
                    )
                },
                move |r| {
                    if let Ok(mut v) = out.lock() {
                        v.push((id, r));
                    }
                },
            );
        }
        changed
    }

    pub(super) fn take_hub_notices(&mut self) {
        let Some(hub) = self.hub.as_mut() else {
            return;
        };
        let shown = hub.alerts.last().or(hub.notices.last()).cloned();
        hub.alerts.clear();
        hub.notices.clear();
        if let Some(text) = shown {
            self.say(text);
        }
    }
}

impl AppState {
    /// Orchestration commands (contracts/ui-commands.md). `None`: not one of them.
    pub(crate) fn run_orch(&mut self, id: &str) -> Option<bool> {
        let pending = self.hub.as_ref().map_or(0, |h| h.pending_requests());
        match id {
            // Opens on Requests when requests wait (contracts/ui-commands.md).
            "orch.open" | "orch.requests" => {
                let tab = if id == "orch.requests" || pending > 0 {
                    Tab::Requests
                } else {
                    Tab::Board
                };
                self.overlay = Overlay::Orch(OrchView {
                    tab,
                    ..OrchView::default()
                });
                return Some(true);
            }
            // Reachable from the palette anywhere (SC-005: one command).
            "orch.revoke_all" => {
                let n = self.hub.as_mut().map_or(0, |h| h.revoke_all());
                self.say(format!(
                    "Revoked {n} standing permission{}",
                    if n == 1 { "" } else { "s" }
                ));
                return Some(true);
            }
            "orch.activity" => {
                self.overlay = Overlay::Orch(OrchView {
                    tab: Tab::Activity,
                    ..OrchView::default()
                });
                return Some(true);
            }
            "orch.coordinate" => return Some(self.open_burst()),
            "runs.register" | "runs.start" | "runs.series" | "orch.memory_budget" => {
                use super::editing::PromptKind;
                let kind = match id {
                    "runs.register" => PromptKind::RegisterTaskSet,
                    "runs.start" => PromptKind::StartRun,
                    "runs.series" => PromptKind::StartSeries,
                    _ => PromptKind::MemoryBudget,
                };
                self.overlay = Overlay::Prompt {
                    kind,
                    input: LineInput::default(),
                    error: None,
                };
                return Some(true);
            }
            "runs.end" => return Some(self.end_run_now()),
            "runs.open" => {
                self.discard_leftover_copies();
                self.overlay = Overlay::Orch(OrchView {
                    tab: Tab::Runs,
                    ..OrchView::default()
                });
                return Some(true);
            }
            "squad.create" => return Some(self.create_squad_file()),
            "orch.end" => {
                let project = self
                    .workspace
                    .active
                    .and_then(|id| self.sessions.get(id))
                    .map(|s| s.project)
                    .or(self.projects.selected().map(|p| p.id));
                let ended = match (project, self.hub.as_mut()) {
                    (Some(p), Some(h)) => h.end_burst(p, "ended by you"),
                    _ => false,
                };
                if ended {
                    self.take_hub_notices();
                } else {
                    self.say("No coordination in this project");
                }
                return Some(true);
            }
            "orch.extend" => {
                let project = self
                    .workspace
                    .active
                    .and_then(|id| self.sessions.get(id))
                    .map(|s| s.project)
                    .or(self.projects.selected().map(|p| p.id));
                let coordinating =
                    project.filter(|p| self.hub.as_ref().is_some_and(|h| h.burst_of(*p).is_some()));
                let Some(p) = coordinating else {
                    self.say("No coordination in this project");
                    return Some(true);
                };
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::ExtendBurst(p),
                    input: LineInput::selected("4h"),
                    error: None,
                };
                return Some(true);
            }
            "orch.task_new" if !matches!(self.overlay, Overlay::Orch(_)) => {
                return Some(self.new_task_prompt(OrchView {
                    tab: Tab::Queue,
                    ..OrchView::default()
                }));
            }
            "orch.grant_new" if !matches!(self.overlay, Overlay::Orch(_)) => {
                self.overlay = Overlay::Orch(OrchView {
                    tab: Tab::Permissions,
                    ..OrchView::default()
                });
                return Some(self.open_grant(None));
            }
            _ => {}
        }
        if id == "session.details" {
            return Some(self.session_details());
        }
        let Overlay::Orch(v) = &mut self.overlay else {
            return None;
        };
        let page = self.area.height.saturating_sub(8).max(1) as usize;
        match id {
            "orch.close" => self.overlay = Overlay::None,
            "orch.tab_next" | "orch.tab_prev" => {
                v.tab = v.tab.next(if id == "orch.tab_next" { 1 } else { -1 });
                v.scroll = 0;
                v.selected = 0;
            }
            "orch.board_toggle" if v.tab == Tab::Board => {
                v.shared = !v.shared;
                v.scroll = 0;
            }
            // Lists: Up/Down move the selection.
            "orch.up" if v.tab != Tab::Board && v.tab != Tab::Activity => {
                v.selected = v.selected.saturating_sub(1);
            }
            "orch.down" if v.tab != Tab::Board && v.tab != Tab::Activity => v.selected += 1,
            "orch.approve" | "orch.reject" | "orch.edit" | "orch.grant"
                if v.tab == Tab::Requests =>
            {
                let (sel, view) = (v.selected, v.clone());
                // A decision during a run is an intervention (005 FR-014).
                self.note_intervention(None);
                let ids = crate::ui::orchestration::requests_shown(self);
                let Some(&r) = ids.get(sel.min(ids.len().saturating_sub(1))) else {
                    return Some(false);
                };
                use crate::ui::orchestration::RequestRef;
                match (id, r) {
                    ("orch.approve", RequestRef::Delivery(rid)) => {
                        if let Some(h) = self.hub.as_mut() {
                            if let Err(e) = h.approve(rid, None) {
                                self.say(e);
                            }
                        }
                    }
                    ("orch.approve" | "orch.reject", RequestRef::Start(st)) => {
                        if let Some(h) = self.hub.as_mut() {
                            h.decide_start(st, id == "orch.approve");
                        }
                    }
                    ("orch.edit", RequestRef::Start(_)) => {
                        self.say("A start request has nothing to edit");
                    }
                    ("orch.approve" | "orch.reject", RequestRef::Task(t)) => {
                        if let Some(h) = self.hub.as_mut() {
                            h.decide_task(t, id == "orch.approve");
                        }
                    }
                    ("orch.reject", RequestRef::Delivery(rid)) => {
                        if let Some(h) = self.hub.as_mut() {
                            h.reject(rid);
                        }
                    }
                    ("orch.edit", RequestRef::Delivery(rid)) => {
                        let text = self
                            .hub
                            .as_ref()
                            .and_then(|h| h.deliveries.get(rid))
                            .map(|d| d.text.clone())
                            .unwrap_or_default();
                        self.overlay = Overlay::Prompt {
                            kind: PromptKind::EditDelivery(rid, view),
                            input: LineInput::selected(&text),
                            error: None,
                        };
                    }
                    ("orch.edit", RequestRef::Task(t)) => {
                        return Some(self.edit_task_prompt(t, true, view));
                    }
                    (_, r) => return Some(self.open_grant(Some(r))),
                }
            }
            "orch.edit" if v.tab == Tab::Queue => {
                let (sel, view) = (v.selected, v.clone());
                let tasks = crate::ui::orchestration::tasks_shown(self);
                let Some(&t) = tasks.get(sel) else {
                    return Some(false);
                };
                return Some(self.edit_task_prompt(t, false, view));
            }
            "orch.task_new" => {
                let view = OrchView {
                    tab: Tab::Queue,
                    ..v.clone()
                };
                return Some(self.new_task_prompt(view));
            }
            "orch.move_up" | "orch.move_down" | "orch.pause" if v.tab == Tab::Queue => {
                let sel = v.selected;
                let tasks = crate::ui::orchestration::tasks_shown(self);
                let Some(&t) = tasks.get(sel) else {
                    return Some(false);
                };
                if let Some(h) = self.hub.as_mut() {
                    match id {
                        "orch.pause" => h.toggle_pause(t),
                        _ => {
                            let step = if id == "orch.move_up" { -1 } else { 1 };
                            h.tasks.move_by(t, step);
                            if let Overlay::Orch(v) = &mut self.overlay {
                                v.selected = (sel as isize + step)
                                    .clamp(0, tasks.len() as isize - 1)
                                    as usize;
                            }
                        }
                    }
                }
            }
            "orch.delete" if v.tab == Tab::Queue => {
                let sel = v.selected;
                let tasks = crate::ui::orchestration::tasks_shown(self);
                if let (Some(&t), Some(h)) = (tasks.get(sel), self.hub.as_mut()) {
                    h.cancel_task(t);
                }
            }
            "orch.grant_new" => return Some(self.open_grant(None)),
            "orch.delete" if v.tab == Tab::Runs => {
                // 005: delete a record, confirmed.
                let records = crate::config::cache_root()
                    .map(|c| crate::orchestration::record::list(&c))
                    .unwrap_or_default();
                if let Some((n, _)) = records.get(v.selected) {
                    let n = *n;
                    let view = v.clone();
                    let d = crate::ui::confirm::Dialog::new(
                        "Delete a run record",
                        &format!("delete run record {n}"),
                        vec![format!("Run record {n} is deleted from Fallow's cache.")],
                        "Delete",
                    );
                    self.overlay = Overlay::Confirm {
                        dialog: d,
                        action: super::editing::Pending::DeleteRecord(n),
                    };
                    let _ = view;
                }
            }
            "orch.repair_again" | "orch.take_over" | "orch.delete" if v.tab == Tab::Squad => {
                use crate::orchestration::repair::Decision;
                let sel = v.selected;
                let sessions = crate::ui::orchestration::squad_sessions(self).len();
                let Some(i) = sel.checked_sub(sessions) else {
                    self.say("Select a stopped repair loop (below the sessions)");
                    return Some(true);
                };
                let Some(project) = self.projects.selected().map(|p| p.id) else {
                    return Some(false);
                };
                let d = match id {
                    "orch.repair_again" => Decision::Again,
                    "orch.take_over" => Decision::TakeOver,
                    _ => Decision::Cancel,
                };
                let guard = start_guard(&self.sessions, self.snapshot.as_deref());
                let view = AppView {
                    sessions: &self.sessions,
                    projects: &self.projects,
                    profiles: &self.profiles,
                    guard,
                };
                if let Some(h) = self.hub.as_mut() {
                    if let Err(e) = h.repair_decide(&view, project, i, d, Instant::now()) {
                        self.say(e);
                    }
                }
                self.take_hub_notices();
            }
            "orch.merge" if v.tab == Tab::Squad => {
                let Some(project) = self.projects.selected().map(|p| p.id) else {
                    return Some(false);
                };
                return Some(self.ask_merge(project));
            }
            "orch.make_lead" if v.tab == Tab::Squad => {
                let sel = v.selected;
                let ids = crate::ui::orchestration::squad_sessions(self);
                let Some(&id) = ids.get(sel) else {
                    return Some(false);
                };
                let Some(project) = self.sessions.get(id).map(|s| s.project) else {
                    return Some(false);
                };
                if let Some(h) = self.hub.as_mut() {
                    match h.set_lead(project, id) {
                        Ok(()) => self.say(format!("#{id} is now the lead")),
                        Err(e) => self.say(e),
                    }
                }
            }
            "orch.filter" if v.tab == Tab::Activity => {
                v.filter = (v.filter + 1) % 3;
                v.scroll = 0;
            }
            "orch.search" if v.tab == Tab::Activity => {
                let view = v.clone();
                let q = view.query.clone();
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::SearchActivity(view),
                    input: LineInput::selected(&q),
                    error: None,
                };
            }
            "orch.delete" if v.tab == Tab::Permissions => {
                let i = v.selected;
                if let Some(h) = self.hub.as_mut() {
                    if let Some(p) = h.permissions.list.get(i).map(|p| p.id) {
                        h.revoke(p);
                    }
                }
            }
            // Board: newest at the bottom, so "up" scrolls back in time.
            "orch.up" => v.scroll += 1,
            "orch.down" => v.scroll = v.scroll.saturating_sub(1),
            "orch.signal" => {
                let view = v.clone();
                let scope = self
                    .projects
                    .selected()
                    .map_or(crate::orchestration::board::BoardId::Shared, |p| {
                        crate::orchestration::board::BoardId::Project(p.id)
                    });
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::SignalEvent(scope, view),
                    input: LineInput::default(),
                    error: None,
                };
            }
            "orch.clear" if v.tab == Tab::Queue => {
                let i = v
                    .selected
                    .checked_sub(crate::ui::orchestration::tasks_shown(self).len());
                let keys = crate::ui::orchestration::events_shown(self);
                if let (Some(k), Some(h)) = (i.and_then(|i| keys.get(i)), self.hub.as_mut()) {
                    h.clear_event(k);
                }
            }
            "orch.page_up" => v.scroll += page,
            "orch.page_down" => v.scroll = v.scroll.saturating_sub(page),
            "orch.write" => {
                let view = v.clone();
                let (board, _) = crate::ui::orchestration::board_shown(self, &view);
                self.overlay = Overlay::Prompt {
                    kind: PromptKind::BoardPost(board, view),
                    input: LineInput::default(),
                    error: None,
                };
            }
            _ => return Some(false),
        }
        Some(true)
    }

    /// Keys while the orchestration view is open.
    pub(crate) fn orch_key(&mut self, key: KeyEvent) -> bool {
        // Help, palette and quit stay reachable.
        if let Some(id) = keymap::lookup(&key, Context::Global, true) {
            if matches!(id, "palette" | "quit") {
                self.overlay = Overlay::None;
                return self.run(id);
            }
        }
        match keymap::lookup(&key, Context::Orchestration, false) {
            Some(id) => self.run(id),
            None => false,
        }
    }

    /// The selected session's link: state, client, reason (contracts/ui-commands.md
    /// "Session details").
    fn session_details(&mut self) -> bool {
        let Some(s) = self.workspace.active.and_then(|id| self.sessions.get(id)) else {
            return false;
        };
        let mut lines = vec![
            format!("Profile: {}", s.profile.name),
            format!("Project: {}", s.project_name),
            format!("Folder:  {}", s.cwd.display()),
            format!("State:   {}", s.state.label()),
        ];
        let link = self.hub.as_ref().and_then(|h| h.links.get(&s.id));
        let orch = match (&s.orch.unsupported, s.orch.token.is_some(), link) {
            (Some(why), ..) => format!("no orchestration ({why})"),
            (None, true, Some(l)) if l.conn.is_some() => match &l.client {
                Some((name, version)) => format!("connected ({name} {version})"),
                None => "connected".into(),
            },
            (None, true, _) => "offered; the agent has not connected yet".into(),
            (None, false, _) => "not running".into(),
        };
        lines.push(format!("Orchestration: {orch}"));
        if let Some(role) = &s.role {
            let level = self
                .hub
                .as_ref()
                .and_then(|h| h.burst_for(s.id))
                .and_then(|b| b.squad.role(role))
                .map_or("", |r| r.level.as_str());
            lines.push(format!("Role:    {role} ({level})"));
        }
        if let Some(c) = &s.command_line {
            lines.push("Command line:".into());
            lines.push(format!("  {c}"));
            if crate::orchestration::roles::bypasses(
                &c.split(' ').map(str::to_string).collect::<Vec<_>>(),
            ) {
                lines.push(
                    "  Warning: these flags let the agent skip its own permission checks.".into(),
                );
            }
        }
        self.overlay = Overlay::Info {
            title: s.label(),
            lines,
        };
        true
    }
}

impl AppState {
    fn new_task_prompt(&mut self, view: OrchView) -> bool {
        self.overlay = Overlay::Prompt {
            kind: PromptKind::NewTask(view),
            input: LineInput::default(),
            error: None,
        };
        true
    }

    fn edit_task_prompt(&mut self, t: u32, approve: bool, view: OrchView) -> bool {
        let text = self
            .hub
            .as_ref()
            .and_then(|h| h.tasks.get(t))
            .map(|t| t.text.clone())
            .unwrap_or_default();
        self.overlay = Overlay::Prompt {
            kind: PromptKind::EditTask(t, approve, view),
            input: LineInput::selected(&text),
            error: None,
        };
        true
    }

    /// The engineer's new task: `#2 text…` or `claude@api text…`, with optional `after:a,b`
    /// (events of the target's project).
    pub(crate) fn submit_new_task(&mut self, line: &str) -> Result<u32, String> {
        use crate::orchestration::tasks::Target;
        let mut words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 2 {
            return Err("Type the target, then the task: #2 merge feature/x after:pr-1".into());
        }
        let target_word = words.remove(0);
        let mut after = Vec::new();
        words.retain(|w| match w.strip_prefix("after:") {
            Some(list) => {
                after.extend(
                    list.split(',')
                        .filter(|n| !n.is_empty())
                        .map(str::to_string),
                );
                false
            }
            None => true,
        });
        let text = words.join(" ");
        let (target, project) = if let Some(n) = target_word.strip_prefix('#') {
            let id: u32 = n
                .parse()
                .map_err(|_| format!("{target_word}: not a session"))?;
            let s = self
                .sessions
                .get(id)
                .filter(|s| s.state.running())
                .ok_or(format!("session #{id} is not running"))?;
            (Target::Session(id), s.project)
        } else if let Some((profile, project)) = target_word.split_once('@') {
            let p = self
                .projects
                .projects
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(project))
                .ok_or(format!("no project named {project}"))?;
            (
                Target::NextFree {
                    profile: profile.to_string(),
                    project: p.id,
                    project_name: p.name.clone(),
                },
                p.id,
            )
        } else {
            return Err(format!(
                "{target_word}: a session (#2) or profile@project (claude@api)"
            ));
        };
        for n in &after {
            if !crate::orchestration::events::valid_name(n) {
                return Err(format!("after:{n}: letters, digits and . _ : / # - only"));
            }
        }
        let text =
            crate::orchestration::board::clean_text(&text, crate::orchestration::tasks::TEXT_MAX)
                .map_err(|(_, m)| m)?;
        let pre = after
            .into_iter()
            .map(|n| (crate::orchestration::board::BoardId::Project(project), n))
            .collect();
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
            profiles: &self.profiles,
            guard: None,
        };
        let hub = self
            .hub
            .as_mut()
            .ok_or("Start an agent session with orchestration first")?;
        let (id, _) = hub.create_task(
            &view,
            crate::orchestration::Party::User,
            text,
            target,
            pre,
            Instant::now(),
        )?;
        Ok(id)
    }

    /// Opens the grant dialog, pre-filled from a request or from the first running sessions.
    fn open_grant(&mut self, request: Option<crate::ui::orchestration::RequestRef>) -> bool {
        use crate::ui::orchestration::GrantForm;
        use crate::ui::orchestration::RequestRef;
        let back = match &self.overlay {
            Overlay::Orch(v) => v.clone(),
            _ => OrchView::default(),
        };
        let running: Vec<u32> = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state.running())
            .map(|s| s.id)
            .collect();
        let from_request = match request {
            Some(RequestRef::Delivery(r)) => self.hub.as_ref().and_then(|h| {
                let d = h.deliveries.get(r)?;
                Some((d.from.session().unwrap_or(d.to), d.to, false))
            }),
            Some(RequestRef::Task(t)) => self.hub.as_ref().and_then(|h| {
                let t = h.tasks.get(t)?;
                let from = t.created_by.session()?;
                let to = match &t.target {
                    crate::orchestration::tasks::Target::Session(id) => *id,
                    crate::orchestration::tasks::Target::NextFree { project, .. } => running
                        .iter()
                        .copied()
                        .find(|s| self.sessions.get(*s).is_some_and(|s| s.project == *project))
                        .unwrap_or(from),
                };
                Some((from, to, true))
            }),
            Some(RequestRef::Start(st)) => self.hub.as_ref().and_then(|h| {
                let s = h.starts.iter().find(|s| s.id == st)?;
                let from = s.from.session()?;
                // A start's target is its project: any running session there stands for it.
                let to = running
                    .iter()
                    .copied()
                    .find(|r| {
                        self.sessions
                            .get(*r)
                            .is_some_and(|x| x.project == s.project)
                    })
                    .unwrap_or(from);
                Some((from, to, false))
            }),
            None => None,
        };
        let start = matches!(request, Some(RequestRef::Start(_)));
        let (from, to, tasks) = match from_request {
            Some(v) => v,
            None => {
                let Some(&a) = running.first() else {
                    self.say("Start agent sessions first");
                    return true;
                };
                (a, *running.get(1).unwrap_or(&a), false)
            }
        };
        self.overlay = Overlay::Grant(GrantForm {
            from,
            from_any: false,
            to,
            to_any: start,
            deliver: !tasks && !start,
            tasks,
            starts: u8::from(start) * 2,
            minutes: self.orch_settings.permission_default_min,
            field: 0,
            request,
            back,
            error: None,
        });
        true
    }

    /// Keys in the grant dialog.
    pub(crate) fn grant_key(&mut self, key: KeyEvent) -> bool {
        use crate::orchestration::permissions::{Grant, Scope};
        use crossterm::event::KeyCode;
        let running: Vec<u32> = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state.running())
            .map(|s| s.id)
            .collect();
        let project_of = |id: u32| self.sessions.get(id).map(|s| s.project);
        let Overlay::Grant(g) = &mut self.overlay else {
            return false;
        };
        let cycle = |cur: u32, step: isize| -> u32 {
            match running.iter().position(|s| *s == cur) {
                Some(i) if !running.is_empty() => {
                    running[(i as isize + step).rem_euclid(running.len() as isize) as usize]
                }
                _ => running.first().copied().unwrap_or(cur),
            }
        };
        g.error = None;
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::Orch(g.back.clone());
                return true;
            }
            KeyCode::Up => g.field = g.field.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                g.field = (g.field + 1).min(crate::ui::orchestration::GRANT_FIELDS - 1)
            }
            KeyCode::Left | KeyCode::Right => {
                let step: isize = if key.code == KeyCode::Left { -1 } else { 1 };
                match g.field {
                    0 => g.from = cycle(g.from, step),
                    1 => g.to = cycle(g.to, step),
                    4 => g.starts = (g.starts as isize + step).clamp(0, 9) as u8,
                    5 => {
                        let unit = if g.minutes < 10 || (g.minutes == 10 && step < 0) {
                            1
                        } else {
                            5
                        };
                        g.minutes = (g.minutes as i64 + step as i64 * unit).clamp(1, 480) as u64;
                    }
                    _ => {}
                }
            }
            KeyCode::Char(' ') => match g.field {
                0 => g.from_any = !g.from_any,
                1 => g.to_any = !g.to_any,
                2 => g.deliver = !g.deliver,
                3 => g.tasks = !g.tasks,
                4 => g.starts = if g.starts == 0 { 1 } else { 0 },
                _ => {}
            },
            KeyCode::Enter => {
                let scope = |id: u32, any: bool| match (any, project_of(id)) {
                    (true, Some(p)) => Scope::Project(p),
                    _ => Scope::Session(id),
                };
                let grant = Grant {
                    source: scope(g.from, g.from_any),
                    target: scope(g.to, g.to_any),
                    deliver: g.deliver,
                    create_task: g.tasks,
                    start_limit: (g.starts > 0).then_some(g.starts),
                    duration: std::time::Duration::from_secs(g.minutes * 60),
                };
                let (request, back) = (g.request, g.back.clone());
                let request: Option<crate::ui::orchestration::RequestRef> = request;
                let Some(hub) = self.hub.as_mut() else {
                    self.overlay = Overlay::Orch(back);
                    return true;
                };
                match hub.grant(&grant, Instant::now()) {
                    Ok(p) => {
                        match request {
                            Some(crate::ui::orchestration::RequestRef::Delivery(r)) => {
                                let _ = hub.approve(r, None);
                            }
                            Some(crate::ui::orchestration::RequestRef::Task(t)) => {
                                hub.decide_task(t, true);
                            }
                            Some(crate::ui::orchestration::RequestRef::Start(s)) => {
                                // The grant counts this start as its first use.
                                if let Some(p) = hub.permissions.list.iter_mut().find(|x| x.id == p)
                                {
                                    p.uses += 1;
                                    p.starts_used = p.starts_used.saturating_add(1);
                                }
                                hub.decide_start(s, true);
                            }
                            None => {}
                        }
                        self.overlay = Overlay::Orch(back);
                        self.say(format!("Granted P{p}"));
                    }
                    Err(e) => {
                        if let Overlay::Grant(g) = &mut self.overlay {
                            g.error = Some(e);
                        }
                    }
                }
                return true;
            }
            _ => return false,
        }
        true
    }
}

/// Why another session may not start now: 002's session limit (FR-007), the machine low on
/// memory, or Fallow's own memory near its 150 MB budget (FR-029).
pub fn start_guard(
    sessions: &SessionManager,
    snapshot: Option<&crate::procmon::snapshot::Snapshot>,
) -> Option<String> {
    if sessions.running() >= sessions.limits.max_sessions {
        return Some(format!(
            "{} sessions are running, the limit is {} (sessions.max_sessions)",
            sessions.running(),
            sessions.limits.max_sessions
        ));
    }
    let summary = &snapshot?.summary;
    if summary.low_memory {
        return Some("the machine is low on memory".into());
    }
    if summary.fallow_ram.bytes > START_MEMORY_LIMIT {
        return Some(format!(
            "Fallow uses {} MB; its budget is 150 MB",
            summary.fallow_ram.bytes / (1024 * 1024)
        ));
    }
    None
}

/// The installer the lead runs when the engineer agrees to set Spec Kit up (004 R-6). Fallow
/// itself downloads nothing and writes nothing into the project.
fn spec_kit_installer(style: Style) -> String {
    let integration = match style {
        Style::Codex => "codex",
        Style::Gemini => "gemini",
        Style::Claude | Style::None => "claude",
    };
    format!(
        "The user agreed to set up Spec Kit in this project for the coordination burst. Run its official installer here: `uvx --from git+https://github.com/github/spec-kit.git specify init --here --force --integration {integration}` (check first that uv is installed with `uv --version`; if it is missing, ask the user how to proceed instead of installing it; if the installer asks a question, ask the user). Then commit the files it added and post on the board that Spec Kit is set up."
    )
}

impl AppState {
    /// The coordination dialog for `lead`'s project (004 contracts/ui-commands.md).
    fn burst_form(&self, lead: u32, offer: bool) -> Result<BurstForm, String> {
        let s = self
            .sessions
            .get(lead)
            .filter(|s| s.state.running())
            .ok_or("Select or focus a running session first: it becomes the lead")?;
        let path = self
            .projects
            .get(s.project)
            .map(|p| p.path.clone())
            .ok_or("The session's project is not in the list")?;
        let loaded = squad::load(&path, self.store.dir.as_deref());
        let has_remote = git::has_remote(&path);
        let (publish, merge) = match &loaded {
            Ok(q) => (q.publish, q.merge),
            Err(_) => (squad::Publish::Auto, squad::Merge::OnApproval),
        };
        let mut publish = match (publish, has_remote) {
            (squad::Publish::DraftPr, _) | (squad::Publish::Auto, true) => {
                "a draft pull request by Fallow".to_string()
            }
            _ => "a local merge by Fallow".to_string(),
        };
        publish.push_str(match merge {
            squad::Merge::OnApproval => ", after your approval",
            squad::Merge::Automatic => ", when the squad's conditions are met",
        });
        let max = self.sessions.limits.max_sessions.clamp(1, 255) as u8;
        Ok(BurstForm {
            project: s.project,
            project_name: s.project_name.clone(),
            lead,
            lead_label: s.label(),
            minutes: 240,
            allowance: DEFAULT_ALLOWANCE.min(max),
            max_allowance: max,
            squad: loaded.map(|q| {
                let roles: Vec<&str> = q.roles.iter().map(|r| r.name.as_str()).collect();
                format!("{} ({})", q.name, roles.join(", "))
            }),
            publish,
            offer_spec_kit: !path.join(".specify").is_dir(),
            setup_spec_kit: false,
            offer,
            field: 0,
            error: None,
        })
    }

    /// "Create squad file": a commented template in the selected project, opened in the editor
    /// (FR-016a). An existing file is opened, never overwritten; nothing is committed.
    fn create_squad_file(&mut self) -> bool {
        let Some(path) = self.projects.selected().map(|p| p.path.clone()) else {
            self.say("Select a project first");
            return true;
        };
        let file = path.join(".fallow").join("squad.toml");
        if file.exists() {
            self.say("This project already has a squad file: opened it");
            return self.open_file(&file);
        }
        let ignore = |f: &str| {
            let low = f.to_ascii_lowercase();
            low.starts_with(".fallow/")
                || low.starts_with("readme")
                || low.starts_with("license")
                || low == ".gitignore"
                || low == ".gitattributes"
        };
        let has_code =
            git::tracked_files(&path).is_ok_and(|files| files.iter().any(|f| !ignore(f)));
        let text = squad::template(squad::guess_test_command(&path), has_code);
        let written = std::fs::create_dir_all(path.join(".fallow"))
            .and_then(|()| std::fs::write(&file, text));
        match written {
            Ok(()) => {
                self.say("Created .fallow/squad.toml: check it, then commit it with your code");
                self.open_file(&file)
            }
            Err(e) => {
                self.say(format!("{}: {e}", file.display()));
                true
            }
        }
    }

    /// "Coordinate this project…" from the palette: the active session is the lead (FR-002).
    fn open_burst(&mut self) -> bool {
        if self.hub.is_none() {
            self.say("Coordination needs orchestration: start an agent session with it first");
            return true;
        }
        let Some(lead) = self.workspace.active else {
            self.say("Select or focus a session first: it becomes the lead");
            return true;
        };
        if let Some(p) = self.sessions.get(lead).map(|s| s.project) {
            if let Some(b) = self.hub.as_ref().and_then(|h| h.burst_of(p)) {
                let name = b.project_name.clone();
                self.say(format!(
                    "{name} is already coordinating (+ extends, Shift-X ends)"
                ));
                return true;
            }
        }
        match self.burst_form(lead, false) {
            Ok(f) => self.overlay = Overlay::Burst(f),
            Err(e) => self.say(e),
        }
        true
    }

    /// Starts a burst in `project` with `lead` (FR-001), reading the squad and git state now.
    fn start_burst_now(
        &mut self,
        project: crate::projects::ProjectId,
        lead: u32,
        duration: std::time::Duration,
        allowance: Option<u8>,
        setup_spec_kit: bool,
    ) -> Result<(), String> {
        let path = self
            .projects
            .get(project)
            .map(|p| p.path.clone())
            .ok_or("The project is not in the list")?;
        let squad = squad::load(&path, self.store.dir.as_deref())
            .map_err(|p| format!("Fix {} first: {}", squad::SQUAD_FILE, p.join("; ")))?;
        let request = BurstRequest {
            lead,
            duration,
            allowance,
            squad,
            start_commit: git::head(&path).ok(),
            target_branch: git::branch(&path),
            has_remote: git::has_remote(&path),
            spec_kit: path.join(".specify").is_dir(),
            max_sessions: self.sessions.limits.max_sessions.clamp(1, 255) as u8,
        };
        let style = self
            .sessions
            .get(lead)
            .map_or(Style::None, |s| s.profile.orchestration);
        let hub = self
            .hub
            .as_mut()
            .ok_or("Coordination needs orchestration: start an agent session with it first")?;
        let view = AppView {
            sessions: &self.sessions,
            projects: &self.projects,
            profiles: &self.profiles,
            guard: None,
        };
        let now = Instant::now();
        hub.start_burst(&view, request, now)?;
        self.bursts_started += 1;
        if setup_spec_kit {
            hub.request_delivery(
                &view,
                crate::orchestration::Party::User,
                lead,
                spec_kit_installer(style),
                crate::orchestration::delivery::Purpose::Message,
                true,
                now,
            )?;
        }
        if let Some(s) = self.sessions.get_mut(lead) {
            s.role = Some("lead".into());
        }
        self.take_hub_notices();
        Ok(())
    }

    /// After a session started: `--coordinate` starts the burst with it, or the first session of
    /// a project offers coordination once per run (FR-002).
    pub(crate) fn after_first_session(&mut self, project: crate::projects::ProjectId, sid: u32) {
        let path = self.projects.get(project).map(|p| p.path.clone());
        if let Some((d, want)) = self.pending_coordinate.clone() {
            let ours = match (&want, &path) {
                (None, _) => true,
                (Some(w), Some(p)) => crate::projects::same_path(w, p),
                _ => false,
            };
            if ours {
                self.pending_coordinate = None;
                if let Err(e) = self.start_burst_now(project, sid, d, None, false) {
                    self.say(format!("--coordinate: {e}"));
                }
                return;
            }
        }
        let Some(hub) = self.hub.as_ref() else {
            return;
        };
        if hub.burst_of(project).is_some()
            || self.coordinate_offered.contains(&project)
            || !matches!(self.overlay, Overlay::None)
        {
            return;
        }
        let connected = self
            .sessions
            .get(sid)
            .is_some_and(|s| s.state.running() && s.orch.token.is_some());
        let running = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.project == project && s.state.running())
            .count();
        let declined = path.as_ref().is_some_and(|p| {
            self.projects
                .no_coordinate_offer
                .iter()
                .any(|q| crate::projects::same_path(q, p))
        });
        if !connected || running != 1 || declined {
            return;
        }
        self.coordinate_offered.push(project);
        if let Ok(f) = self.burst_form(sid, true) {
            self.overlay = Overlay::Burst(f);
        }
    }

    /// Keys in the coordination dialog.
    pub(crate) fn burst_key(&mut self, key: KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        let Overlay::Burst(b) = &mut self.overlay else {
            return false;
        };
        b.error = None;
        let fields = if b.offer_spec_kit { 3 } else { 2 };
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up => b.field = b.field.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => b.field = (b.field + 1).min(fields - 1),
            KeyCode::Left | KeyCode::Right => {
                let step: i64 = if key.code == KeyCode::Left { -1 } else { 1 };
                match b.field {
                    0 => b.minutes = (b.minutes as i64 + step * 30).clamp(30, 480) as u64,
                    1 => {
                        b.allowance = (i64::from(b.allowance) + step)
                            .clamp(1, i64::from(b.max_allowance))
                            as u8
                    }
                    _ => b.setup_spec_kit = !b.setup_spec_kit,
                }
            }
            KeyCode::Char(' ') if b.field == 2 => b.setup_spec_kit = !b.setup_spec_kit,
            KeyCode::Char('d' | 'D') if b.offer => {
                let project = b.project;
                self.overlay = Overlay::None;
                if let Some(p) = self.projects.get(project).map(|p| p.path.clone()) {
                    self.projects.no_coordinate_offer.push(p);
                    self.save_projects();
                    self.say("Fallow won't offer coordination for this project again");
                }
            }
            KeyCode::Enter => {
                if b.squad.is_err() {
                    b.error = Some(format!(
                        "Fix {} first, or remove it to use the built-in squad",
                        squad::SQUAD_FILE
                    ));
                    return true;
                }
                let (project, lead, minutes, allowance, setup) = (
                    b.project,
                    b.lead,
                    b.minutes,
                    b.allowance,
                    b.offer_spec_kit && b.setup_spec_kit,
                );
                let d = std::time::Duration::from_secs(minutes * 60);
                match self.start_burst_now(project, lead, d, Some(allowance), setup) {
                    Ok(()) => self.overlay = Overlay::None,
                    Err(e) => {
                        if let Overlay::Burst(b) = &mut self.overlay {
                            b.error = Some(e);
                        }
                    }
                }
            }
            _ => {}
        }
        true
    }
}
