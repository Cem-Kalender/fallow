//! Runs (005 US2, research R-11 to R-13): task sets, plain, solo and squad runs in their own copy
//! of the project, the end of a run, interventions, hidden tests and series.
//!
//! This module holds what a run is and decides; the app starts its sessions, types its
//! description, closes its sessions after the engineer confirms, and runs its hidden tests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use toml::Spanned;

use crate::config::merge::line_of;

pub const TASKSET_FILE: &str = "taskset.toml";
/// The line every run's description ends with, the same in every mode (R-13).
pub const CLOSING_LINE: &str = "When you are finished, commit your work.";
pub const MAX_TASKS: usize = 100;
/// A run ends when nothing of it worked this long (squad setting `run_idle_end_min`).
pub const DEFAULT_IDLE_END: Duration = Duration::from_secs(3 * 60);

/// How a run is built (FR-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// One session with its skills, no Fallow orchestration tools.
    Plain,
    /// The lead builds alone, with Fallow's gates and a reviewer.
    Solo,
    /// The lead decides how to delegate.
    Squad,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "plain" => Mode::Plain,
            "solo" => Mode::Solo,
            "squad" => Mode::Squad,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Plain => "plain",
            Mode::Solo => "solo",
            Mode::Squad => "squad",
        }
    }

    pub const ALL: [Mode; 3] = [Mode::Plain, Mode::Solo, Mode::Squad];
}

/// A file the task set copies somewhere: from (in the task set) → to (in the run's copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub from: PathBuf,
    pub to: String,
}

/// One hidden test: a command that passes when it exits 0 within the time limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenTest {
    pub command: String,
    pub windows: Option<String>,
    pub macos: Option<String>,
    pub linux: Option<String>,
}

