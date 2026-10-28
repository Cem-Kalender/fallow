//! Gate runs, failure names and classification against the baseline (004 US4, research R-5, R-6).
//!
//! A gate is a command session (the shell profile, no orchestration, role `gate`). Its exit
//! status is the only input for `gate-<name>-passed/-failed`, which Fallow signals itself
//! (FR-020). Failing checks are named from the session's output, in memory, so that a final run
//! can be compared with the baseline (FR-021).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;

use super::activity::Action;
use super::board::BoardId;
use super::squad::{Failures, Gate};
use super::{Hub, Party, TEXT_BUDGET, tasks};
use crate::projects::ProjectId;

/// The role name of gate sessions.
pub const GATE_ROLE: &str = "gate";
/// The profile id of gate sessions: command sessions Fallow starts itself, never agents.
pub const GATE_PROFILE: &str = "fallow-gate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On the starting commit, before builders begin.
    Baseline,
    /// On the lead's worktree when every builder task is done.
    Final,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Baseline => "baseline",
            Kind::Final => "final",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    Passed,
    Failed(Option<i32>),
    TimedOut,
    /// A baseline that found no tests (a new project): nothing to compare with.
    NoTests,
}

impl RunState {
    pub fn label(&self) -> String {
        match self {
            RunState::Running => "running".into(),
            RunState::Passed => "passed".into(),
            RunState::Failed(Some(c)) => format!("failed (exit {c})"),
            RunState::Failed(None) => "failed".into(),
            RunState::TimedOut => "failed (timed out)".into(),
            RunState::NoTests => "no tests ran: nothing to compare".into(),
        }
    }

    pub fn passed(&self) -> bool {
        *self == RunState::Passed
    }
}

