//! A run's event log, `runs/<id>/events.jsonl` (006 research R-2, contracts/storage.md).
//!
//! The log is the only authoritative state: an event is written, flushed and synced before
//! Fallow acts on it, and the run's state is the fold of its events (`state::RunState::fold`).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Autonomy, RoleChoice};
use crate::verify::gates::{Class, GateResult};

/// What kind of run (FR-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Solo,
    Squad,
    Plain,
}

/// A run's stage (data-model.md Run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Planning,
    Building,
    Integrating,
    Gating,
    Reviewing,
    Ready,
    Publishing,
    Paused,
    Done,
    Stopped,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Planning => "planning",
            Stage::Building => "building",
            Stage::Integrating => "integrating",
            Stage::Gating => "gating",
            Stage::Reviewing => "reviewing",
            Stage::Ready => "ready to merge",
            Stage::Publishing => "publishing",
            Stage::Paused => "paused",
            Stage::Done => "done",
            Stage::Stopped => "stopped",
        }
    }

    pub fn is_final(self) -> bool {
        matches!(self, Stage::Done | Stage::Stopped)
    }

    /// Whether the stage machine allows `self → to`.
    pub fn may_go_to(self, to: Stage) -> bool {
        use Stage::*;
        if self.is_final() {
            return false;
        }
        match (self, to) {
            (_, Paused | Stopped) => self != Paused || to == Stopped,
            (Paused, _) => true,
            (Planning, Building) => true,
            (Building, Integrating | Gating | Reviewing) => true,
            (Integrating, Gating | Building | Reviewing) => true,
            (Gating, Reviewing | Building) => true,
            (Reviewing, Ready | Building) => true,
            (Ready, Done | Publishing | Building) => true,
            (Publishing, Done | Building) => true,
            _ => false,
        }
    }
}

/// What a step does (data-model.md Step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    PlanTurn,
    Build,
    Take,
    Repair,
    Decide,
    Review,
    Plain,
}

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            StepKind::PlanTurn => "plan_turn",
            StepKind::Build => "build",
            StepKind::Take => "take",
            StepKind::Repair => "repair",
            StepKind::Decide => "decide",
            StepKind::Review => "review",
            StepKind::Plain => "plain",
        }
    }

    pub fn role(self) -> &'static str {
        match self {
            StepKind::PlanTurn | StepKind::Decide => "planner",
            StepKind::Build | StepKind::Take | StepKind::Repair | StepKind::Plain => "builder",
            StepKind::Review => "reviewer",
        }
    }
}

/// The run's limits (FR-038).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub usd: f64,
    pub minutes: u64,
    pub step_minutes: u64,
}

/// A gate as the run uses it, fixed at the run's start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSpec {
    pub name: String,
    pub command: String,
    pub timeout_min: u64,
    pub baseline: bool,
    /// `auto`, `exit-code` or a pattern.
    pub failures: String,
}

/// Which commit a gate ran on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Baseline,
    Part,
    Integration,
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StepEnd {
    /// The agent's structured result.
    Result { value: Value },
    /// No usable result: crash, time-out, error or a result outside the schema.
    Failed { why: String },
}

/// One reviewer answer (contracts/step-results.md, review).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub criterion: String,
    pub ran: String,
    pub result: String,
    pub passed: bool,
    #[serde(default)]
    pub failing_case: Option<String>,
}

/// The state of a part (data-model.md Part).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    Waiting,
    Building,
    Blocked,
    Done,
    Reopened,
    Repairing,
    WithEngineer,
    Paused,
}

/// The engineer's actions (data-model.md Steering action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteeringKind {
    ApprovePlan,
    Answer,
    Overrule,
    AddCriterion,
    ChangeCriterion,
    Note,
    Pause,
    Resume,
    Stop,
    TakeOver,
    HandBack,
    SetAutonomy,
    SetLimits,
    Merge,
    RequestChanges,
    Reject,
}

impl SteeringKind {
    /// Whether it counts as an intervention (FR-020): everything except approving the plan and
    /// merging, which the success criteria count apart.
    pub fn intervention(self) -> bool {
        !matches!(self, SteeringKind::ApprovePlan | SteeringKind::Merge)
    }
}

