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
            // 007: a feature's loop reviews within Building (at each checkpoint) and is ready
            // when its last phase is done.
            (Building, Integrating | Gating | Reviewing | Ready) => true,
            (Integrating, Gating | Building | Reviewing) => true,
            (Gating, Reviewing | Building) => true,
            (Reviewing, Ready | Building) => true,
            (Ready, Done | Publishing | Building) => true,
            // 010: a pull request closed without merging puts the loop back to ready.
            (Publishing, Done | Building | Ready) => true,
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
    /// 007: the phase's builder writes its handover (FR-010a).
    Handover,
    /// 007: a maker step of the pipeline other than the builder (a test-writer, a docs-writer).
    Make,
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
            StepKind::Handover => "handover",
            StepKind::Make => "make",
        }
    }

    pub fn role(self) -> &'static str {
        match self {
            StepKind::PlanTurn | StepKind::Decide => "planner",
            StepKind::Build
            | StepKind::Take
            | StepKind::Repair
            | StepKind::Plain
            | StepKind::Handover
            | StepKind::Make => "builder",
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
    /// Held at the repair cap until the engineer allows another repair (FR-008).
    AtCap,
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
    /// 007: the engineer unlocked the feature (FR-009).
    Unlock,
    /// 010: publishing as a pull request, by the engineer or when verified as chosen at the lock.
    Publish,
    /// 011: bring the loop up to date (also "Verify again on the latest <target>").
    Update,
    /// 011: merge without the latest check, at the limit of overtaken updates.
    MergeUnchecked,
    /// 011: publish without the latest check, at the limit.
    PublishUnchecked,
    /// 011: leave the pull request as it is, at the limit.
    LeavePullRequest,
    /// 011: lock again after the target changed the feature's documents.
    LockAgain,
    /// 011: wait for another loop.
    WaitFor,
    /// 011: stop waiting for another loop.
    StopWaiting,
}