impl HiddenTest {
    pub fn command_for(&self, os: &str) -> &str {
        let o = match os {
            "windows" => &self.windows,
            "macos" => &self.macos,
            _ => &self.linux,
        };
        o.as_deref()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(&self.command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub project: PathBuf,
    pub start: String,
    pub description: String,
    pub several_components: bool,
    pub time_limit: Duration,
    pub hidden: Vec<Copy>,
    pub tests: Vec<HiddenTest>,
    /// The description's fingerprint (R-12), computed once at load.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSet {
    pub dir: PathBuf,
    pub setup: Vec<Copy>,
    pub tasks: Vec<Task>,
}

impl TaskSet {
    pub fn task(&self, name: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.name == name)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCopy {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTest {
    command: String,
    windows: Option<String>,
    macos: Option<String>,
    linux: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTask {
    name: String,
    project: String,
    start: String,
    description: String,
    #[serde(default)]
    several_components: bool,
    time_limit_min: Option<i64>,
    #[serde(default)]
    hidden: Vec<RawCopy>,
    #[serde(default)]
    tests: Vec<RawTest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTaskSet {
    #[serde(default)]
    setup: Vec<RawCopy>,
    #[serde(default)]
    task: Vec<Spanned<RawTask>>,
}

/// A relative path inside a folder: no root, no `..`.
fn inside(p: &str) -> bool {
    let path = Path::new(p);
    !p.trim().is_empty()
        && path.is_relative()
        && !path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
}

fn copies(raw: Vec<RawCopy>, dir: &Path, at: &str, problems: &mut Vec<String>) -> Vec<Copy> {
    let mut out = Vec::new();
    for c in raw {
        if !inside(&c.from) || !dir.join(&c.from).is_file() {
            problems.push(format!("{at}: {:?} is not a file in the task set", c.from));
            continue;
        }
        if !inside(&c.to) {
            problems.push(format!(
                "{at}: {:?}: a path inside the project, without ..",
                c.to
            ));
            continue;
        }
        out.push(Copy {
            from: dir.join(&c.from),
            to: c.to,
        });
    }
    out
}

/// Reads and checks a task set (contracts/task-set.md). Problems are `file:line: message`.
pub fn load(dir: &Path) -> Result<TaskSet, Vec<String>> {
    let path = dir.join(TASKSET_FILE);
    let file = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|e| vec![format!("{file}: {e}")])?;
    parse(dir, &file, &text)
}

pub fn parse(dir: &Path, file: &str, text: &str) -> Result<TaskSet, Vec<String>> {
    let raw: RawTaskSet = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return Err(vec![format!("{file}:{line}: {}", e.message())]);
        }
    };
    let mut problems = Vec::new();
    let setup = copies(raw.setup, dir, &format!("{file}:1"), &mut problems);
    if raw.task.len() > MAX_TASKS {
        problems.push(format!("{file}:1: at most {MAX_TASKS} tasks"));
    }
    let mut tasks: Vec<Task> = Vec::new();
    for t in raw.task {
        let at = format!("{file}:{}", line_of(text, t.span().start));
        let t = t.into_inner();
        if !super::manifest::valid_id(&t.name) {
            problems.push(format!(
                "{at}: task name {:?}: expected [a-z][a-z0-9-]{{0,31}}",
                t.name
            ));
        }
        if tasks.iter().any(|x| x.name == t.name) {
            problems.push(format!("{at}: task name {:?} is not unique", t.name));
        }
        if t.start.trim().is_empty() {
            problems.push(format!("{at}: task {:?}: start must name a commit", t.name));
        }
        let description = if inside(&t.description) {
            match std::fs::read_to_string(dir.join(&t.description)) {
                Ok(d) if !d.trim().is_empty() => d,
                Ok(_) => {
                    problems.push(format!("{at}: task {:?}: the description is empty", t.name));
                    String::new()
                }
                Err(e) => {
                    problems.push(format!("{at}: task {:?}: {}: {e}", t.name, t.description));
                    String::new()
                }
            }
        } else {
            problems.push(format!(
                "{at}: task {:?}: description must be a file in the task set",
                t.name
            ));
            String::new()
        };
        let limit = t.time_limit_min.unwrap_or(120);
        if !(1..=480).contains(&limit) {
            problems.push(format!(
                "{at}: task {:?}: time_limit_min {limit}: expected 1..=480",
                t.name
            ));
        }
        if t.tests.is_empty() {
            problems.push(format!("{at}: task {:?}: at least one hidden test", t.name));
        }
        if t.tests.iter().any(|x| x.command.trim().is_empty()) {
            problems.push(format!("{at}: task {:?}: a test command is empty", t.name));
        }
        let hidden = copies(t.hidden, dir, &at, &mut problems);
        // A relative project path is relative to the task set (005 review).
        let project = dir.join(&t.project);
        if !project.is_dir() {
            problems.push(format!(
                "{at}: task {:?}: project {}: not a folder",
                t.name,
                project.display()
            ));
        }
        let fingerprint = super::git::fingerprint(&description).unwrap_or_else(|e| {
            problems.push(format!("{at}: task {:?}: {e}", t.name));
            String::new()
        });
        tasks.push(Task {
            name: t.name,
            project,
            fingerprint,
            start: t.start,
            description,
            several_components: t.several_components,
            time_limit: Duration::from_secs(limit.clamp(1, 480) as u64 * 60),
            hidden,
            tests: t
                .tests
                .into_iter()
                .map(|x| HiddenTest {
                    command: x.command,
                    windows: x.windows,
                    macos: x.macos,
                    linux: x.linux,
                })
                .collect(),
        });
    }
    if problems.is_empty() {
        Ok(TaskSet {
            dir: dir.to_path_buf(),
            setup,
            tasks,
        })
    } else {
        Err(problems)
    }
}

/// The task's fingerprint (R-12): its description, line endings normalised.
pub fn task_fingerprint(t: &Task) -> Result<String, String> {
    if t.fingerprint.is_empty() {
        return super::git::fingerprint(&t.description);
    }
    Ok(t.fingerprint.clone())
}

/// The hidden tests' fingerprint (R-12): each file's content and target path, and the commands.
pub fn hidden_fingerprint(t: &Task) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();
    for h in &t.hidden {
        let text = std::fs::read(&h.from).map_err(|e| format!("{}: {e}", h.from.display()))?;
        let blob = super::git::fingerprint(&String::from_utf8_lossy(&text))?;
        lines.push(format!("file {} {blob}", h.to));
    }
    lines.sort();
    for x in &t.tests {
        lines.push(format!(
            "test {} | {} | {} | {}",
            x.command,
            x.windows.as_deref().unwrap_or(""),
            x.macos.as_deref().unwrap_or(""),
            x.linux.as_deref().unwrap_or("")
        ));
    }
    super::git::fingerprint(&lines.join("\n"))
}

/// What the engineer asked for: a task in a mode, with components switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub set: PathBuf,
    pub task: String,
    pub mode: Mode,
    pub off: BTreeSet<&'static str>,
    pub record: bool,
    /// The agent profile every session of the run uses (the same in every mode).
    pub agent: String,
}

/// A series of runs (FR-018): one at a time, in a shuffled order.
#[derive(Debug, Clone, Default)]
pub struct Series {
    pub queue: Vec<Plan>,
    pub done: usize,
}

impl Series {
    /// Every task × mode × repeat, shuffled with a generator seeded by `seed`.
    pub fn new(
        set: &Path,
        tasks: &[String],
        modes: &[Mode],
        repeats: usize,
        off: &BTreeSet<&'static str>,
        seed: u64,
    ) -> Series {
        let mut queue = Vec::new();
        for t in tasks {
            for m in modes {
                for _ in 0..repeats.max(1) {
                    queue.push(Plan {
                        set: set.to_path_buf(),
                        task: t.clone(),
                        mode: *m,
                        off: off.clone(),
                        record: true,
                        agent: "claude".into(),
                    });
                }
            }
        }
        shuffle(&mut queue, seed);
        Series { queue, done: 0 }
    }