/// A final run compared with the baseline (research R-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Pass,
    /// It failed, but only where the baseline already failed.
    BaselineOnly,
    /// A check failed that passed in the baseline.
    Regression,
    /// It failed, and there are no names or no baseline to compare with.
    Fail,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Pass => "pass",
            Class::BaselineOnly => "failures already in the baseline only",
            Class::Regression => "regression",
            Class::Fail => "fail",
        }
    }

    /// Whether a merge condition `gate:<name>` holds with this result.
    pub fn opens(self) -> bool {
        matches!(self, Class::Pass | Class::BaselineOnly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRun {
    pub gate: String,
    pub kind: Kind,
    pub session: u32,
    /// What it ran against.
    pub commit: Option<String>,
    pub state: RunState,
    pub failing: BTreeSet<String>,
    /// Final runs, once finished.
    pub class: Option<Class>,
    pub started: Instant,
    pub deadline: Instant,
    /// Builder tasks done when the run started: a final run is due again after more are done.
    pub after_tasks: usize,
    /// A temporary worktree (baseline) to remove when the run ends.
    pub dir: Option<PathBuf>,
    /// The gate session could not be started: not a result, tried again later.
    pub start_failed: bool,
}

/// How long a gate that could not start waits before Fallow tries again.
pub const START_RETRY: Duration = Duration::from_secs(60);
/// A baseline that could not start this often is given up: the burst runs without one.
pub const BASELINE_TRIES: usize = 3;

/// Whether a gate's baseline is done: it ran (any result), or could not start
/// `BASELINE_TRIES` times (verification C: a failed start is not a result).
fn baseline_settled(runs: &[GateRun], gate: &str) -> bool {
    let mine = runs
        .iter()
        .filter(|r| r.gate == gate && r.kind == Kind::Baseline);
    let (mut ran, mut failed_starts) = (false, 0);
    for r in mine {
        if r.start_failed {
            failed_starts += 1;
        } else if r.state != RunState::Running {
            ran = true;
        }
    }
    ran || failed_starts >= BASELINE_TRIES
}

/// A gate run the app should start now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub project: ProjectId,
    pub gate: Gate,
    pub kind: Kind,
    /// Baseline: the burst's starting commit. Final: run in the lead's worktree.
    pub commit: Option<String>,
    pub lead: u32,
}

/// A task that builds the work: handed to a builder (a role that edits, not a gate), or one of
/// the lead's own pieces of work (005 R-16).
fn work_task(b: &super::burst::Burst, t: &tasks::Task) -> bool {
    t.assigned.and_then(|a| b.roles.get(&a)).is_some_and(|r| {
        r != GATE_ROLE
            && b.squad.role(r).is_some_and(|role| role.level.edits())
            && (r != "lead" || t.work.is_some())
    })
}

impl Hub {
    /// Work tasks in a burst: tasks handed to builders (roles that edit, not gates), and the
    /// lead's own pieces of work (005 R-16: the lead builds by default). How many in all, and
    /// how many are done.
    fn builder_tasks(&self, project: ProjectId) -> (usize, usize) {
        let Some(b) = self.burst_of(project) else {
            return (0, 0);
        };
        let handed: Vec<&tasks::Task> = self
            .tasks
            .list
            .iter()
            .filter(|t| t.created_at >= b.started && work_task(b, t))
            .filter(|t| !matches!(t.state, tasks::State::Cancelled(_)))
            .collect();
        let done = handed
            .iter()
            .filter(|t| t.state == tasks::State::Done)
            .count();
        (handed.len(), done)
    }

    /// Whether builders' tasks wait for a baseline run (FR-021).
    pub fn baseline_pending(&self, project: ProjectId) -> bool {
        let Some(b) = self.burst_of(project) else {
            return false;
        };
        b.start_commit.is_some()
            && b.squad
                .gates
                .iter()
                .any(|g| g.baseline && !baseline_settled(&b.gate_runs, &g.name))
    }

    /// Whether a task for `session` waits for the baseline: every role but the lead's.
    pub(crate) fn held_for_baseline(&self, session: u32) -> bool {
        self.burst_for(session).is_some_and(|b| {
            b.roles.get(&session).is_some_and(|r| r != "lead") && self.baseline_pending(b.project)
        })
    }

    /// The commits builders reported done in a burst (their evidence), oldest first.
    fn builder_commits(&self, project: ProjectId) -> Vec<String> {
        let Some(b) = self.burst_of(project) else {
            return Vec::new();
        };
        self.tasks
            .list
            .iter()
            .filter(|t| t.created_at >= b.started && t.state == tasks::State::Done)
            .filter(|t| work_task(b, t))
            .filter_map(|t| {
                t.assigned
                    .zip(t.evidence.as_ref().map(|e| e.commit.clone()))
            })
            // Only each builder's latest reported commit must be integrated: an amended or
            // superseded earlier report must not hold the final gate forever (review).
            .fold(Vec::<(u32, String)>::new(), |mut acc, (s, c)| {
                match acc.iter_mut().find(|(x, _)| *x == s) {
                    Some(e) => e.1 = c,
                    None => acc.push((s, c)),
                }
                acc
            })
            .into_iter()
            .map(|(_, c)| c)
            .collect()
    }

    /// Gate runs to start now (research R-6): baselines at once; a final run when every task
    /// handed to a builder is done and the lead's commit contains all of their reported
    /// commits (`integrated` answers with that commit, asked only then), once per commit.
    pub fn gates_due(
        &self,
        integrated: &dyn Fn(ProjectId, &[String]) -> Option<String>,
    ) -> Vec<Job> {
        let mut jobs = Vec::new();
        for b in &self.bursts {
            let (handed, done) = self.builder_tasks(b.project);
            for g in &b.squad.gates {
                let runs: Vec<&GateRun> = b.gate_runs.iter().filter(|r| r.gate == g.name).collect();
                if runs.iter().any(|r| r.state == RunState::Running) {
                    continue;
                }
                // A gate that could not start is tried again after START_RETRY, not at once.
                if runs
                    .last()
                    .is_some_and(|r| r.start_failed && r.started.elapsed() < START_RETRY)
                {
                    continue;
                }
                if g.baseline
                    && b.start_commit.is_some()
                    && !baseline_settled(&b.gate_runs, &g.name)
                    && !runs
                        .iter()
                        .any(|r| r.kind == Kind::Baseline && r.state == RunState::Running)
                {
                    jobs.push(Job {
                        project: b.project,
                        gate: g.clone(),
                        kind: Kind::Baseline,
                        commit: b.start_commit.clone(),
                        lead: b.lead,
                    });
                    continue;
                }
                if handed == 0 || done < handed {
                    continue;
                }
                // Only the lead's integrated work is gated: a commit without a builder's
                // reported work would fail for the wrong reason (004 manual run).
                let Some(head) = integrated(b.project, &self.builder_commits(b.project)) else {
                    continue;
                };
                let last = runs.iter().rev().find(|r| r.kind == Kind::Final);
                if last.is_none_or(|l| l.commit.as_deref() != Some(head.as_str())) {
                    jobs.push(Job {
                        project: b.project,
                        gate: g.clone(),
                        kind: Kind::Final,
                        commit: Some(head),
                        lead: b.lead,
                    });
                }
            }
        }
        jobs
    }

    /// The app started a gate session (or could not: `session` is `None`).
    #[allow(clippy::too_many_arguments)]
    pub fn gate_started(
        &mut self,
        job: &Job,
        session: Result<u32, String>,
        commit: Option<String>,
        dir: Option<PathBuf>,
        now: Instant,
    ) {
        let (_, done) = self.builder_tasks(job.project);
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == job.project) else {
            return;
        };
        let timeout = Duration::from_secs(u64::from(job.gate.timeout_min) * 60);
        let mut run = GateRun {
            gate: job.gate.name.clone(),
            kind: job.kind,
            session: 0,
            commit,
            state: RunState::Running,
            failing: BTreeSet::new(),
            class: None,
            started: now,
            deadline: now + timeout,
            after_tasks: done,
            dir,
            start_failed: false,
        };
        match session {
            Ok(id) => {
                run.session = id;
                b.roles.insert(id, GATE_ROLE.into());
                b.gate_runs.push(run);
                // The lead's new commit is its answer to a failure of the whole work: the next
                // final failure counts a new round (verification B).
                if job.kind == Kind::Final {
                    if let Some(l) = b.repairs.iter_mut().find(|l| l.part.is_none()) {
                        l.pending = false;
                    }
                }
            }
            Err(why) => {
                // Not a result of the work: no commit (so it is due again), no repair round,
                // and retried after START_RETRY (review).
                run.commit = None;
                run.state = RunState::Failed(None);
                run.start_failed = true;
                if job.kind == Kind::Final {
                    run.class = Some(Class::Fail);
                }
                b.gate_runs.push(run);
                self.tell_lead(
                    job.project,
                    format!(
                        "Gate {} ({}) could not start: {why}. Fallow tries again in a minute.",
                        job.gate.name,
                        job.kind.label()
                    ),
                );
            }
        }
    }

    /// A session ended: if it ran a gate, its exit status decides the gate (FR-020). Returns
    /// the finished run (its temporary worktree is the app's to remove).
    pub fn gate_finished(
        &mut self,
        session: u32,
        exit: Option<i32>,
        output: &str,
    ) -> Option<GateRun> {
        let (project, i, failures) = self.bursts.iter().find_map(|b| {
            let i = b
                .gate_runs
                .iter()
                .position(|r| r.session == session && r.state == RunState::Running)?;
            let how = b
                .squad
                .gates
                .iter()
                .find(|g| g.name == b.gate_runs[i].gate)
                .map_or(Failures::ExitCode, |g| g.failures.clone());
            Some((b.project, i, how))
        })?;
        let baseline = self
            .bursts
            .iter()
            .find(|b| b.project == project)
            .is_some_and(|b| b.gate_runs[i].kind == Kind::Baseline);
        let state = if exit == Some(0) {
            RunState::Passed
        } else if baseline && no_tests(output) {
            RunState::NoTests
        } else {
            RunState::Failed(exit)
        };
        let failing = if state.passed() {
            BTreeSet::new()
        } else {
            failing_names(output, &failures)
        };
        self.finish_run(project, i, state, failing);
        self.burst_of(project).map(|b| b.gate_runs[i].clone())
    }

    /// Ends run `i` of a project's burst: classification, Fallow's own signal and a board note.
    fn finish_run(
        &mut self,
        project: ProjectId,
        i: usize,
        state: RunState,
        failing: BTreeSet<String>,
    ) {
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        let baseline = {
            let gate = b.gate_runs[i].gate.clone();
            b.gate_runs
                .iter()
                .rev()
                .find(|r| {
                    r.gate == gate
                        && r.kind == Kind::Baseline
                        && r.state != RunState::Running
                        && !r.start_failed
                })
                .cloned()
        };
        let run = &mut b.gate_runs[i];
        run.state = state;
        run.failing = failing;
        if run.kind == Kind::Final {
            run.class = Some(classify(run, baseline.as_ref()));
        }
        let run = run.clone();
        let mut text = format!(
            "Gate {} ({}{}) {}",
            run.gate,
            run.kind.label(),
            run.commit
                .as_ref()
                .map_or(String::new(), |c| format!(", {}", &c[..c.len().min(10)])),
            run.state.label()
        );
        if let Some(c) = run.class {
            text.push_str(&format!(": {}", c.label()));
        }
        if !run.failing.is_empty() {
            let names: Vec<&str> = run.failing.iter().take(10).map(String::as_str).collect();
            text.push_str(&format!(". Failing: {}", names.join(", ")));
        }
        if run.kind == Kind::Final {
            let ok = run.class.is_some_and(Class::opens);
            // A failed gate is not tied to one part: it goes back to the lead (FR-025).
            if !ok {
                self.pending_repairs.push((project, None, text.clone()));
            }
            let board = BoardId::Project(project);
            let (yes, no) = (
                format!("gate-{}-passed", run.gate),
                format!("gate-{}-failed", run.gate),
            );
            let (on, off) = if ok { (yes, no) } else { (no, yes) };
            self.clear_event(&(board, off));
            let _ = self.signal(&(board, on), Party::Fallow, Some(text.clone()));
        }
        self.boards.post(
            BoardId::Project(project),
            Party::Fallow,
            text.clone(),
            std::time::SystemTime::now(),
            TEXT_BUDGET,
        );
        self.activity
            .add(Party::Fallow, Action::Signal, &text, None);
        self.tell_lead(project, text);
    }

    /// Gate runs past their time limit fail "timed out"; their sessions are for the app to end
    /// (FR-022).
    pub(crate) fn tick_gates(&mut self, now: Instant) -> bool {
        let due: Vec<(ProjectId, usize, u32)> = self
            .bursts
            .iter()
            .flat_map(|b| {
                b.gate_runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.state == RunState::Running && now >= r.deadline)
                    .map(|(i, r)| (b.project, i, r.session))
                    .collect::<Vec<_>>()
            })
            .collect();
        let changed = !due.is_empty();
        for (p, i, session) in due {
            self.finish_run(p, i, RunState::TimedOut, BTreeSet::new());
            self.end_gates.push(session);
        }
        changed
    }
}

