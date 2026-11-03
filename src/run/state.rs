//! A run's state: the fold of its events (data-model.md). Nothing here runs git or agents; the
//! stage machine (`machine`) reads this state to decide what happens next.

use std::collections::{BTreeMap, BTreeSet};
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
    /// The event number of its start (0 until it starts).
    pub started_seq: u64,
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
        self.started.is_none() && self.end.is_none()
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
    /// When it started needing the engineer.
    pub at: String,
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
    /// Steps queued before this event number are not this task's: a new lock changed it.
    pub since: u64,
}

/// Whether `by` names a lead (009): any agent other than the engineer. Records from before 009
/// say `lead`.
pub fn by_a_lead(by: Option<&str>) -> bool {
    by.is_some_and(|b| b != "engineer")
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
    pub recommended: Option<String>,
    pub reopened: Vec<String>,
    /// Why it was relayed to the engineer.
    pub why: Option<String>,
    /// The event number of its latest change, and of when it was asked.
    pub seq: u64,
    pub asked_seq: u64,
    /// Every change, oldest first, for the feed (FR-023).
    pub history: Vec<QuestionChange>,
}

impl QuestionEntry {
    /// Answered by a lead, the default one or another (009).
    pub fn by_lead(&self) -> bool {
        by_a_lead(self.by.as_deref())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionChange {
    pub at: String,
    pub seq: u64,
    pub state: QuestionState,
    pub by: Option<String>,
    pub answer: Option<String>,
    pub commit: Option<String>,
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
    /// The event number of the latest new lock: tries and repairs count from there.
    pub relocked_seq: u64,
    /// How the loop ended: verified, and what was not checked.
    pub outcome: Option<(bool, Vec<String>)>,
    /// 010: what happens when the loop ends verified.
    pub when_verified: WhenVerified,
    /// 010: the loop's pull request, with its latest state.
    pub publication: Option<Publication>,
    /// 010: the last failed publication, until the next try (step, reason, what was pushed).
    pub publish_failed: Option<PublishFailure>,
    /// 010: a publication was started and has not ended (a restart reconciles it, R-8).
    pub publish_pending: bool,
    /// 010: a publication was ever tried (the loop publishes itself at most once).
    pub publish_tried: bool,
    /// 010: each publishing step, for the activity (FR-018): when, what, and whether it went
    /// through.
    pub publish_steps: Vec<(String, String, bool)>,
    /// 010: why the loop did not publish itself when verified (it waits for the engineer).
    pub publish_waited: Option<String>,
    /// 011: tries and repairs count from this event: the latest new lock or update (research R-3).
    pub fresh_seq: u64,
    /// 011: the update in progress, until the loop is ready (or pushed forward) again.
    pub update: Option<UpdateState>,
    /// 011: how many updates were started.
    pub updates: u32,
    /// 011: how many updates in a row may be overtaken before the engineer decides.
    pub update_limit: u32,
    /// 011: updates in a row overtaken (reset by a merge, a publication, a forward push, or the
    /// engineer's "Verify again on the latest <target>").
    pub overtaken: u32,
    /// 011: the target commit last merged in by an update; the lock commit before any. The gate
    /// baseline is looked up by this commit.
    pub base_commit: String,
    /// 011: the target commit the latest "Lock again" was reconciled against.
    pub reconciled_target: Option<String>,
    /// 011: when the loop joined its target's merge line; `None` when not in it.
    pub lined_at: Option<String>,
    /// 011: files the loop's accepted steps changed.
    pub touched: BTreeSet<String>,
    /// 011: files the locked tasks name.
    pub task_paths: BTreeSet<String>,
    /// 011: other unfinished loops touching the same files.
    pub overlaps: Vec<OverlapRec>,
    /// 011: the loop this one waits for, and who chose it.
    pub waits_for: Option<(u64, WaitFrom)>,
    /// 011: the parts switched on for this run.
    pub switches: Switches,
    /// 011: target commits a merge or publication "without the latest check" was not verified
    /// with, or that a pull request left as it is was not verified with.
    pub unchecked: Vec<String>,
    /// 011: each update's steps, for the history (FR-008): when, what, and whether it went well.
    pub update_steps: Vec<(String, String, bool)>,
    /// 011: each merged update's number, merge commit and the target commit it merged in.
    pub update_merges: Vec<(u32, String, String)>,
}

/// 011: an update of the loop in progress (data-model.md).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateState {
    pub n: u32,
    pub target: String,
    pub target_commit: String,
    pub loop_head: String,
    pub remote: bool,
    pub conflicts: Vec<UpdateConflict>,
    pub working: Option<String>,
    pub branch: Option<String>,
    pub docs_changed: Vec<String>,
    pub merged: bool,
    pub started_seq: u64,
}

/// 011: another unfinished loop touching the same files.
#[derive(Debug, Clone, Serialize)]
pub struct OverlapRec {
    pub with: u64,
    pub files: Vec<String>,
}

/// The paths the tasks of `phases` name (011 research R-8).
fn task_paths(phases: &[PhaseSpec]) -> BTreeSet<String> {
    phases
        .iter()
        .flat_map(|p| p.tasks.iter())
        .flat_map(|t| crate::speckit::tasks::paths_in(&t.text))
        .collect()
}

/// 010: a loop's pull request (`Published`) and its latest state (`PullRequestState`).
#[derive(Debug, Clone, Serialize)]
pub struct Publication {
    pub remote: String,
    pub service: String,
    pub repo: String,
    pub branch: String,
    pub commit: String,
    pub target: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub by: String,
    pub state: crate::publish::host::PrStatus,
    pub checks: Vec<crate::publish::host::Check>,
    pub merged_at: Option<String>,
    /// When it was published.
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishFailure {
    pub step: String,
    pub why: String,
    pub pushed: Option<(String, String)>,
    pub by: String,
    pub at: String,
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
    /// When the stage last changed.
    pub stage_at: String,
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
            stage_at: first.at.clone(),
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
        // Unlocked, the loop waits for the engineer's new lock (research S-17).
        let unlocked = self.feature_loop.as_ref().is_some_and(|f| f.unlocked);
        // Ready, or ended, the loop's clock stops: what follows is the engineer's time.
        let done = self.stage == Stage::Ready || self.stage.is_final();
        let waiting = (self.needs.is_some() || unlocked || done) && self.paused_since.is_none();
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
                if *to == Stage::Ready && self.stage.may_go_to(*to) {
                    if let Some(f) = self.feature_loop.as_mut() {
                        if f.update.as_ref().is_some_and(|u| u.merged) {
                            let n = f.update.take().map_or(0, |u| u.n);
                            f.update_steps.push((
                                l.at.clone(),
                                format!("Update {n}: verified again, ready"),
                                true,
                            ));
                        }
                    }
                }
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
                self.stage_at = l.at.clone();
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
                        started_seq: 0,
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
                    st.started_seq = l.seq;
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
                if *kind == SteeringKind::Publish {
                    if let Some(f) = self.feature_loop.as_mut() {
                        f.publish_pending = true;
                        f.publish_tried = true;
                    }
                }
                if *kind == SteeringKind::Update {
                    if let Some(f) = self.feature_loop.as_mut() {
                        f.overtaken = 0;
                    }
                }
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
                    at: l.at.clone(),
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
            Event::Merged {
                commit, unchecked, ..
            } => {
                self.merged = Some(commit.clone());
                if let Some(f) = self.feature_loop.as_mut() {
                    f.overtaken = 0;
                    f.lined_at = None;
                    f.unchecked = unchecked.clone();
                }
            }
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
                when_verified,
                update_limit,
                run_after: _,
                switches,
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
                                since: 0,
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
                    relocked_seq: 0,
                    outcome: None,
                    when_verified: *when_verified,
                    publication: None,
                    publish_failed: None,
                    publish_pending: false,
                    publish_tried: false,
                    publish_steps: Vec::new(),
                    publish_waited: None,
                    fresh_seq: 0,
                    update: None,
                    updates: 0,
                    update_limit: (*update_limit).clamp(1, 20),
                    overtaken: 0,
                    base_commit: lock_commit.clone(),
                    reconciled_target: None,
                    lined_at: None,
                    touched: BTreeSet::new(),
                    task_paths: task_paths(phases),
                    overlaps: Vec::new(),
                    waits_for: None,
                    switches: *switches,
                    unchecked: Vec::new(),
                    update_steps: Vec::new(),
                    update_merges: Vec::new(),
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
                recommended,
                cost_usd,
                reopened,
                why,
            } => {
                self.cost_usd += cost_usd;
                let step_phase = self.steps.get(step).and_then(|s| s.phase);
                if let Some(f) = self.feature_loop.as_mut() {
                    let asked_seq = f.questions.get(id).map_or(l.seq, |q| q.asked_seq);
                    let prev_why = f.questions.get(id).and_then(|q| q.why.clone());
                    let mut history = f
                        .questions
                        .get(id)
                        .map(|q| q.history.clone())
                        .unwrap_or_default();
                    history.push(QuestionChange {
                        at: l.at.clone(),
                        seq: l.seq,
                        state: *state,
                        by: by.clone(),
                        answer: answer.clone(),
                        commit: commit.clone(),
                    });
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
                            recommended: recommended.clone(),
                            reopened: reopened.clone(),
                            why: why.clone().or(prev_why),
                            seq: l.seq,
                            asked_seq,
                            history,
                        },
                    );
                    if *state == QuestionState::Overruled {
                        // The work built on the old answer is done again: the phase that holds
                        // the story's checkpoint (or the asking step's phase) is open again,
                        // in its session, and the loop is no longer ready (research S-10).
                        let of_story = |p: &PhaseStatus| {
                            affects.as_ref().is_some_and(|a| {
                                p.spec.story.as_ref() == Some(a)
                                    || p.spec.tasks.iter().any(|t| t.story.as_ref() == Some(a))
                            })
                        };
                        let target = f
                            .phases
                            .iter()
                            .rev()
                            .find(|p| of_story(p))
                            .map(|p| p.spec.n)
                            .or(step_phase);
                        if let Some(p) = f.phases.iter_mut().find(|p| Some(p.spec.n) == target) {
                            if p.state == PhaseState::Done {
                                p.state = PhaseState::Building;
                            }
                        }
                        f.outcome = None;
                    }
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
                reconciled_target,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.unlocked = false;
                    f.fresh_seq = l.seq;
                    // The base follows the new lock until an update merged a target in.
                    if f.base_commit == f.lock_commit {
                        f.base_commit = commit.clone();
                    }
                    if reconciled_target.is_some() {
                        f.reconciled_target = reconciled_target.clone();
                    }
                    if !phases.is_empty() {
                        f.task_paths = task_paths(phases);
                    }
                    f.lock = *lock;
                    f.prev_lock_commit =
                        Some(std::mem::replace(&mut f.lock_commit, commit.clone()));
                    f.affected = affected.clone();
                    f.outcome = None;
                    // Paused when it was ready: going on builds the change, never back to a
                    // ready stage without an outcome (constitution I).
                    if self.stage == Stage::Paused && self.paused_from == Some(Stage::Ready) {
                        self.paused_from = Some(Stage::Building);
                    }
                    f.relocked_seq = l.seq;
                    if !phases.is_empty() {
                        // The new lock's tasks: a phase keeps where it was when it is the same
                        // phase (place and title), and a new or moved phase starts fresh; a task
                        // keeps its result only when its text is the same (ids can be renumbered).
                        let old = std::mem::take(&mut f.phases);
                        let old_text = |id: &str| {
                            old.iter()
                                .flat_map(|p| p.spec.tasks.iter())
                                .find(|t| t.id == id)
                                .map(|t| t.text.clone())
                        };
                        for p in phases {
                            let kept = old
                                .iter()
                                .find(|o| o.spec.n == p.n && o.spec.title == p.title);
                            f.phases.push(PhaseStatus {
                                spec: p.clone(),
                                state: kept.map_or(PhaseState::Pending, |k| k.state),
                                session: kept.and_then(|k| k.session.clone()),
                                handover: kept.and_then(|k| k.handover.clone()),
                                reopened: if kept.is_some() { None } else { Some(l.seq) },
                            });
                            for t in &p.tasks {
                                let same = old_text(&t.id).as_deref() == Some(t.text.as_str());
                                let fresh = TaskStatus {
                                    phase: p.n,
                                    state: TaskState::Pending,
                                    commit: None,
                                    repairs: 0,
                                    since: l.seq,
                                };
                                match f.tasks.get_mut(&t.id) {
                                    Some(s) if same => s.phase = p.n,
                                    Some(s) => *s = fresh,
                                    None => {
                                        f.tasks.insert(t.id.clone(), fresh);
                                    }
                                }
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
            Event::Published {
                remote,
                service,
                repo,
                branch,
                commit,
                target,
                pr_number,
                pr_url,
                by,
                unchecked,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.overtaken = 0;
                    f.unchecked = unchecked.clone();
                    f.publication = Some(Publication {
                        remote: remote.clone(),
                        service: service.clone(),
                        repo: repo.clone(),
                        branch: branch.clone(),
                        commit: commit.clone(),
                        target: target.clone(),
                        pr_number: *pr_number,
                        pr_url: pr_url.clone(),
                        by: by.clone(),
                        state: crate::publish::host::PrStatus::Open,
                        checks: Vec::new(),
                        merged_at: None,
                        at: l.at.clone(),
                    });
                    f.publish_failed = None;
                    f.publish_pending = false;
                    f.publish_steps.push((
                        l.at.clone(),
                        format!(
                            "Pushed {branch} at {} to {remote} and opened pull request #{pr_number} into {target} on {repo}{}",
                            &commit[..commit.len().min(7)],
                            if by == "when_verified" { ", when verified as chosen at the lock" } else { "" }
                        ),
                        true,
                    ));
                }
            }
            Event::PublishFailed {
                step,
                why,
                pushed,
                by,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.publish_failed = Some(PublishFailure {
                        step: step.clone(),
                        why: why.clone(),
                        pushed: pushed.clone(),
                        by: by.clone(),
                        at: l.at.clone(),
                    });
                    f.publish_pending = false;
                    f.publish_steps.push((
                        l.at.clone(),
                        format!(
                            "Publishing stopped at the {}: {why}{}",
                            match step.as_str() {
                                "check" => "check of the remote",
                                "push" => "push",
                                _ => "pull request",
                            },
                            match pushed {
                                Some((b, c)) =>
                                    format!(" ({b} was pushed at {})", &c[..c.len().min(7)]),
                                None => String::new(),
                            }
                        ),
                        false,
                    ));
                }
            }
            Event::PublishWaited { why } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.publish_tried = true;
                    f.publish_waited = Some(why.clone());
                }
            }
            Event::PullRequestState {
                state,
                checks,
                merged_at,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if let Some(p) = f.publication.as_mut() {
                        let n = p.pr_number;
                        let was = p.state;
                        p.state = *state;
                        p.checks = checks.clone();
                        p.merged_at = merged_at.clone();
                        let what = match state {
                            crate::publish::host::PrStatus::Merged if was != *state => {
                                format!("Pull request #{n} was merged on {}", p.repo)
                            }
                            crate::publish::host::PrStatus::Closed if was != *state => {
                                format!("Pull request #{n} was closed without merging")
                            }
                            _ => {
                                let list: Vec<String> = checks
                                    .iter()
                                    .map(|c| {
                                        format!(
                                            "{} {}",
                                            c.name,
                                            serde_json::to_value(c.state)
                                                .ok()
                                                .and_then(|v| v.as_str().map(str::to_string))
                                                .unwrap_or_default()
                                        )
                                    })
                                    .collect();
                                format!(
                                    "Pull request #{n}: checks {}",
                                    if list.is_empty() {
                                        "none yet".into()
                                    } else {
                                        list.join(", ")
                                    }
                                )
                            }
                        };
                        let ok = !checks
                            .iter()
                            .any(|c| c.state == crate::publish::host::CheckState::Failed)
                            && *state != crate::publish::host::PrStatus::Closed;
                        f.publish_steps.push((l.at.clone(), what, ok));
                    }
                }
            }
            Event::Updated {
                n,
                target,
                target_commit,
                loop_head,
                remote,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.fresh_seq = l.seq;
                    f.updates += 1;
                    f.outcome = None;
                    f.update = Some(UpdateState {
                        n: *n,
                        target: target.clone(),
                        target_commit: target_commit.clone(),
                        loop_head: loop_head.clone(),
                        remote: *remote,
                        conflicts: Vec::new(),
                        working: None,
                        branch: None,
                        docs_changed: Vec::new(),
                        merged: false,
                        started_seq: l.seq,
                    });
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "Update {n}: bringing the loop up to date with {target}{} at {}",
                            if *remote { " on the remote" } else { "" },
                            short(target_commit)
                        ),
                        true,
                    ));
                }
            }
            Event::UpdateConflicted {
                n,
                working,
                branch,
                conflicts,
            } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if let Some(u) = f.update.as_mut().filter(|u| u.n == *n) {
                        u.working = Some(working.clone());
                        u.branch = Some(branch.clone());
                        u.conflicts = conflicts.clone();
                    }
                    let files: Vec<&str> = conflicts.iter().map(|c| c.path.as_str()).collect();
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "Update {n}: conflicts in {}; a builder resolves them",
                            files.join(", ")
                        ),
                        // The usual way of an update with conflicts, not a failure.
                        true,
                    ));
                }
            }
            Event::UpdateDocsChanged { n, paths } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if let Some(u) = f.update.as_mut().filter(|u| u.n == *n) {
                        u.docs_changed = paths.clone();
                    }
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "Update {n}: the target changed this feature's documents ({}); nothing merged, your decision",
                            paths.join(", ")
                        ),
                        false,
                    ));
                }
            }
            Event::UpdateMerged { n, commit } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if let Some(u) = f.update.as_mut().filter(|u| u.n == *n) {
                        u.merged = true;
                        u.working = None;
                        f.base_commit = u.target_commit.clone();
                        f.update_merges
                            .push((*n, commit.clone(), u.target_commit.clone()));
                    }
                    f.outcome = None;
                    // Every story's phases are checked again (research R-3); `affected` is left
                    // as it is, so no relock repair is queued.
                    for p in &mut f.phases {
                        let story = p.spec.story.is_some()
                            || p.spec.tasks.iter().any(|t| t.story.is_some());
                        if story {
                            p.state = PhaseState::Pending;
                            p.reopened = Some(l.seq);
                        }
                    }
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "Update {n}: merged in by Fallow ({}); gates and every story checked again",
                            short(commit)
                        ),
                        true,
                    ));
                }
            }
            Event::UpdateDropped { n, why } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if f.update.as_ref().is_some_and(|u| u.n == *n) {
                        f.update = None;
                    }
                    f.update_steps.push((
                        l.at.clone(),
                        format!("Update {n} dropped: {why}"),
                        false,
                    ));
                }
            }
            Event::PushedForward { from, to } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.overtaken = 0;
                    f.update = None;
                    if let Some(p) = f.publication.as_mut() {
                        p.commit = to.clone();
                    }
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "The loop's branch on the remote moved forward from {} to {}; the pull request shows it",
                            short(from),
                            short(to)
                        ),
                        true,
                    ));
                }
            }
            Event::LeftPullRequest { unchecked } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.unchecked = unchecked.clone();
                    f.update = None;
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "The pull request was left as it is: not verified with {} newer commit(s) of the target",
                            unchecked.len()
                        ),
                        false,
                    ));
                }
            }
            Event::Touched { files } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.touched.extend(files.iter().cloned());
                }
            }
            Event::Overtaken { n, target_commit } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.overtaken += 1;
                    f.update_steps.push((
                        l.at.clone(),
                        format!(
                            "Update {n} was overtaken: the target moved again (now at {}); {} of {} in a row",
                            short(target_commit),
                            f.overtaken,
                            f.update_limit
                        ),
                        false,
                    ));
                }
            }
            Event::Lined => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.lined_at = Some(l.at.clone());
                }
            }
            Event::Unlined { .. } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.lined_at = None;
                }
            }
            Event::Overlap { with, files } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    match f.overlaps.iter_mut().find(|o| o.with == *with) {
                        Some(o) => {
                            for file in files {
                                if !o.files.contains(file) {
                                    o.files.push(file.clone());
                                }
                            }
                        }
                        None => f.overlaps.push(OverlapRec {
                            with: *with,
                            files: files.clone(),
                        }),
                    }
                }
            }
            Event::WaitsFor { run, from } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    f.waits_for = Some((*run, *from));
                }
            }
            Event::WaitEnded { run, .. } => {
                if let Some(f) = self.feature_loop.as_mut() {
                    if f.waits_for.is_some_and(|(r, _)| r == *run) {
                        f.waits_for = None;
                    }
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
        let newest = || {
            self.gate_runs
                .iter()
                .rev()
                .find(|g| g.kind == GateKind::Baseline && g.result.gate == gate)
                .map(|g| &g.result)
        };
        // 011: a loop's baseline is the one on its base (research R-3).
        match self.feature_loop.as_ref().map(|f| f.base_commit.as_str()) {
            Some(base) => self
                .gate_runs
                .iter()
                .rev()
                .find(|g| {
                    g.kind == GateKind::Baseline && g.result.gate == gate && g.result.commit == base
                })
                .map(|g| &g.result)
                .or_else(newest),
            None => newest(),
        }
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

/// A commit's short form for the history.
fn short(c: &str) -> &str {
    &c[..c.len().min(7)]
}