    pub fn take_next(&mut self) -> Option<Plan> {
        if self.queue.is_empty() {
            None
        } else {
            self.done += 1;
            Some(self.queue.remove(0))
        }
    }
}

/// Fisher–Yates with xorshift64*; no random crate needed (R-13).
pub fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        v.swap(i, (r % (i as u64 + 1)) as usize);
    }
}

/// A seed from the clock.
pub fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// Why a run ended (shown, never recorded: R-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Idle,
    TimeLimit,
    Engineer,
}

/// Where a run is (data-model "Run").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// The first session is starting; the description is typed once it is idle.
    Starting,
    Running,
    /// Ended; waiting for the engineer to confirm closing the run's sessions.
    Ended(End),
    /// Sessions are closing; hidden tests start once they all exited.
    Closing(End),
    /// Hidden tests run one after another.
    Testing {
        next: usize,
        running: Option<u32>,
        passed: u32,
        failed: u32,
    },
    Recorded,
    Discarded,
}

/// What Fallow measures per session of a run (FR-014).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measured {
    pub working: Duration,
    pub blocked: Duration,
    pub interventions: u32,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
    pub version: Option<String>,
}

/// One run.
#[derive(Debug, Clone)]
pub struct Run {
    pub id: u64,
    pub plan: Plan,
    pub task: Task,
    pub task_fp: String,
    pub hidden_fp: String,
    /// The run's own repository and the folder holding it and its worktrees.
    pub copy: PathBuf,
    pub root: PathBuf,
    /// The temporary project the copy is registered as.
    pub project: crate::projects::ProjectId,
    pub start_commit: String,
    pub first: Option<u32>,
    pub sessions: BTreeSet<u32>,
    pub started_at: Instant,
    pub started: SystemTime,
    pub ended_at: Option<Instant>,
    pub stage: Stage,
    pub typed: bool,
    pub idle_since: Option<Instant>,
    pub idle_end: Duration,
    pub measured: BTreeMap<u32, Measured>,
    pub last_seen: Option<Instant>,
    pub final_commit: Option<String>,
    /// The session whose folder holds the result: the lead when the run ended (FR-013).
    pub result_session: Option<u32>,
    pub tested_dir: Option<PathBuf>,
    /// The squad as the record needs it, taken before the burst ends.
    pub snapshot: Option<Snapshot>,
}

/// What a record needs from a run's burst (numbers and fixed values come from it later).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Session → role name (turned into numbers in the record).
    pub roles: Vec<(u32, String)>,
    /// Session → part (turned into numbers in the record).
    pub parts: Vec<(u32, Option<String>)>,
    /// Final gate outcomes as fixed values.
    pub gates: Vec<String>,
    /// Verdicts as fixed values.
    pub reviews: Vec<String>,
    /// Repair rounds per part.
    pub repairs: Vec<(Option<String>, u32)>,
    /// Signals raised per session.
    pub signals: Vec<(u32, u32)>,
}

impl Run {
    /// The end rule (FR-013): nothing of the run worked for `idle_end` while nothing is in
    /// progress, or the time limit passed.
    pub fn due_end(&self, now: Instant, working: bool, in_progress: bool) -> Option<End> {
        if !matches!(self.stage, Stage::Running) {
            return None;
        }
        if now.saturating_duration_since(self.started_at) >= self.task.time_limit {
            return Some(End::TimeLimit);
        }
        match self.idle_since {
            Some(t)
                if !working
                    && !in_progress
                    && now.saturating_duration_since(t) >= self.idle_end =>
            {
                Some(End::Idle)
            }
            _ => None,
        }
    }

