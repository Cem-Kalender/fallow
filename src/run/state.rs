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
    /// The latest event number.
    pub seq: u64,
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
            seq: first.seq,
        };
        for l in &events[1..] {
            s.apply(l);
        }
        Ok(s)
    }

    /// Applies one event.
    pub fn apply(&mut self, l: &Logged) {
        self.seq = l.seq;
        match &l.event {
            Event::RunCreated { .. } => self
                .notes
                .push(format!("event {}: a second run_created was ignored", l.seq)),
            Event::PlanSaid { text } => self.conversation.push(Turn {
                who: "engineer",
                text: text.clone(),
                at: l.at.clone(),
            }),
            Event::PlanReply { reply, ready, .. } => {
                self.conversation.push(Turn {
                    who: "planner",
                    text: reply.clone(),
                    at: l.at.clone(),
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
                } else if self.stage == Stage::Paused {
                    self.paused_from = None;
                    self.limit_reached = None;
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
            } => {
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
            Event::Merged { commit, .. } => self.merged = Some(commit.clone()),
            Event::Stopped { reason } => self.stopped = Some(reason.clone()),
            Event::Note { text } => self.notes.push(text.clone()),
        }
    }

    /// The next step number.
    pub fn next_step(&self) -> u32 {
        self.steps.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The session of the latest step of `kind` (and `part`) that started, to resume it.
    pub fn session_of(&self, role: &str, part: Option<&str>) -> Option<String> {
        self.steps
            .values()
            .rev()
            .filter(|s| s.kind.role() == role && s.part.as_deref() == part)
            .find_map(|s| s.session.clone())
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

    /// Minutes since building started.
    pub fn minutes_used(&self, now: u64) -> u64 {
        self.building_started
            .as_deref()
            .and_then(super::time::parse_rfc3339)
            .map_or(0, |t| now.saturating_sub(t) / 60)
    }
}