impl SteeringKind {
    /// Whether it counts as an intervention (FR-020): everything except approving the plan and
    /// merging, which the success criteria count apart.
    pub fn intervention(self) -> bool {
        !matches!(
            self,
            SteeringKind::ApprovePlan | SteeringKind::Merge | SteeringKind::Publish
        )
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
        /// 007: the task, the phase and the agent definition of a loop's step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        def: Option<String>,
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
        /// One more step past the limits (FR-038 "continue once").
        #[serde(default)]
        once: bool,
    },
    /// An agent session that could not be resumed; it is never resumed again.
    SessionLost {
        session: String,
    },
    /// What needed the engineer no longer does (a retry, or a transient error after a restart).
    NeedsCleared {
        why: String,
    },
    Merged {
        commit: String,
        how: String,
        /// 011: target commits a "Merge without the latest check" was not verified with.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unchecked: Vec<String>,
    },
    Stopped {
        reason: Option<String>,
    },
    Note {
        text: String,
    },
    /// 007: a loop over a locked feature's `tasks.md`; follows `run_created` (contracts/storage.md).
    LoopCreated {
        project: String,
        feature: String,
        lock: u32,
        lock_commit: String,
        base_branch: String,
        build_branch: String,
        /// The pipeline used, resolved, as TOML.
        pipeline: String,
        level: Level,
        budget_usd: f64,
        budget_minutes: u64,
        phases: Vec<PhaseSpec>,
        /// 010: what happens when the loop ends verified.
        #[serde(default)]
        when_verified: WhenVerified,
        /// 011: how many updates in a row may be overtaken before the engineer decides.
        #[serde(default = "update_limit")]
        update_limit: u32,
        /// 011: the loop this one waits for, chosen at the lock ("Run after").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_after: Option<u64>,
        /// 011: the parts switched on for this run (`.fallow/squad.toml` `[updates]`).
        #[serde(default)]
        switches: Switches,
    },
    Task {
        id: String,
        state: TaskState,
        commit: Option<String>,
        repairs: u32,
    },
    Phase {
        n: u32,
        state: PhaseState,
        session: Option<String>,
        handover: Option<String>,
    },
    Question {
        id: String,
        step: u32,
        task: Option<String>,
        text: String,
        options: Vec<String>,
        state: QuestionState,
        by: Option<String>,
        answer: Option<String>,
        commit: Option<String>,
        affects: Option<String>,
        /// The lead's recommendation, for a question relayed to the engineer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recommended: Option<String>,
        /// What the lead's call cost; it counts against the loop's budget (research S-10).
        #[serde(default, skip_serializing_if = "is_zero_f")]
        cost_usd: f64,
        /// Overruled: the tasks built on the old answer, reopened.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reopened: Vec<String>,
        /// Relayed: why it went to the engineer (the lead relayed it, a rule of Fallow's, a
        /// failed or slow call).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    Unlocked,
    Relocked {
        lock: u32,
        commit: String,
        affected: Vec<String>,
        /// The phases of the new lock's `tasks.md` (it may have changed).
        #[serde(default)]
        phases: Vec<PhaseSpec>,
        /// 011: the target commit a "Lock again" was reconciled against.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reconciled_target: Option<String>,
    },
    /// How a loop ended: verified (ready to merge), or done but not verified, naming what was
    /// not checked (FR-015).
    Outcome {
        verified: bool,
        missing: Vec<String>,
    },
    /// 010: the loop's branch pushed at the verified commit, and a pull request opened from it
    /// (FR-009). `by` is `engineer` or `when_verified`.
    Published {
        remote: String,
        service: String,
        repo: String,
        branch: String,
        commit: String,
        target: String,
        pr_number: u64,
        pr_url: String,
        by: String,
        /// 011: target commits a "Publish without the latest check" was not verified with.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unchecked: Vec<String>,
    },
    /// 010: publishing failed at `step` (`check`, `push`, `pull_request`); what was pushed, if
    /// anything, is named (FR-012).
    PublishFailed {
        step: String,
        why: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pushed: Option<(String, String)>,
        /// `engineer` or `when_verified`.
        #[serde(default = "engineer")]
        by: String,
        /// 011 (FR-011a): the remote target's commit when it moved since the loop was verified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        moved_to: Option<String>,
    },
    /// 010: a loop chosen to publish itself when verified did not, and why (FR-020): final, it
    /// waits for the engineer from then on.
    PublishWaited {
        why: String,
    },
    /// 010: the pull request's state and checks, as the remote reports them; recorded when
    /// something changed (FR-013).
    PullRequestState {
        state: crate::publish::host::PrStatus,
        #[serde(default)]
        checks: Vec<crate::publish::host::Check>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merged_at: Option<String>,
        /// 011: conflicts with its target, behind it, and whether the target requires it to be
        /// up to date (research R-7).
        #[serde(default)]
        conflicting: bool,
        #[serde(default)]
        behind: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
    /// 011: an update started (research R-2), written before the merge is computed.
    Updated {
        n: u32,
        target: String,
        target_commit: String,
        loop_head: String,
        #[serde(default)]
        remote: bool,
        /// Started by the loop holding its line's turn, for the merge asked for (R-6).
        #[serde(default)]
        in_turn: bool,
        /// Started for a publication: the remote target moved before the first push (FR-011a);
        /// any move of it until the push counts as overtaken (FR-007).
        #[serde(default)]
        for_publication: bool,
    },
    /// 011: the update's merge conflicts; the repair starts on `working`, kept on the loop's own
    /// branch `branch`.
    UpdateConflicted {
        n: u32,
        working: String,
        branch: String,
        conflicts: Vec<UpdateConflict>,
    },
    /// 011: the target changed the feature's locked documents; nothing merged.
    UpdateDocsChanged {
        n: u32,
        paths: Vec<String>,
    },
    /// 011: Fallow's two-parent merge commit is on the loop's branch.
    UpdateMerged {
        n: u32,
        commit: String,
    },
    /// 011: the update was dropped (a pull request merged or closed, the loop stopped, locked
    /// again).
    UpdateDropped {
        n: u32,
        why: String,
    },
    /// 011: the loop's branch on the remote moved forward from `from` to `to`.
    PushedForward {
        from: String,
        to: String,
    },
    /// 011: "Leave the pull request as it is" at the limit.
    LeftPullRequest {
        #[serde(default)]
        unchecked: Vec<String>,
    },
    /// 011: files an accepted step changed (the overlap warnings).
    Touched {
        files: Vec<String>,
    },
    /// 011: the target moved again while the loop held its turn.
    Overtaken {
        n: u32,
        target_commit: String,
    },
    /// 011: joined the merge line of its target.
    Lined {
        /// When, in milliseconds since the epoch: the line's order (research R-5).
        at_ms: u64,
    },
    /// 011: left the merge line.
    Unlined {
        why: String,
    },
    /// 011: another unfinished loop touches the same files.
    Overlap {
        with: u64,
        files: Vec<String>,
    },
    /// 011: does not start steps until loop `run` is merged, stopped or deleted.
    WaitsFor {
        run: u64,
        from: WaitFrom,
    },
    /// 011: the wait is over.
    WaitEnded {
        run: u64,
        why: String,
    },
}

/// 011: a conflicting path of an update, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateConflict {
    pub path: String,
    pub kind: ConflictKindRec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKindRec {
    Text,
    NonText,
}

/// 011: who made a loop wait for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitFrom {
    Lock,
    Engineer,
}

/// 011: the parts of an update that act on their own, each switchable per project (FR-016c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switches {
    #[serde(default = "yes")]
    pub auto: bool,
    #[serde(default = "yes")]
    pub line: bool,
    #[serde(default = "yes")]
    pub overlap: bool,
}