    /// Updates the idle clock: it starts when nothing works and nothing is in progress.
    pub fn observe_idle(&mut self, now: Instant, working: bool, in_progress: bool) {
        if working || in_progress || !self.typed {
            self.idle_since = None;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(now);
        }
    }

    /// Adds `dt` of work or blocking to a session's measures.
    pub fn add_time(&mut self, session: u32, working: bool, blocked: bool, dt: Duration) {
        let m = self.measured.entry(session).or_default();
        if working {
            m.working += dt;
        }
        if blocked {
            m.blocked += dt;
        }
    }

    pub fn intervention(&mut self, session: u32) {
        self.measured.entry(session).or_default().interventions += 1;
    }

    /// The text typed into the first session: the fixed description and the closing line.
    pub fn description(&self) -> String {
        format!("{}\n\n{CLOSING_LINE}", self.task.description.trim_end())
    }

    pub fn in_progress(&self) -> bool {
        !matches!(self.stage, Stage::Recorded | Stage::Discarded)
    }
}

impl super::Hub {
    /// Whether a session waits on something (FR-014's time blocked): a task for it whose
    /// preconditions are unmet, an open signal it raised, or a start it asked for waiting for
    /// memory.
    pub fn blocked(&self, session: u32) -> bool {
        self.tasks.list.iter().any(|t| {
            t.state == super::tasks::State::Blocked
                && (t.assigned == Some(session)
                    || t.target == super::tasks::Target::Session(session))
        }) || self.signals.open_from(session)
            || self.memory_waits.iter().any(|(id, _, _)| {
                self.starts
                    .iter()
                    .any(|s| s.id == *id && s.from.session() == Some(session))
            })
    }

    /// Whether something of a run is in progress in the hub (FR-013): a start pending or
    /// approved, a gate run, or a hand-over not typed yet into one of its sessions.
    pub fn run_busy(&self, project: crate::projects::ProjectId, sessions: &BTreeSet<u32>) -> bool {
        let starts = self.starts.iter().any(|s| {
            s.project == project
                && matches!(
                    s.state,
                    super::StartState::Pending | super::StartState::Approved
                )
        });
        let gates = self.burst_of(project).is_some_and(|b| {
            b.gate_runs
                .iter()
                .any(|g| g.state == super::gates::RunState::Running)
        });
        let typing = self
            .deliveries
            .list
            .iter()
            .any(|d| sessions.contains(&d.to) && !d.state.finished());
        starts
            || gates
            || typing
            || self.memory_waiting(project)
            || self.gate_memory_waits.contains(&project)
    }

    /// What a run's record needs from its burst, before the burst ends (R-15).
    pub fn run_snapshot(&self, project: crate::projects::ProjectId) -> Option<Snapshot> {
        let b = self.burst_of(project)?;
        let gates = b
            .gate_runs
            .iter()
            .filter(|g| g.kind == super::gates::Kind::Final)
            .map(|g| match (g.start_failed, &g.state, g.class) {
                (true, ..) => "start-failed",
                (_, super::gates::RunState::TimedOut, _) => "timed-out",
                (_, _, Some(super::gates::Class::Pass)) => "pass",
                (_, _, Some(super::gates::Class::BaselineOnly)) => "baseline-only",
                (_, _, Some(super::gates::Class::Regression)) => "regression",
                _ => "failed",
            })
            .map(str::to_string)
            .collect();
        let mut reviews: Vec<String> = b
            .verdicts
            .values()
            .map(|(v, _)| match v.as_str() {
                "approved" => "approved",
                "incomplete" => "incomplete",
                _ => "changes-requested",
            })
            .map(str::to_string)
            .collect();
        reviews.sort();
        let mut roles: Vec<(u32, String)> = b.roles.iter().map(|(s, r)| (*s, r.clone())).collect();
        roles.sort();
        let mut parts: Vec<(u32, Option<String>)> = b
            .assigned
            .iter()
            .map(|(s, (_, p))| (*s, p.clone()))
            .collect();
        parts.sort();
        Some(Snapshot {
            roles,
            parts,
            gates,
            reviews,
            repairs: b
                .repairs
                .iter()
                .map(|l| (l.part.clone(), u32::from(l.rounds)))
                .collect(),
            signals: self.signals.count_by_session(),
        })
    }
}