fn auto_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // cargo test
            r"^test (\S+) \.\.\. FAILED",
            // pytest short summary
            r"^FAILED (\S+)",
            // Go
            r"^\s*--- FAIL: (\S+)",
            // .NET
            r"^\s*Failed (\S+)",
            // Jest / Vitest: a failing test, then the file summary
            r"^\s*[✕×] (.+?)(?: \(\d+ ?m?s\))?$",
            r"^\s*FAIL (\S+)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid pattern"))
        .collect()
    })
}

/// The names of failing checks in a gate's output (research R-5). `ExitCode`: none.
pub fn failing_names(output: &str, how: &Failures) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let one = |re: &Regex, out: &mut BTreeSet<String>| {
        for line in output.lines() {
            if let Some(c) = re.captures(line.trim_end()).and_then(|c| c.get(1)) {
                out.insert(c.as_str().trim().to_string());
            }
        }
    };
    match how {
        Failures::ExitCode => {}
        Failures::Auto => {
            for re in auto_patterns() {
                one(re, &mut out);
            }
        }
        Failures::Regex(p) => {
            if let Ok(re) = Regex::new(p) {
                one(&re, &mut out);
            }
        }
    }
    out
}

/// Whether a run's output says no tests ran (unittest, pytest, cargo, Jest, Go, .NET).
pub fn no_tests(output: &str) -> bool {
    // cargo prints "running 0 tests" for every target without tests even when others ran and
    // failed (review): a run with any test is not "no tests".
    static RAN: OnceLock<Regex> = OnceLock::new();
    let ran = RAN.get_or_init(|| {
        Regex::new(r"(?m)running [1-9][0-9]* tests?|Ran [1-9][0-9]* tests?|collected [1-9][0-9]* items?|^(FAIL|ok)\s")
            .expect("valid pattern")
    });
    if ran.is_match(output) || !failing_names(output, &Failures::Auto).is_empty() {
        return false;
    }
    const SAYS: &[&str] = &[
        "Ran 0 tests",
        "NO TESTS RAN",
        "no tests ran",
        "collected 0 items",
        "running 0 tests",
        "No tests found",
        "no test files",
        "No test is available",
    ];
    SAYS.iter().any(|s| output.contains(s))
}

/// Classifies a finished final run against the baseline's run (research R-5).
pub fn classify(final_run: &GateRun, baseline: Option<&GateRun>) -> Class {
    if final_run.state.passed() {
        return Class::Pass;
    }
    let Some(b) = baseline
        .filter(|b| !b.start_failed && !matches!(b.state, RunState::Running | RunState::NoTests))
    else {
        return Class::Fail;
    };
    if final_run.failing.is_empty() || b.state.passed() || b.failing.is_empty() {
        return if b.state.passed() && !final_run.failing.is_empty() {
            Class::Regression
        } else {
            Class::Fail
        };
    }
    if final_run.failing.is_subset(&b.failing) {
        Class::BaselineOnly
    } else {
        Class::Regression
    }
}

/// The command line a gate session runs: the shell with the gate's command for this OS
/// (research R-6).
pub fn shell_command(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let ps = if which("pwsh") { "pwsh" } else { "powershell" };
        (
            ps.into(),
            vec!["-NoProfile".into(), "-Command".into(), command.into()],
        )
    } else {
        let sh = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        (sh, vec!["-c".into(), command.into()])
    }
}

fn which(program: &str) -> bool {
    crate::session::profiles::resolve(program).is_ok()
}