/// One event (contracts/storage.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunCreated {
        repo: PathBuf,
        target: String,
        start_commit: String,
        task: String,
        mode: Mode,
        autonomy: Autonomy,
        limits: Limits,
        roles: BTreeMap<String, RoleChoice>,
        repair_cap: u32,
        gates: Vec<GateSpec>,
        allow: BTreeMap<String, Vec<String>>,
        test_patterns: Vec<String>,
        recorded: bool,
        #[serde(default)]
        left_out: Vec<String>,
    },
    /// The engineer's turn in the planning conversation.
    PlanSaid {
        text: String,
    },
    /// The planner's reply.
    PlanReply {
        step: u32,
        reply: String,
        ready: bool,
    },
    PlanCommitted {
        commit: String,
        approvable: bool,
        problems: Vec<String>,
    },
    PlanApproved {
        commit: String,
    },
    Stage {
        to: Stage,
        why: String,
    },
    StepQueued {
        n: u32,
        kind: StepKind,
        part: Option<String>,
        /// The session to resume; none for a new one.
        resume: Option<String>,
        reason: String,
    },
    StepStarted {
        n: u32,
        agent: String,
        model: String,
        session: String,
        worktree: PathBuf,
        base: String,
    },
    StepFinished {
        n: u32,
        end: StepEnd,
        cost_usd: Option<f64>,
        duration_ms: u64,
        denials: Vec<String>,
    },
    Verified {
        n: u32,
        accepted: bool,
        reasons: Vec<String>,
        /// The verified commit, when accepted.
        #[serde(default)]
        commit: Option<String>,
    },
    GateRun {
        kind: GateKind,
        part: Option<String>,
        result: GateResult,
        class: Option<Class>,
    },
    Part {
        id: String,
        state: PartState,
        result_commit: Option<String>,
        repairs: u32,
    },
    Integrated {
        commit: String,
    },
    Verdict {
        step: u32,
        answers: Vec<Answer>,
        complete: bool,
        approved: bool,
        missing: Vec<String>,
    },
    Steering {
        kind: SteeringKind,
        target: Option<String>,
        detail: Option<String>,
    },
    /// Something only the engineer can resolve; cleared by the next `Stage` event.
    NeedsEngineer {
        why: String,
        options: Vec<String>,
    },
    LimitReached {
        which: String,
        used: String,
    },
    /// The engineer raised the run's limits or repair cap.
    LimitsSet {
        usd: Option<f64>,
        minutes: Option<u64>,
        repair_cap: Option<u32>,
    },
    Merged {
        commit: String,
        how: String,
    },
    Stopped {
        reason: Option<String>,
    },
    Note {
        text: String,
    },
}

impl Event {
    pub fn type_name(&self) -> &'static str {
        match self {
            Event::RunCreated { .. } => "run_created",
            Event::PlanSaid { .. } => "plan_said",
            Event::PlanReply { .. } => "plan_reply",
            Event::PlanCommitted { .. } => "plan_committed",
            Event::PlanApproved { .. } => "plan_approved",
            Event::Stage { .. } => "stage",
            Event::StepQueued { .. } => "step_queued",
            Event::StepStarted { .. } => "step_started",
            Event::StepFinished { .. } => "step_finished",
            Event::Verified { .. } => "verified",
            Event::GateRun { .. } => "gate_run",
            Event::Part { .. } => "part",
            Event::Integrated { .. } => "integrated",
            Event::Verdict { .. } => "verdict",
            Event::Steering { .. } => "steering",
            Event::NeedsEngineer { .. } => "needs_engineer",
            Event::LimitReached { .. } => "limit_reached",
            Event::LimitsSet { .. } => "limits_set",
            Event::Merged { .. } => "merged",
            Event::Stopped { .. } => "stopped",
            Event::Note { .. } => "note",
        }
    }
}

/// An event with its place in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logged {
    pub seq: u64,
    pub at: String,
    #[serde(flatten)]
    pub event: Event,
}

/// The append-only log of one run.
pub struct Log {
    path: PathBuf,
    file: std::fs::File,
    next: u64,
}

impl std::fmt::Debug for Log {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Log").field("path", &self.path).finish()
    }
}

pub const EVENTS_FILE: &str = "events.jsonl";

impl Log {
    /// Opens (creating) the log in `dir` and reads every event. A last line that is cut off (a
    /// crash during a write) is dropped and reported in `notes`.
    pub fn open(dir: &Path) -> Result<(Log, Vec<Logged>, Vec<String>), String> {
        crate::platform::private_dir(dir)?;
        let path = dir.join(EVENTS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut events = Vec::new();
        let mut notes = Vec::new();
        let mut good_len = 0usize;
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                good_len += line.len();
                continue;
            }
            match serde_json::from_str::<Logged>(trimmed) {
                Ok(l) if line.ends_with('\n') => {
                    events.push(l);
                    good_len += line.len();
                }
                _ => {
                    notes.push(format!(
                        "{}: an incomplete last event was dropped",
                        path.display()
                    ));
                    break;
                }
            }
        }
        if good_len < text.len() {
            // Cut the torn tail so later appends start on a fresh line.
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            f.set_len(good_len as u64)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let next = events.last().map_or(1, |l| l.seq + 1);
        Ok((Log { path, file, next }, events, notes))
    }

    /// Appends an event, flushed and synced before returning (research R-2).
    pub fn append(&mut self, event: Event) -> Result<Logged, String> {
        let logged = Logged {
            seq: self.next,
            at: super::time::rfc3339(super::time::unix_now()),
            event,
        };
        let mut line = serde_json::to_string(&logged).map_err(|e| e.to_string())?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .and_then(|()| self.file.sync_data())
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.next += 1;
        Ok(logged)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