impl Default for Switches {
    fn default() -> Self {
        Switches {
            auto: true,
            line: true,
            overlap: true,
        }
    }
}

fn yes() -> bool {
    true
}

/// 011: the default number of overtaken updates before the engineer decides.
pub fn update_limit() -> u32 {
    3
}

fn engineer() -> String {
    "engineer".into()
}

/// 010: what a loop does when it ends verified (FR-019): wait for the engineer, or open a pull
/// request on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WhenVerified {
    #[default]
    Wait,
    PullRequest,
}

impl WhenVerified {
    /// The preset for a level: Free opens a pull request (agreed 2026-11-02).
    pub fn preset(level: Level) -> WhenVerified {
        match level {
            Level::Free => WhenVerified::PullRequest,
            _ => WhenVerified::Wait,
        }
    }

    pub fn parse(s: &str) -> Option<WhenVerified> {
        match s {
            "wait" => Some(WhenVerified::Wait),
            "pull_request" => Some(WhenVerified::PullRequest),
            _ => None,
        }
    }
}

/// How much the lead decides without the engineer (FR-020a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Strict,
    #[default]
    Normal,
    Free,
}

/// A task of `tasks.md`, as locked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub story: Option<String>,
    pub parallel: bool,
    pub text: String,
}

/// A phase of `tasks.md`, as locked. `n` is its place (1, 2, …); `label` is the number the file
/// gives it (`3a`). `story` is set for a user story's phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseSpec {
    pub n: u32,
    #[serde(default)]
    pub label: String,
    pub title: String,
    pub story: Option<String>,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Current,
    Accepted,
    Failed,
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    Pending,
    Building,
    Checkpoint,
    Repairing,
    Done,
    Reopened,
    Waiting,
}

fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    WithLead,
    Answered,
    Decided,
    WaitingEngineer,
    Overruled,
    /// A decision of the lead the engineer confirmed (FR-020a).
    Confirmed,
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
            Event::SessionLost { .. } => "session_lost",
            Event::NeedsCleared { .. } => "needs_cleared",
            Event::Merged { .. } => "merged",
            Event::Stopped { .. } => "stopped",
            Event::Note { .. } => "note",
            Event::LoopCreated { .. } => "loop_created",
            Event::Task { .. } => "task",
            Event::Phase { .. } => "phase",
            Event::Question { .. } => "question",
            Event::Unlocked => "unlocked",
            Event::Relocked { .. } => "relocked",
            Event::Outcome { .. } => "outcome",
            Event::Published { .. } => "published",
            Event::PublishFailed { .. } => "publish_failed",
            Event::PullRequestState { .. } => "pull_request_state",
            Event::PublishWaited { .. } => "publish_waited",
            Event::Updated { .. } => "updated",
            Event::UpdateConflicted { .. } => "update_conflicted",
            Event::UpdateDocsChanged { .. } => "update_docs_changed",
            Event::UpdateMerged { .. } => "update_merged",
            Event::UpdateDropped { .. } => "update_dropped",
            Event::PushedForward { .. } => "pushed_forward",
            Event::LeftPullRequest { .. } => "left_pull_request",
            Event::Touched { .. } => "touched",
            Event::Overtaken { .. } => "overtaken",
            Event::Lined { .. } => "lined",
            Event::Unlined { .. } => "unlined",
            Event::Overlap { .. } => "overlap",
            Event::WaitsFor { .. } => "waits_for",
            Event::WaitEnded { .. } => "wait_ended",
        }
    }
}

/// An event with its place in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logged<E = Event> {
    pub seq: u64,
    pub at: String,
    #[serde(flatten)]
    pub event: E,
}

/// The append-only log of one run, or (with another event type) of one conversation.
pub struct Log<E = Event> {
    path: PathBuf,
    file: std::fs::File,
    next: u64,
    _events: std::marker::PhantomData<E>,
}

impl<E> std::fmt::Debug for Log<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Log").field("path", &self.path).finish()
    }
}

pub const EVENTS_FILE: &str = "events.jsonl";

/// An opened log, the events it holds, and notes about what was dropped.
pub type Opened<E> = (Log<E>, Vec<Logged<E>>, Vec<String>);

impl<E: Serialize + serde::de::DeserializeOwned> Log<E> {
    /// Opens (creating) the log in `dir` and reads every event. A last line that is cut off (a
    /// crash during a write) is dropped and reported in `notes`.
    pub fn open(dir: &Path) -> Result<Opened<E>, String> {
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
            match serde_json::from_str::<Logged<E>>(trimmed) {
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
        Ok((
            Log {
                path,
                file,
                next,
                _events: std::marker::PhantomData,
            },
            events,
            notes,
        ))
    }

    /// Appends an event, flushed and synced before returning (research R-2).
    pub fn append(&mut self, event: E) -> Result<Logged<E>, String> {
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
