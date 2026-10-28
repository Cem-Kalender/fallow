//! Recorded runs in the app (005 US2, research R-11, R-13, R-15): starting a run in its own copy
//! of the project, typing its description, measuring it, ending it, closing its sessions after
//! the engineer confirms, running the hidden tests, writing the record, and series.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::editing::Pending;
use super::state::{AppState, Overlay};
use crate::orchestration::burst::{self, BurstRequest};
use crate::orchestration::record::{self, RunRecord};
use crate::orchestration::runs::{self, End, Measured, Mode, Plan, Run, Series, Stage, TaskSet};
use crate::orchestration::{Sessions, git, roles, squad};
use crate::session::profiles::Style;

/// Hidden tests get this long each, at most (the run's own time limit if shorter).
const TEST_LIMIT: Duration = Duration::from_secs(30 * 60);
/// A burst for a run lasts the time limit plus this margin (at most 8 hours).
const BURST_MARGIN: Duration = Duration::from_secs(10 * 60);

/// What the app keeps for runs.
#[derive(Debug, Default)]
pub struct Runs {
    pub sets: Vec<TaskSet>,
    pub run: Option<Run>,
    pub series: Option<Series>,
    next: u64,
    /// When the close confirmation was last shown, so it is not reopened every tick.
    asked: bool,
    /// When the running hidden test started.
    test_started: Option<Instant>,
}

impl AppState {
    /// Registers a task set (FR-011) and shows the hidden-tests notice.
    pub fn register_task_set(&mut self, dir: &Path) -> Result<String, String> {
        let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let set = runs::load(&dir).map_err(|p| p.join("; "))?;
        let n = set.tasks.len();
        self.runs.sets.retain(|s| s.dir != dir);
        self.runs.sets.push(set);
        let dirs: Vec<String> = self
            .runs
            .sets
            .iter()
            .map(|s| s.dir.display().to_string())
            .collect();
        self.orch_settings.task_sets = dirs;
        Ok(format!(
            "Task set registered: {n} task(s). Hidden tests are kept out of every run until its sessions are closed. Agents run as you, so one could read them on purpose."
        ))
    }

    /// The registered task sets, loaded on first use (never at startup, Principle II).
    pub fn task_sets(&mut self) -> &[TaskSet] {
        if self.runs.sets.is_empty() {
            for d in self.orch_settings.task_sets.clone() {
                if let Ok(s) = runs::load(Path::new(&d)) {
                    self.runs.sets.push(s);
                }
            }
        }
        &self.runs.sets
    }

