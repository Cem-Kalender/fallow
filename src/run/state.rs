//! A run's state: the fold of its events (data-model.md). Nothing here runs git or agents; the
//! stage machine (`machine`) reads this state to decide what happens next.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use super::events::*;
use crate::config::{Autonomy, RoleChoice};
use crate::verify::gates::{Class, GateResult};

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub n: u32,
    pub kind: StepKind,
    pub part: Option<String>,
    pub reason: String,
    pub resume: Option<String>,
    pub started: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub session: Option<String>,
    pub worktree: Option<PathBuf>,
    pub base: Option<String>,
    pub end: Option<StepEnd>,
    pub finished: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub denials: Vec<String>,
    pub verified: Option<(bool, Vec<String>)>,
    pub verified_commit: Option<String>,
    /// The event number that queued it, to tell rounds apart.
    pub queued_seq: u64,
    /// 007: a loop step's task (`T014`), phase and agent definition (`builder`).
    pub task: Option<String>,
    pub phase: Option<u32>,
    pub def: Option<String>,
}

impl Step {
    pub fn running(&self) -> bool {
        self.started.is_some() && self.end.is_none()
    }

    pub fn queued(&self) -> bool {
        self.started.is_none()
    }

    /// The structured result, when the step produced one.
    pub fn result(&self) -> Option<&Value> {
        match &self.end {
            Some(StepEnd::Result { value }) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PartStatus {
    pub id: String,
    pub state: PartState,
    pub result_commit: Option<String>,
    pub repairs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateEntry {
    pub kind: GateKind,
    pub part: Option<String>,
    pub result: GateResult,
    pub class: Option<Class>,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerdictEntry {
    pub step: u32,
    pub answers: Vec<Answer>,
    pub complete: bool,
    pub approved: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SteeringEntry {
    pub kind: SteeringKind,
    pub target: Option<String>,
    pub detail: Option<String>,
    pub at: String,
    pub stage: Stage,
    /// The event number, to order it among steps.
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Need {
    pub why: String,
    pub options: Vec<String>,
}

/// One line of the planning conversation.
#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub who: &'static str,
    pub text: String,
    pub at: String,
    /// The planning turn that replied (none for the engineer's lines).
    pub step: Option<u32>,
}

/// A phase of a loop, as locked, and where it is (007 data-model.md, Phase).
#[derive(Debug, Clone, Serialize)]
pub struct PhaseStatus {
    pub spec: PhaseSpec,
    pub state: PhaseState,
    /// The phase's builder session (FR-010a).
    pub session: Option<String>,
    pub handover: Option<String>,
    /// Reopened by a new lock (the event's number): its story is repaired with the change and
    /// checked again (research S-17).
    pub reopened: Option<u64>,
}

/// A task of a loop and where it is (007 data-model.md, Task).
#[derive(Debug, Clone, Serialize)]
pub struct TaskStatus {
    pub phase: u32,
    pub state: TaskState,
    pub commit: Option<String>,
    pub repairs: u32,
}

/// A question an agent asked during the loop (007 data-model.md, Question).
#[derive(Debug, Clone, Serialize)]
pub struct QuestionEntry {
    pub id: String,
    pub step: u32,
    pub task: Option<String>,
    pub text: String,
    pub options: Vec<String>,
    pub state: QuestionState,
    pub by: Option<String>,
    pub answer: Option<String>,
    pub commit: Option<String>,
    pub affects: Option<String>,
    pub seq: u64,
}

/// 007: the loop over a locked feature's `tasks.md`.
#[derive(Debug, Clone, Serialize)]
pub struct LoopState {
    pub project: String,
    pub feature: String,
    pub lock: u32,
    pub lock_commit: String,
    pub base_branch: String,
    pub build_branch: String,
    pub pipeline: String,
    pub level: Level,
    pub budget_usd: f64,
    pub budget_minutes: u64,
    pub phases: Vec<PhaseStatus>,
    pub tasks: BTreeMap<String, TaskStatus>,
    pub questions: BTreeMap<String, QuestionEntry>,
    /// Unlocked and not locked again yet (FR-009).
    pub unlocked: bool,
    /// The stories reopened by the latest new lock.
    pub affected: Vec<String>,
    /// The lock commit before the latest new lock, to show the builder what changed.
    pub prev_lock_commit: Option<String>,
    /// How the loop ended: verified, and what was not checked.
    pub outcome: Option<(bool, Vec<String>)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunState {
    pub id: u64,
    pub repo: PathBuf,
    pub target: String,
    pub start_commit: String,
    pub task: String,
    pub mode: Mode,
    pub autonomy: Autonomy,
    pub limits: Limits,
    pub roles: BTreeMap<String, RoleChoice>,
    pub repair_cap: u32,
    pub gates: Vec<GateSpec>,
    pub allow: BTreeMap<String, Vec<String>>,
    pub test_patterns: Vec<String>,
    pub recorded: bool,
    pub left_out: Vec<String>,
    pub created: String,
    pub stage: Stage,
    /// The stage a pause came from.
    pub paused_from: Option<Stage>,
    pub conversation: Vec<Turn>,
    pub plan_commit: Option<String>,
    pub plan_approvable: bool,
    pub plan_problems: Vec<String>,
    pub plan_ready: bool,
    pub approved_plan: Option<String>,
    pub building_started: Option<String>,
    pub steps: BTreeMap<u32, Step>,
    pub parts: BTreeMap<String, PartStatus>,
    pub gate_runs: Vec<GateEntry>,
    pub integrated: Option<String>,
    pub integrated_seq: u64,
    pub verdicts: Vec<VerdictEntry>,
    pub steering: Vec<SteeringEntry>,
    pub needs: Option<Need>,
    pub limit_reached: Option<String>,
    pub merged: Option<String>,
    pub stopped: Option<Option<String>>,
    pub notes: Vec<String>,
    pub cost_usd: f64,
    /// Sessions that could not be resumed.
    pub lost_sessions: Vec<String>,
    /// One step may start past the limits (FR-038 "continue once").
    pub allow_once: bool,
    /// When the current pause began, and the paused seconds before it: paused time does not
    /// count against the time limit.
    pub paused_since: Option<String>,
    pub paused_secs: u64,
    /// When the engineer was last asked something while the run was not paused: time waiting
    /// for the engineer does not count either (FR-016a).
    pub waiting_since: Option<String>,
    /// The latest event number.
    pub seq: u64,
    /// 007: set when this run is a loop over a locked feature.
    pub feature_loop: Option<LoopState>,
}

impl RunState {
    /// Folds a run's events. The first must be `run_created`; an event the stage machine does
    /// not allow is ignored and noted, never applied.
    pub fn fold(id: u64, events: &[Logged]) -> Result<RunState, String> {
        let Some(first) = events.first() else {
            return Err(format!("run {id}: no events"));
        };
        let Event::RunCreated {
            repo,
            target,
            start_commit,
            task,
            mode,
            autonomy,
            limits,
            roles,
            repair_cap,
            gates,
            allow,
            test_patterns,
            recorded,
            left_out,
        } = &first.event
        else {
            return Err(format!("run {id}: the first event is not run_created"));
        };
        let mut s = RunState {
            id,
            repo: repo.clone(),
            target: target.clone(),
            start_commit: start_commit.clone(),
            task: task.clone(),
            mode: *mode,
            autonomy: *autonomy,
            limits: *limits,
            roles: roles.clone(),
            repair_cap: *repair_cap,
            gates: gates.clone(),
            allow: allow.clone(),
            test_patterns: test_patterns.clone(),
            recorded: *recorded,
            left_out: left_out.clone(),
            created: first.at.clone(),
            stage: Stage::Planning,
            paused_from: None,
            conversation: Vec::new(),
            plan_commit: None,
            plan_approvable: false,
            plan_problems: Vec::new(),
            plan_ready: false,
            approved_plan: None,
            building_started: None,
            steps: BTreeMap::new(),
            parts: BTreeMap::new(),
            gate_runs: Vec::new(),
            integrated: None,
            integrated_seq: 0,
            verdicts: Vec::new(),
            steering: Vec::new(),
            needs: None,
            limit_reached: None,
            merged: None,
            stopped: None,
            notes: Vec::new(),
            cost_usd: 0.0,
            lost_sessions: Vec::new(),
            allow_once: false,
            paused_since: None,
            paused_secs: 0,
            waiting_since: None,
            seq: first.seq,
            feature_loop: None,
        };
        for l in &events[1..] {
            s.apply(l);
        }
        Ok(s)
    }

    /// Applies one event.
    pub fn apply(&mut self, l: &Logged) {
        self.seq = l.seq;
        self.apply_event(l);
        self.wait_clock(&l.at);
    }

    /// Starts or stops the clock of the time spent waiting for the engineer, which a pause
    /// already covers.
    fn wait_clock(&mut self, at: &str) {
        let waiting = self.needs.is_some() && self.paused_since.is_none();
        match (&self.waiting_since, waiting) {
            (None, true) => self.waiting_since = Some(at.to_string()),
            (Some(since), false) => {
                if let (Some(a), Some(b)) = (
                    super::time::parse_rfc3339(since),
                    super::time::parse_rfc3339(at),
                ) {
                    self.paused_secs += b.saturating_sub(a);
                }
                self.waiting_since = None;
            }
            _ => {}
        }
    }

    fn apply_event(&mut self, l: &Logged) {
        match &l.event {
            Event::RunCreated { .. } => self
                .notes
                .push(format!("event {}: a second run_created was ignored", l.seq)),
            Event::PlanSaid { text } => self.conversation.push(Turn {
                who: "engineer",
                text: text.clone(),
                at: l.at.clone(),
                step: None,
            }),
            Event::PlanReply { step, reply, ready } => {
                self.conversation.push(Turn {
                    who: "planner",
                    text: reply.clone(),
                    at: l.at.clone(),
                    step: Some(*step),
                });
                self.plan_ready = *ready;
            }
            Event::PlanCommitted {
                commit,
                approvable,
                problems,
            } => {
                self.plan_commit = Some(commit.clone());
                self.plan_approvable = *approvable;
                self.plan_problems = problems.clone();
            }
            Event::PlanApproved { commit } => {
                self.approved_plan = Some(commit.clone());
            }
            Event::Stage { to, .. } => {
                if !self.stage.may_go_to(*to) {
                    self.notes.push(format!(
                        "event {}: {} → {} is not allowed and was ignored",
                        l.seq,
                        self.stage.label(),
                        to.label()
                    ));
                    return;
                }
                if *to == Stage::Paused {
                    self.paused_from = Some(self.stage);
                    self.paused_since = Some(l.at.clone());
                } else if self.stage == Stage::Paused {
                    self.paused_from = None;
                    self.limit_reached = None;
                    let since = self
                        .paused_since
                        .take()
                        .as_deref()
                        .and_then(super::time::parse_rfc3339);
                    let now = super::time::parse_rfc3339(&l.at);
                    if let (Some(a), Some(b)) = (since, now) {
                        self.paused_secs += b.saturating_sub(a);
                    }
                }
                if *to == Stage::Building && self.building_started.is_none() {
                    self.building_started = Some(l.at.clone());
                }
                self.stage = *to;
                self.needs = None;
            }
            Event::StepQueued {
                n,
                kind,
                part,
                resume,
                reason,
                task,
                phase,
                def,
            } => {
                self.steps.insert(
                    *n,
                    Step {
                        n: *n,
                        kind: *kind,
                        part: part.clone(),
                        reason: reason.clone(),
                        resume: resume.clone(),
                        started: None,
                        agent: None,
                        model: None,
                        session: None,
                        worktree: None,
                        base: None,
                        end: None,
                        finished: None,
                        cost_usd: None,
                        duration_ms: None,
                        denials: Vec::new(),
                        verified: None,
                        verified_commit: None,
                        queued_seq: l.seq,
                        task: task.clone(),
                        phase: *phase,
                        def: def.clone(),
                    },
                );
            }
            Event::StepStarted {
                n,
                agent,
                model,
                session,
                worktree,
                base,
            } => {
                self.allow_once = false;
                if let Some(st) = self.steps.get_mut(n) {
                    st.started = Some(l.at.clone());
                    st.agent = Some(agent.clone());
                    st.model = Some(model.clone());
                    st.session = Some(session.clone());
                    st.worktree = Some(worktree.clone());
                    st.base = Some(base.clone());
                }
            }
            Event::StepFinished {
                n,
                end,
                cost_usd,
                duration_ms,
                denials,
            } => {
                if let Some(st) = self.steps.get_mut(n) {
                    st.end = Some(end.clone());
                    st.finished = Some(l.at.clone());
                    st.cost_usd = *cost_usd;
                    st.duration_ms = Some(*duration_ms);
                    st.denials = denials.clone();
                }
                self.cost_usd += cost_usd.unwrap_or(0.0);
            }
            Event::Verified {
                n,
                accepted,
                reasons,
                commit,
            } => {
                if let Some(st) = self.steps.get_mut(n) {
                    st.verified = Some((*accepted, reasons.clone()));
                    st.verified_commit = commit.clone();
                }
            }
            Event::GateRun {
                kind,
                part,
                result,
                class,
            } => self.gate_runs.push(GateEntry {
                kind: *kind,
                part: part.clone(),
                result: result.clone(),
                class: *class,
                at: l.at.clone(),
            }),
            Event::Part {
                id,
                state,
                result_commit,
                repairs,
            } => {
                self.parts.insert(
                    id.clone(),
                    PartStatus {
                        id: id.clone(),
                        state: *state,
                        result_commit: result_commit.clone(),
                        repairs: *repairs,
                    },
                );
            }
            Event::Integrated { commit } => {
                self.integrated = Some(commit.clone());
                self.integrated_seq = l.seq;
            }
            Event::Verdict {
                step,
                answers,
                complete,
                approved,
                missing,
            } => self.verdicts.push(VerdictEntry {
                step: *step,
                answers: answers.clone(),
                complete: *complete,
                approved: *approved,
                missing: missing.clone(),
            }),
            Event::Steering {
                kind,
                target,
                detail,
            } => {
                // An action of the engineer answers what was waiting for them.
                self.needs = None;
                if *kind == SteeringKind::SetAutonomy {
                    if let Some(a) = detail.as_deref().and_then(Autonomy::parse) {
                        self.autonomy = a;
                    }
                }
                self.steering.push(SteeringEntry {
                    kind: *kind,
                    target: target.clone(),
                    detail: detail.clone(),
                    at: l.at.clone(),
                    stage: self.stage,
                    seq: l.seq,
                });
            }
            Event::NeedsEngineer { why, options } => {
                self.needs = Some(Need {
                    why: why.clone(),
                    options: options.clone(),
                })
            }
            Event::LimitReached { which, used } => {
                self.limit_reached = Some(format!("{which}: {used}"));
            }
            Event::LimitsSet {
                usd,
                minutes,
                repair_cap,
                once,
            } => {
                if *once {
                    self.allow_once = true;
                }
                if let Some(u) = usd {
                    self.limits.usd = *u;
                }
                if let Some(m) = minutes {
                    self.limits.minutes = *m;
                }
                if let Some(r) = repair_cap {
                    self.repair_cap = *r;
                }
            }
            Event::SessionLost { session } => self.lost_sessions.push(session.clone()),
            Event::NeedsCleared { .. } => self.needs = None,
            Event::Merged { commit, .. } => self.merged = Some(commit.clone()),
            Event::Stopped { reason } => self.stopped = Some(reason.clone()),
            Event::Note { text } => self.notes.push(text.clone()),
            Event::LoopCreated {
                project,
                feature,
                lock,
                lock_commit,
                base_branch,
                build_branch,
                pipeline,
                level,
                budget_usd,
                budget_minutes,
                phases,
            } => {
                let mut tasks = BTreeMap::new();
                for p in phases {
                    for t in &p.tasks {
                        tasks.insert(
                            t.id.clone(),
                            TaskStatus {
                                phase: p.n,
                                state: TaskState::Pending,
                                commit: None,
                                repairs: 0,
                            },
                        );
                    }
                }
                self.feature_loop = Some(LoopState {
                    project: project.clone(),
                    feature: feature.clone(),
                    lock: *lock,
                    lock_commit: lock_commit.clone(),
                    base_branch: base_branch.clone(),
                    build_branch: build_branch.clone(),
                    pipeline: pipeline.clone(),
                    level: *level,
                    budget_usd: *budget_usd,
                    budget_minutes: *budget_minutes,
                    phases: phases
                        .iter()
                        .map(|p| PhaseStatus {
                            spec: p.clone(),
                            state: PhaseState::Pending,
                            session: None,
                            handover: None,
                            reopened: None,
                        })
                        .collect(),
                    tasks,
                    questions: BTreeMap::new(),
                    unlocked: false,
                    affected: Vec::new(),
                    prev_lock_commit: None,
                    outcome: None,
                });
            }
            Event::Task {
                id,
                state,
                commit,
                repairs,
            } => match self.feature_loop.as_mut().and_then(|f| f.tasks.get_mut(id)) {
                Some(t) => {
                    t.state = *state;
                    if commit.is_some() {
                        t.commit = commit.clone();
                    }
                    t.repairs = *repairs;
                }
                None => self.notes.push(format!("event {}: no task {id}", l.seq)),
            },
            Event::Phase {
                n,
                state,
                session,
                handover,
            } => match self
                .feature_loop
                .as_mut()
                .and_then(|f| f.phases.iter_mut().find(|p| p.spec.n == *n))
            {
                Some(p) => {
                    p.state = *state;
                    if session.is_some() {
                        p.session = session.clone();
                    }
                    if handover.is_some() {
                        p.handover = handover.clone();
                    }
                }
                None => self.notes.push(format!("event {}: no phase {n}", l.seq)),
            },
            Event::Question {
                id,
                step,
                task,
                text,
                options,
                state,
                by,
                answer,
                commit,
                affects,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.questions.insert(
                        id.clone(),
                        QuestionEntry {
                            id: id.clone(),
                            step: *step,
                            task: task.clone(),
                            text: text.clone(),
                            options: options.clone(),
                            state: *state,
                            by: by.clone(),
                            answer: answer.clone(),
                            commit: commit.clone(),
                            affects: affects.clone(),
                            seq: l.seq,
                        },
                    );
                }
            }
            Event::Unlocked => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.unlocked = true;
                }
            }
            Event::Relocked {
                lock,
                commit,
                affected,
                phases,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.unlocked = false;
                    f.lock = *lock;
                    f.prev_lock_commit = Some(std::mem::replace(&mut f.lock_commit, commit.clone()));
                    f.affected = affected.clone();
                    f.outcome = None;
                    if !phases.is_empty() {
                        // The new lock's tasks: a phase keeps where it was when it is the same
                        // phase (place and title); new tasks start pending.
                        let old = std::mem::take(&mut f.phases);
                        for p in phases {
                            let kept = old
                                .iter()
                                .find(|o| o.spec.n == p.n && o.spec.title == p.title);
                            f.phases.push(PhaseStatus {
                                spec: p.clone(),
                                state: kept.map_or(PhaseState::Pending, |k| k.state),
                                session: kept.and_then(|k| k.session.clone()),
                                handover: kept.and_then(|k| k.handover.clone()),
                                reopened: None,
                            });
                            for t in &p.tasks {
                                f.tasks.entry(t.id.clone()).or_insert(TaskStatus {
                                    phase: p.n,
                                    state: TaskState::Pending,
                                    commit: None,
                                    repairs: 0,
                                });
                            }
                        }
                    }
                    // The affected stories' phases reopen, in a fresh session (research S-17);
                    // a done phase with a new task goes on with it.
                    let tasks = &f.tasks;
                    for p in &mut f.phases {
                        let story_hit = p
                            .spec
                            .story
                            .iter()
                            .chain(p.spec.tasks.iter().filter_map(|t| t.story.as_ref()))
                            .any(|s| affected.contains(s));
                        let new_task = p.spec.tasks.iter().any(|t| {
                            tasks
                                .get(&t.id)
                                .is_none_or(|s| s.state != TaskState::Accepted)
                        });
                        if story_hit {
                            p.state = PhaseState::Pending;
                            p.session = None;
                            p.reopened = Some(l.seq);
                        } else if new_task && p.state == PhaseState::Done {
                            p.state = PhaseState::Pending;
                        }
                    }
                }
            }
            Event::Outcome { verified, missing } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.outcome = Some((*verified, missing.clone()));
                }
            }
        }
    }

    /// The next step number.
    pub fn next_step(&self) -> u32 {
        self.steps.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The session of the latest step of `role` (and `part`) that started and can still be
    /// resumed.
    pub fn session_of(&self, role: &str, part: Option<&str>) -> Option<String> {
        self.steps
            .values()
            .rev()
            .filter(|s| s.kind.role() == role && s.part.as_deref() == part)
            .find_map(|s| s.session.clone())
            .filter(|s| !self.lost_sessions.contains(s))
    }

    /// The baseline result of a gate, if it ran.
    pub fn baseline(&self, gate: &str) -> Option<&GateResult> {
        self.gate_runs
            .iter()
            .rev()
            .find(|g| g.kind == GateKind::Baseline && g.result.gate == gate)
            .map(|g| &g.result)
    }

    /// Interventions so far (FR-020).
    pub fn interventions(&self) -> usize {
        self.steering
            .iter()
            .filter(|s| s.kind.intervention())
            .count()
    }

    /// Minutes since building started, without the time the run was paused or waited for the
    /// engineer.
    pub fn minutes_used(&self, now: u64) -> u64 {
        let Some(start) = self
            .building_started
            .as_deref()
            .and_then(super::time::parse_rfc3339)
        else {
            return 0;
        };
        let paused_now = self
            .paused_since
            .as_deref()
            .and_then(super::time::parse_rfc3339)
            .map_or(0, |p| now.saturating_sub(p));
        let waiting_now = self
            .waiting_since
            .as_deref()
            .and_then(super::time::parse_rfc3339)
            .map_or(0, |p| now.saturating_sub(p));
        now.saturating_sub(start)
            .saturating_sub(self.paused_secs + paused_now + waiting_now)
            / 60
    }

    /// Which limit stops the next agent step, if one does (FR-038): `(which, how much is used)`.
    pub fn limit_hit(&self, now: u64) -> Option<(String, String)> {
        if self.allow_once {
            return None;
        }
        if self.cost_usd >= self.limits.usd {
            return Some((
                "cost".into(),
                format!("${:.2} of ${:.2}", self.cost_usd, self.limits.usd),
            ));
        }
        let used = self.minutes_used(now);
        if self.building_started.is_some() && used >= self.limits.minutes {
            return Some((
                "time".into(),
                format!("{used} of {} min", self.limits.minutes),
            ));
        }
        None
    }
}