    /// Starts a run (FR-011a, FR-012, R-13).
    pub fn start_run(&mut self, plan: Plan) -> Result<u64, String> {
        if self.runs.run.as_ref().is_some_and(Run::in_progress) {
            return Err("A run is in progress: end it first".into());
        }
        self.task_sets();
        let set = self
            .runs
            .sets
            .iter()
            .find(|s| s.dir == plan.set)
            .cloned()
            .ok_or("That task set is not registered")?;
        let task = set
            .task(&plan.task)
            .cloned()
            .ok_or_else(|| format!("No task {:?} in the task set", plan.task))?;
        let cache = crate::config::cache_root().ok_or("no cache folder is known")?;
        self.runs.next += 1;
        let id = self.runs.next;
        let root = cache
            .join("runs")
            .join("work")
            .join(format!("{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let copy = root.join(crate::projects::folder_name(&task.project));
        git::copy_at(&task.project, &task.start, &copy)
            .map_err(|e| format!("Could not copy the project at {}: {e}", task.start))?;
        for c in &set.setup {
            let to = copy.join(&c.to);
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::copy(&c.from, &to).map_err(|e| format!("{}: {e}", c.to))?;
        }
        let start_commit = git::head(&copy)?;
        let task_fp = runs::task_fingerprint(&task)?;
        let hidden_fp = runs::hidden_fingerprint(&task)?;
        let project = self
            .projects
            .push_temporary(copy.clone(), &format!("run {id} {}", task.name));
        let agent = plan_agent(&plan);
        let profile = self
            .profiles
            .iter()
            .find(|p| p.id == agent)
            .cloned()
            .ok_or_else(|| format!("no profile {agent:?}"))?;
        // Every editing session runs at the same level, never asking the engineer (FR-012).
        let level = roles::level_flags(profile.orchestration, squad::Level::EditRun)
            .ok_or_else(|| format!("{} has no flags for running unattended", profile.name))?;
        let mut run = Run {
            id,
            plan: plan.clone(),
            task: task.clone(),
            task_fp,
            hidden_fp,
            copy: copy.clone(),
            root,
            project,
            start_commit: start_commit.clone(),
            first: None,
            sessions: BTreeSet::new(),
            started_at: Instant::now(),
            started: SystemTime::now(),
            ended_at: None,
            stage: Stage::Starting,
            typed: false,
            idle_since: None,
            idle_end: runs::DEFAULT_IDLE_END,
            measured: Default::default(),
            last_seen: None,
            final_commit: None,
            tested_dir: None,
            snapshot: None,
        };
        // The run's idle end comes from the copy's squad file in every mode (R-13).
        if let Ok(s) = squad::load(&copy, self.store.dir.as_deref()) {
            run.idle_end = Duration::from_secs(u64::from(s.run_idle_end_min) * 60);
        }
        if plan.mode != Mode::Plain {
            self.ensure_hub(profile.orchestration);
        }
        let sid = self.start_in_run(&run, &profile.id, level, plan.mode == Mode::Plain)?;
        run.first = Some(sid);
        run.sessions.insert(sid);
        if plan.mode != Mode::Plain {
            let s = squad::load(&copy, self.store.dir.as_deref()).map_err(|p| p.join("; "))?;
            run.idle_end = Duration::from_secs(u64::from(s.run_idle_end_min) * 60);
            let max = self.sessions.limits.max_sessions.clamp(2, 255) as u8;
            let req = BurstRequest {
                lead: sid,
                duration: (task.time_limit + BURST_MARGIN).min(Duration::from_secs(8 * 3600)),
                allowance: Some(max - 1),
                squad: s,
                start_commit: Some(start_commit),
                target_branch: Some("main".into()),
                has_remote: false,
                spec_kit: copy.join(".specify").is_dir(),
                max_sessions: max,
            };
            let view = super::orchestration::AppView {
                sessions: &self.sessions,
                projects: &self.projects,
                profiles: &self.profiles,
                guard: super::orchestration::start_guard(&self.sessions, self.snapshot.as_deref()),
            };
            let Some(h) = self.hub.as_mut() else {
                return Err("orchestration is unavailable".into());
            };
            h.start_burst(&view, req, Instant::now())?;
            if let Some(b) = h.bursts.iter_mut().find(|b| b.project == project) {
                b.description = Some(task.description.clone());
                b.switched_off = plan.off.clone();
                if plan.mode == Mode::Solo {
                    b.switched_off.insert(burst::OFF_PARALLEL);
                }
            }
            self.bursts_started += 1;
            self.take_hub_notices();
        }
        self.runs.run = Some(run);
        self.runs.asked = false;
        self.say(format!(
            "Run {id}: {} as {} started in its own copy",
            task.name,
            plan.mode.label()
        ));
        Ok(id)
    }

    /// Starts the run's first session in the copy's folder.
    fn start_in_run(
        &mut self,
        run: &Run,
        profile: &str,
        level: Vec<String>,
        plain: bool,
    ) -> Result<u32, String> {
        let profile = self
            .profiles
            .iter()
            .find(|p| p.id == profile)
            .cloned()
            .ok_or("no such profile")?;
        let tx = self
            .events
            .clone()
            .ok_or("Sessions are not available in this mode")?;
        let area = crate::ui::workspace::session_rect(self);
        let p = self.projects.get(run.project).ok_or("the run's project")?;
        let req = crate::session::StartRequest {
            project: p.id,
            project_name: p.name.clone(),
            cwd: run.copy.clone(),
            workdir: crate::session::Workdir::ProjectFolder,
            profile,
            rows: area.height.max(1),
            cols: area.width.max(1),
        };
        let sid = self
            .sessions
            .start_run_session(req, None, level, plain, true, &tx)?;
        self.track(sid);
        Ok(sid)
    }

    /// The run's periodic work: typing, measuring, the end rule, closing, hidden tests.
    pub fn tick_runs(&mut self) -> bool {
        let now = Instant::now();
        let Some(mut run) = self.runs.run.take() else {
            return self.next_in_series();
        };
        let changed = self.step_run(&mut run, now);
        let finished = !run.in_progress();
        self.runs.run = Some(run);
        if finished {
            self.next_in_series();
        }
        changed
    }

    fn step_run(&mut self, run: &mut Run, now: Instant) -> bool {
        // Every session of the run's project belongs to it.
        for s in &self.sessions.sessions {
            if s.project == run.project {
                run.sessions.insert(s.id);
            }
        }
        match run.stage.clone() {
            Stage::Starting | Stage::Running => {
                // A first session that is gone, or a time limit reached before the description
                // could be typed, ends the run (nothing waits forever).
                let first_gone = run
                    .first
                    .and_then(|f| self.sessions.get(f))
                    .is_none_or(|s| !s.state.running());
                if !run.typed
                    && (first_gone
                        || now.saturating_duration_since(run.started_at) >= run.task.time_limit)
                {
                    self.end_run(run, End::Idle);
                    return true;
                }
                if !run.typed {
                    let ready = run
                        .first
                        .and_then(|f| self.sessions.get(f))
                        .is_some_and(|s| {
                            s.state.running()
                                && !matches!(s.state, crate::session::SessionState::Starting)
                                && s.state_since.elapsed() > Duration::from_secs(2)
                                && now.saturating_duration_since(s.last_output)
                                    > Duration::from_secs(2)
                        });
                    if ready {
                        if let Some(s) = run.first.and_then(|f| self.sessions.get(f)) {
                            // As deliveries are typed: one paste, or one line when the agent
                            // takes no pastes (each line break would be Enter), then Enter.
                            let (_, bracketed, _) = s.modes();
                            let text = if bracketed {
                                run.description()
                            } else {
                                run.description()
                                    .replace("\r\n", "\n")
                                    .split('\n')
                                    .filter(|l| !l.trim().is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" / ")
                            };
                            s.write(&crate::session::keys::paste(&text, bracketed));
                            s.write(b"\r");
                        }
                        run.typed = true;
                        super::diag::mark("run_typed", run.id);
                        run.stage = Stage::Running;
                        run.started_at = now;
                        return true;
                    }
                    return false;
                }
                self.measure(run, now);
                let (working, in_progress) = self.run_activity(run);
                run.observe_idle(now, working, in_progress);
                if let Some(why) = run.due_end(now, working, in_progress) {
                    self.end_run(run, why);
                    return true;
                }
                false
            }
            Stage::Ended(_) => {
                if !self.runs.asked && matches!(self.overlay, Overlay::None) {
                    self.ask_close(run);
                    self.runs.asked = true;
                    return true;
                }
                false
            }
            Stage::Closing(_) => {
                let still = run
                    .sessions
                    .iter()
                    .any(|id| self.sessions.get(*id).is_some_and(|s| s.state.running()));
                if still {
                    return false;
                }
                self.prepare_hidden_tests(run);
                true
            }
            Stage::Testing {
                next,
                running,
                passed,
                failed,
            } => {
                if let Some(id) = running {
                    let limit = run.task.time_limit.min(TEST_LIMIT);
                    if self.runs.test_started.is_some_and(|t| t.elapsed() > limit) {
                        let _ = self.sessions.end_gate(id);
                    }
                    return false;
                }
                if next < run.task.tests.len() {
                    match self.start_hidden_test(run, next) {
                        Ok(id) => {
                            run.stage = Stage::Testing {
                                next,
                                running: Some(id),
                                passed,
                                failed,
                            };
                            self.runs.test_started = Some(now);
                        }
                        Err(_) => {
                            run.stage = Stage::Testing {
                                next: next + 1,
                                running: None,
                                passed,
                                failed: failed + 1,
                            };
                        }
                    }
                    return true;
                }
                self.finish_run(run, passed, failed);
                true
            }
            Stage::Recorded | Stage::Discarded => false,
        }
    }

    /// Time working and blocked per session, and reported cost (FR-014, FR-015).
    fn measure(&mut self, run: &mut Run, now: Instant) {
        let dt = run
            .last_seen
            .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
        run.last_seen = Some(now);
        let hub = self.hub.as_ref();
        let ids: Vec<u32> = run.sessions.iter().copied().collect();
        for id in ids {
            let Some(s) = self.sessions.get(id) else {
                continue;
            };
            if s.role.as_deref() == Some(crate::orchestration::gates::GATE_ROLE) {
                continue;
            }
            let info = self.sessions.info(id);
            let working = info.as_ref().is_some_and(|i| i.running && !i.idle);
            let blocked = hub.is_some_and(|h| h.blocked(id));
            run.add_time(id, working, blocked, dt);
            let report = s.term.lock().ok().and_then(|mut t| {
                let n = t.total_lines();
                let lines: Vec<String> = (n.saturating_sub(8)..n).map(|v| t.line_text(v)).collect();
                crate::orchestration::usage::read(&lines)
            });
            if let Some(r) = report {
                let m = run.measured.entry(id).or_default();
                if r.cost_usd.is_some() {
                    m.cost_usd = r.cost_usd;
                }
                if r.model.is_some() {
                    m.model = r.model;
                }
                if r.version.is_some() {
                    m.version = r.version;
                }
            }
        }
    }

    /// Whether any session of the run works, and whether anything of it is in progress (a gate,
    /// a start, a hand-over) (FR-013).
    fn run_activity(&self, run: &Run) -> (bool, bool) {
        let gate = |id: &u32| {
            self.sessions
                .get(*id)
                .is_some_and(|s| s.role.as_deref() == Some(crate::orchestration::gates::GATE_ROLE))
        };
        let working = run.sessions.iter().filter(|id| !gate(id)).any(|id| {
            self.sessions
                .info(*id)
                .is_some_and(|i| i.running && !i.idle)
        });
        let gates = run
            .sessions
            .iter()
            .filter(|id| gate(id))
            .any(|id| self.sessions.get(*id).is_some_and(|s| s.state.running()));
        let hub_busy = self
            .hub
            .as_ref()
            .is_some_and(|h| h.run_busy(run.project, &run.sessions));
        (working, gates || hub_busy)
    }

    /// Engineer's "End the run" (counted as an intervention).
    pub fn end_run_now(&mut self) -> bool {
        let Some(mut run) = self.runs.run.take() else {
            self.say("No run is in progress");
            return true;
        };
        if matches!(run.stage, Stage::Starting | Stage::Running) {
            if let Some(f) = run.first {
                run.intervention(f);
            }
            self.end_run(&mut run, End::Engineer);
        }
        self.runs.run = Some(run);
        true
    }

    fn end_run(&mut self, run: &mut Run, why: End) {
        super::diag::mark("run_end", why as u64);
        run.ended_at = Some(Instant::now());
        run.stage = Stage::Ended(why);
        // The record's view of the squad, before the burst ends.
        if let Some(h) = self.hub.as_mut() {
            run.snapshot = h.run_snapshot(run.project);
            h.end_burst(run.project, "the run ended");
        }
        self.take_hub_notices();
        self.runs.asked = false;
        self.say(format!(
            "Run {} ended ({}): confirm closing its sessions to run the hidden tests",
            run.id,
            match why {
                End::Idle => "nothing worked for a while",
                End::TimeLimit => "time limit",
                End::Engineer => "ended by you",
            }
        ));
    }

    /// The one confirmation per run that names its sessions (Principle IV, FR-013).
    fn ask_close(&mut self, run: &Run) {
        let ids: Vec<u32> = run
            .sessions
            .iter()
            .copied()
            .filter(|id| self.sessions.get(*id).is_some_and(|s| s.state.running()))
            .collect();
        if ids.is_empty() {
            return self.close_confirmed_none(run.id);
        }
        let names: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
        let d =
            crate::ui::confirm::Dialog::new(
                &format!("Run {} finished", run.id),
                &crate::session::run_subject(run.id, &ids),
                vec![
                format!("Close its sessions {} and run the hidden tests?", names.join(", ")),
                "The sessions are stopped gracefully; the run's copy is removed after the record."
                    .into(),
            ],
                "Close and test",
            );
        self.overlay = Overlay::Confirm {
            dialog: d,
            action: Pending::EndRun {
                run: run.id,
                sessions: ids,
            },
        };
    }

    fn close_confirmed_none(&mut self, run: u64) {
        if let Some(r) = self.runs.run.as_mut().filter(|r| r.id == run) {
            if let Stage::Ended(why) = r.stage {
                r.stage = Stage::Closing(why);
            }
        }
    }

    /// The engineer confirmed closing a run's sessions.
    pub(super) fn close_run(
        &mut self,
        run: u64,
        ids: &[u32],
        token: crate::ui::confirm::ConfirmedToken,
    ) {
        self.sessions.stop_run_confirmed(run, ids, token);
        if let Some(r) = self.runs.run.as_mut().filter(|r| r.id == run) {
            if let Stage::Ended(why) = r.stage {
                r.stage = Stage::Closing(why);
            }
        }
    }

    /// The tested state: the lead's (or plain session's) worktree as the run left it.
    fn prepare_hidden_tests(&mut self, run: &mut Run) {
        let dir = run
            .first
            .and_then(|f| self.sessions.get(f))
            .map_or(run.copy.clone(), |s| s.cwd.clone());
        run.final_commit = git::head(&dir).ok();
        let mut failed = 0;
        for h in &run.task.hidden {
            let to = dir.join(&h.to);
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&h.from, &to).is_err() {
                failed = run.task.tests.len() as u32;
            }
        }
        run.tested_dir = Some(dir);
        run.stage = if failed > 0 {
            Stage::Testing {
                next: run.task.tests.len(),
                running: None,
                passed: 0,
                failed,
            }
        } else {
            Stage::Testing {
                next: 0,
                running: None,
                passed: 0,
                failed: 0,
            }
        };
    }

    /// A hidden test as a command session (a gate's kind: no orchestration, ended at its limit).
    fn start_hidden_test(&mut self, run: &Run, i: usize) -> Result<u32, String> {
        use crate::orchestration::gates;
        let dir = run.tested_dir.clone().ok_or("no tested folder")?;
        let test = &run.task.tests[i];
        let (program, args) = gates::shell_command(test.command_for(std::env::consts::OS));
        let profile = crate::session::profiles::AgentProfile {
            id: gates::GATE_PROFILE.into(),
            name: format!("hidden test {} of run {}", i + 1, run.id),
            command: program,
            args,
            env: Vec::new(),
            builtin: false,
            orchestration: Style::None,
        };
        let tx = self
            .events
            .clone()
            .ok_or("Sessions are not available in this mode")?;
        let area = crate::ui::workspace::session_rect(self);
        let p = self.projects.get(run.project).ok_or("the run's project")?;
        let req = crate::session::StartRequest {
            project: p.id,
            project_name: p.name.clone(),
            cwd: dir,
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

    /// A session ended; a hidden test's exit code is its outcome.
    pub fn run_session_exited(&mut self, id: u32, code: Option<i32>) {
        let Some(run) = self.runs.run.as_mut() else {
            return;
        };
        if let Stage::Testing {
            next,
            running: Some(r),
            passed,
            failed,
        } = run.stage
        {
            if r == id {
                let ok = code == Some(0);
                run.stage = Stage::Testing {
                    next: next + 1,
                    running: None,
                    passed: passed + u32::from(ok),
                    failed: failed + u32::from(!ok),
                };
            }
        }
    }

    fn finish_run(&mut self, run: &mut Run, passed: u32, failed: u32) {
        let mut note = format!(
            "Run {} ({} {}): hidden tests {passed} passed, {failed} failed",
            run.id,
            run.task.name,
            run.plan.mode.label()
        );
        if run.plan.record {
            match crate::config::cache_root()
                .ok_or_else(|| "no cache folder".to_string())
                .and_then(|c| record::write(&c, &build_record(run, &self.sessions, passed, failed)))
            {
                Ok(n) => note.push_str(&format!("; recorded as {n}")),
                Err(e) => note.push_str(&format!("; not recorded: {e}")),
            }
        }
        self.discard_copy(run);
        run.stage = Stage::Recorded;
        self.say(note);
    }

    /// Closes the run's exited sessions, removes its temporary project and its copy.
    fn discard_copy(&mut self, run: &Run) {
        for id in &run.sessions {
            let _ = self.sessions.close(*id);
        }
        self.projects.remove_temporary(run.project);
        let _ = std::fs::remove_dir_all(&run.root);
    }

    /// A run in progress when Fallow exits is discarded (FR-019a); its copy is removed when the
    /// next run starts or the Runs tab opens (T028), never at startup.
    pub fn discard_leftover_copies(&mut self) {
        let Some(cache) = crate::config::cache_root() else {
            return;
        };
        let work = cache.join("runs").join("work");
        let mine = format!("{}-", std::process::id());
        if let Ok(entries) = std::fs::read_dir(&work) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with(&mine) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }

    /// Starts the next run of a series when none is in progress (FR-018).
    fn next_in_series(&mut self) -> bool {
        if self.runs.run.as_ref().is_some_and(Run::in_progress) {
            return false;
        }
        let Some(plan) = self.runs.series.as_mut().and_then(Series::take_next) else {
            if self.runs.series.take().is_some() {
                self.say("The series is complete (Runs tab)");
                return true;
            }
            return false;
        };
        if let Err(e) = self.start_run(plan) {
            self.say(format!("The series stopped: {e}"));
            self.runs.series = None;
        }
        true
    }

    /// Starts a series (FR-018): every task × mode × repeat, one at a time, shuffled.
    pub fn start_series(
        &mut self,
        set: PathBuf,
        tasks: Vec<String>,
        modes: Vec<Mode>,
        repeats: usize,
        off: BTreeSet<&'static str>,
    ) -> Result<usize, String> {
        if self.runs.run.as_ref().is_some_and(Run::in_progress) || self.runs.series.is_some() {
            return Err("A run or series is in progress".into());
        }
        let series = Series::new(&set, &tasks, &modes, repeats, &off, runs::clock_seed());
        let n = series.queue.len();
        self.runs.series = Some(series);
        self.next_in_series();
        Ok(n)
    }

    /// The engineer typed into a session or decided something during a run (FR-014).
    pub fn note_intervention(&mut self, session: Option<u32>) {
        if let Some(run) = self
            .runs
            .run
            .as_mut()
            .filter(|r| matches!(r.stage, Stage::Starting | Stage::Running))
        {
            let target = session.filter(|s| run.sessions.contains(s)).or(run.first);
            if let Some(t) = target {
                run.intervention(t);
            }
        }
    }

    /// Whether `session` belongs to the run in progress.
    pub fn in_run(&self, session: u32) -> bool {
        self.runs
            .run
            .as_ref()
            .is_some_and(|r| r.in_progress() && r.sessions.contains(&session))
    }
}

/// The plan's agent profile (`claude` unless the plan's components name one).
fn plan_agent(plan: &Plan) -> String {
    plan.agent.clone()
}

/// The record of a run (contracts/run-record.md): numbers and fixed values only.
pub fn build_record(
    run: &Run,
    sessions: &crate::session::SessionManager,
    passed: u32,
    failed: u32,
) -> RunRecord {
    let snap = run.snapshot.clone().unwrap_or_default();
    // Roles as numbers: 1 is the first session's role (lead, or plain); then by first appearance.
    let mut role_names: Vec<String> = Vec::new();
    let role_of = |id: u32| -> String {
        if Some(id) == run.first {
            return "first".into();
        }
        snap.roles
            .iter()
            .find(|(s, _)| *s == id)
            .map_or_else(|| "other".into(), |(_, r)| r.clone())
    };
    let mut ids: Vec<u32> =
        run.sessions
            .iter()
            .copied()
            .filter(|id| {
                sessions.get(*id).is_none_or(|s| {
                    s.role.as_deref() != Some(crate::orchestration::gates::GATE_ROLE)
                }) && snap
                    .roles
                    .iter()
                    .all(|(s, r)| *s != *id || r != crate::orchestration::gates::GATE_ROLE)
            })
            .collect();
    ids.sort_unstable_by_key(|id| (Some(*id) != run.first, *id));
    let mut role_no = |name: String| -> u32 {
        match role_names.iter().position(|r| *r == name) {
            Some(i) => i as u32 + 1,
            None => {
                role_names.push(name);
                role_names.len() as u32
            }
        }
    };
    let mut rows = Vec::new();
    let mut per_role: std::collections::BTreeMap<u32, (u64, u64, u32, Option<f64>, bool)> =
        Default::default();
    for (n, id) in ids.iter().enumerate() {
        let role = role_no(role_of(*id));
        let m = run.measured.get(id).cloned().unwrap_or_default();
        let agent = sessions
            .get(*id)
            .map(|s| {
                if crate::session::profiles::builtins()
                    .iter()
                    .any(|b| b.id == s.profile.id)
                {
                    s.profile.id.clone()
                } else {
                    "custom".into()
                }
            })
            .unwrap_or_else(|| "custom".into());
        rows.push(record::SessionRow {
            n: n as u32 + 1,
            role,
            agent,
            model: m.model.clone(),
            version: m.version.clone(),
        });
        let e = per_role.entry(role).or_insert((0, 0, 0, Some(0.0), true));
        e.0 += m.working.as_secs();
        e.1 += m.blocked.as_secs();
        e.2 += m.interventions;
        match (e.3, m.cost_usd) {
            (Some(a), Some(c)) => e.3 = Some(a + c),
            _ => e.3 = None,
        }
    }
    let measured = |pred: &dyn Fn(u32) -> bool| -> (u64, u64) {
        run.measured
            .iter()
            .filter(|(id, _)| pred(**id))
            .fold((0, 0), |(w, b), (_, m): (&u32, &Measured)| {
                (w + m.working.as_secs(), b + m.blocked.as_secs())
            })
    };
    // Parts as numbers; without parallel parts, the whole work is part 1.
    let mut part_names: Vec<String> = snap.parts.iter().filter_map(|(_, p)| p.clone()).collect();
    part_names.sort();
    part_names.dedup();
    let mut parts = Vec::new();
    if part_names.is_empty() {
        let (w, b) = measured(&|_| true);
        parts.push(record::PartRow {
            n: 1,
            working_s: w,
            blocked_s: b,
            signals: snap.signals.iter().map(|(_, n)| *n).sum(),
            repair_rounds: snap.repairs.iter().map(|(_, n)| *n).sum(),
            gates: snap.gates.clone(),
            reviews: snap.reviews.clone(),
        });
    } else {
        for (i, p) in part_names.iter().enumerate() {
            let members: Vec<u32> = snap
                .parts
                .iter()
                .filter(|(_, q)| q.as_deref() == Some(p.as_str()))
                .map(|(s, _)| *s)
                .collect();
            let (w, b) = measured(&|id| members.contains(&id));
            parts.push(record::PartRow {
                n: i as u32 + 1,
                working_s: w,
                blocked_s: b,
                signals: snap
                    .signals
                    .iter()
                    .filter(|(s, _)| members.contains(s))
                    .map(|(_, n)| *n)
                    .sum(),
                repair_rounds: snap
                    .repairs
                    .iter()
                    .filter(|(q, _)| q.as_deref() == Some(p.as_str()))
                    .map(|(_, n)| *n)
                    .sum(),
                gates: snap.gates.clone(),
                reviews: snap.reviews.clone(),
            });
        }
    }
    let duration = run.ended_at.map_or(Duration::ZERO, |e| {
        e.saturating_duration_since(run.started_at)
    });
    RunRecord {
        mode: run.plan.mode.label().into(),
        switched_off: run.plan.off.iter().map(|c| (*c).to_string()).collect(),
        fingerprints: record::Fingerprints {
            task: run.task_fp.clone(),
            hidden: run.hidden_fp.clone(),
        },
        sessions: rows,
        start_commit: run.start_commit.clone(),
        final_commit: run.final_commit.clone(),
        started: crate::orchestration::tools::rfc3339(run.started),
        duration_s: duration.as_secs(),
        time_limit_s: run.task.time_limit.as_secs(),
        idle_end_s: run.idle_end.as_secs(),
        parts,
        per_role: per_role
            .into_iter()
            .map(|(role, (w, b, i, c, _))| record::RoleRow {
                role,
                working_s: w,
                blocked_s: b,
                interventions: i,
                cost_usd: c,
                tokens: None,
            })
            .collect(),
        hidden: record::Hidden { passed, failed },
    }
}

/// The component names a run line may switch off.
fn components(list: &str) -> Result<BTreeSet<&'static str>, String> {
    let mut out = BTreeSet::new();
    for c in list.split(',').filter(|c| !c.is_empty()) {
        match burst::COMPONENTS.iter().find(|x| **x == c) {
            Some(x) => {
                out.insert(*x);
            }
            None => {
                return Err(format!(
                    "unknown component {c:?}: {}",
                    burst::COMPONENTS.join(", ")
                ));
            }
        }
    }
    Ok(out)
}

/// `<task> <plain|solo|squad> [off=a,b] [agent=id] [norecord]` (contracts/ui-commands.md).
pub fn parse_run_line(line: &str, sets: &[TaskSet]) -> Result<Plan, String> {
    let mut words = line.split_whitespace();
    let task = words.next().ok_or("name a task")?;
    let mode = words
        .next()
        .and_then(Mode::parse)
        .ok_or("then a mode: plain, solo or squad")?;
    let set = sets
        .iter()
        .find(|s| s.task(task).is_some())
        .ok_or_else(|| format!("no registered task set has a task {task:?}"))?;
    let mut plan = Plan {
        set: set.dir.clone(),
        task: task.to_string(),
        mode,
        off: BTreeSet::new(),
        record: true,
        agent: "claude".into(),
    };
    for w in words {
        if let Some(list) = w.strip_prefix("off=") {
            plan.off = components(list)?;
        } else if let Some(a) = w.strip_prefix("agent=") {
            plan.agent = a.to_string();
        } else if w == "norecord" {
            plan.record = false;
        } else {
            return Err(format!("{w:?}: expected off=…, agent=… or norecord"));
        }
    }
    Ok(plan)
}

type SeriesLine = (
    PathBuf,
    Vec<String>,
    Vec<Mode>,
    usize,
    BTreeSet<&'static str>,
);

/// `[tasks=a,b|all] [modes=plain,solo,squad] [repeats=3] [off=a,b]`, from the first registered
/// task set (or the one holding the named tasks).
pub fn parse_series_line(line: &str, sets: &[TaskSet]) -> Result<SeriesLine, String> {
    let mut tasks: Option<Vec<String>> = None;
    let mut modes = Mode::ALL.to_vec();
    let mut repeats = 3;
    let mut off = BTreeSet::new();
    for w in line.split_whitespace() {
        if let Some(t) = w.strip_prefix("tasks=") {
            if t != "all" {
                tasks = Some(t.split(',').map(str::to_string).collect());
            }
        } else if let Some(m) = w.strip_prefix("modes=") {
            modes = m
                .split(',')
                .map(|x| Mode::parse(x).ok_or_else(|| format!("unknown mode {x:?}")))
                .collect::<Result<_, _>>()?;
        } else if let Some(r) = w.strip_prefix("repeats=") {
            repeats = r
                .parse::<usize>()
                .ok()
                .filter(|r| (1..=20).contains(r))
                .ok_or("repeats: 1..=20")?;
        } else if let Some(list) = w.strip_prefix("off=") {
            off = components(list)?;
        } else {
            return Err(format!("{w:?}: expected tasks=, modes=, repeats= or off="));
        }
    }
    let set = match &tasks {
        Some(t) => sets
            .iter()
            .find(|s| t.iter().all(|n| s.task(n).is_some()))
            .ok_or("no registered task set has all those tasks")?,
        None => sets.first().ok_or("register a task set first")?,
    };
    let tasks = tasks.unwrap_or_else(|| set.tasks.iter().map(|t| t.name.clone()).collect());
    Ok((set.dir.clone(), tasks, modes, repeats, off))
}
