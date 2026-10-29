//! The engine: one thread that owns every run (006 research R-1, R-2).
//!
//! Requests from the page and the command line, and the results of agent steps, gates and
//! merges, arrive as messages. Each change is an event, written to the run's log before it is
//! applied; then the stage machine decides what happens next. Agent calls, gates and merges run
//! on threads of their own and report back here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::events::*;
use super::state::RunState;
use crate::agent::{self, Activity};
use crate::config::{Autonomy, RoleChoice, Settings, Squad};
use crate::plan::Plan;
use crate::verify::gates::GateResult;
use crate::verify::git;

/// What a worker thread reports back.
#[derive(Debug)]
pub enum Done {
    Step {
        n: u32,
        outcome: agent::Outcome,
    },
    Gates {
        kind: GateKind,
        part: Option<String>,
        results: Vec<GateResult>,
        /// Fallow could not run the gates (no results are recorded then).
        error: Option<String>,
    },
    Merge(Result<String, String>),
}

/// A request from the page or the command line.
#[derive(Debug)]
pub enum Request {
    List,
    Get {
        id: u64,
    },
    Activity {
        id: u64,
        n: u32,
    },
    /// Waits until the run has an event after `seq` or activity after `activity` (long
    /// polling, research R-6): answered at once if it already has.
    Wait {
        id: u64,
        seq: u64,
        activity: u64,
    },
    Create(Create),
    Say {
        id: u64,
        text: String,
    },
    Approve {
        id: u64,
    },
    Answer {
        id: u64,
        text: String,
    },
    Limits {
        id: u64,
        usd: Option<f64>,
        minutes: Option<u64>,
        repair_cap: Option<u32>,
        /// One more step past the limits.
        once: bool,
    },
    /// Try again after a question Fallow raised itself.
    Retry {
        id: u64,
    },
    Merge {
        id: u64,
    },
    Stop {
        id: u64,
        reason: Option<String>,
    },
}

/// What starting a run takes (contracts/cli.md `fallow run`, web-api.md `POST /api/runs`).
#[derive(Debug, Clone, Default)]
pub struct Create {
    pub repo: PathBuf,
    pub task: String,
    pub target: Option<String>,
    pub autonomy: Option<Autonomy>,
    pub limit_usd: Option<f64>,
    pub limit_min: Option<u64>,
    pub roles: BTreeMap<String, RoleChoice>,
}

#[derive(Debug)]
pub enum Reply {
    Ok(Value),
    Err(String),
}

#[derive(Debug)]
pub enum Msg {
    Request(Request, Sender<Reply>),
    Done { run: u64, done: Done },
    Activity { run: u64, n: u32, line: Activity },
    Tick,
    Shutdown,
}

/// What is running for a run right now (never persisted: a restart finds nothing running).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    Step(u32),
    Gates,
    Merge,
}

/// One run in memory.
pub struct Run {
    pub dir: PathBuf,
    pub log: Log,
    pub state: RunState,
    pub busy: Option<Busy>,
    /// The approved plan, read from the repository when needed.
    pub plan: Option<Plan>,
    /// Ends the running step early (stop, take over).
    pub stop: Arc<AtomicBool>,
    /// Pages waiting for the next change (long polling), with when they started waiting.
    pub waiters: Vec<(Instant, Sender<Reply>)>,
    /// Activity lines seen since Fallow started, so a waiting page knows there are new ones.
    pub activity: u64,
}

impl Run {
    pub fn id(&self) -> u64 {
        self.state.id
    }

    /// Writes an event, applies it, and wakes the pages waiting for a change.
    pub fn append(&mut self, event: Event) -> Result<(), String> {
        let logged = self.log.append(event)?;
        self.state.apply(&logged);
        self.wake();
        Ok(())
    }

    /// Where the run is, for a waiting page.
    pub fn mark(&self) -> Value {
        json!({ "seq": self.state.seq, "activity": self.activity })
    }

    /// Answers every waiting page.
    pub fn wake(&mut self) {
        let mark = self.mark();
        for (_, w) in self.waiters.drain(..) {
            let _ = w.send(Reply::Ok(mark.clone()));
        }
    }

    /// Lets go of pages that stopped waiting (the page gives up after 25 s).
    pub fn prune_waiters(&mut self) {
        self.waiters
            .retain(|(since, _)| since.elapsed() < Duration::from_secs(60));
    }

    pub fn worktrees(&self) -> PathBuf {
        self.dir.join("wt")
    }

    pub fn steps_dir(&self) -> PathBuf {
        self.dir.join("steps")
    }
}

/// What the stage machine needs from around it.
pub struct Ctx {
    pub settings: Settings,
    pub tx: Sender<Msg>,
    /// Fallow's own executable and process id, for the step tools' bridge.
    pub exe: PathBuf,
    pub pid: u32,
}

pub struct Engine {
    pub data: PathBuf,
    pub runs: BTreeMap<u64, Run>,
    pub ctx: Ctx,
    /// Messages for the console ("needs you" lines, problems on resume).
    pub console: Vec<String>,
}

pub fn runs_dir(data: &Path) -> PathBuf {
    data.join("runs")
}

impl Engine {
    /// Loads every run under `data/runs`, resuming those that are not finished (research R-2).
    pub fn load(data: PathBuf, ctx: Ctx) -> Result<Engine, String> {
        crate::platform::private_dir(&data)?;
        crate::platform::private_dir(&runs_dir(&data))?;
        let mut e = Engine {
            data,
            runs: BTreeMap::new(),
            ctx,
            console: Vec::new(),
        };
        let mut ids: Vec<u64> = std::fs::read_dir(runs_dir(&e.data))
            .map_err(|err| err.to_string())?
            .flatten()
            .filter_map(|d| d.file_name().to_str()?.parse().ok())
            .collect();
        ids.sort_unstable();
        for id in ids {
            let dir = runs_dir(&e.data).join(id.to_string());
            match Log::open(&dir).and_then(|(log, events, notes)| {
                RunState::fold(id, &events).map(|s| (log, s, notes))
            }) {
                Ok((log, state, notes)) => {
                    e.console.extend(notes);
                    let mut run = Run {
                        dir,
                        log,
                        state,
                        busy: None,
                        plan: None,
                        stop: Arc::new(AtomicBool::new(false)),
                        waiters: Vec::new(),
                        activity: 0,
                    };
                    if !run.state.stage.is_final() {
                        if let Err(err) = super::machine::resume(&mut run) {
                            e.console.push(format!("run {id}: {err}"));
                        }
                    }
                    e.runs.insert(id, run);
                }
                Err(err) => e
                    .console
                    .push(format!("run {id} could not be loaded: {err}")),
            }
        }
        Ok(e)
    }

    /// Runs the engine until `Shutdown`.
    pub fn serve(mut self, rx: Receiver<Msg>) {
        let ids: Vec<u64> = self.runs.keys().copied().collect();
        for id in ids {
            self.drive(id);
        }
        loop {
            let msg = match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(m) => m,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Msg::Tick,
                Err(_) => break,
            };
            match msg {
                Msg::Request(Request::Wait { id, seq, activity }, reply) => {
                    match self.runs.get_mut(&id) {
                        Some(r) if r.state.seq > seq || r.activity > activity => {
                            let _ = reply.send(Reply::Ok(r.mark()));
                        }
                        Some(r) => r.waiters.push((Instant::now(), reply)),
                        None => {
                            let _ = reply.send(Reply::Err(format!("there is no run {id}")));
                        }
                    }
                }
                Msg::Request(r, reply) => {
                    let out = self.request(r);
                    let _ = reply.send(out);
                }
                Msg::Done { run, done } => {
                    if let Some(r) = self.runs.get_mut(&run) {
                        r.busy = None;
                        if let Err(e) = super::machine::on_done(r, &self.ctx, done) {
                            self.console.push(format!("run {run}: {e}"));
                        }
                    }
                    self.drive(run);
                }
                Msg::Activity { run, .. } => {
                    if let Some(r) = self.runs.get_mut(&run) {
                        r.activity += 1;
                        r.wake();
                    }
                }
                Msg::Tick => {
                    let ids: Vec<u64> = self.runs.keys().copied().collect();
                    for id in ids {
                        if let Some(r) = self.runs.get_mut(&id) {
                            r.prune_waiters();
                        }
                        self.drive(id);
                    }
                }
                Msg::Shutdown => {
                    for r in self.runs.values() {
                        r.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    break;
                }
            }
            for line in self.console.drain(..) {
                eprintln!("{line}");
            }
        }
    }

    fn drive(&mut self, id: u64) {
        if let Some(r) = self.runs.get_mut(&id) {
            let before = needs(r);
            if let Err(e) = super::machine::drive(r, &self.ctx) {
                // Asked once: a run that needs the engineer is not driven again until they act.
                if r.state.needs.is_none() {
                    let _ = r.append(Event::NeedsEngineer {
                        why: format!("{}{e}", super::machine::COULD_NOT_GO_ON),
                        options: vec!["retry".into(), "stop".into()],
                    });
                }
            }
            // A line on the console whenever the run starts needing the engineer (FR-016).
            let after = needs(r);
            if let Some(why) = after.filter(|a| before.as_ref() != Some(a)) {
                self.console.push(format!("run {id} needs you: {why}"));
            }
        }
    }

    fn request(&mut self, r: Request) -> Reply {
        let out: Result<Value, String> = match r {
            Request::List => Ok(Value::Array(
                self.runs.values().rev().map(summary).collect(),
            )),
            Request::Get { id } => run_mut(self, id).map(|r| view(r)),
            Request::Activity { id, n } => run_mut(self, id).map(|r| {
                let text = std::fs::read_to_string(r.steps_dir().join(format!("{n}.jsonl")))
                    .unwrap_or_default();
                Value::Array(
                    text.lines()
                        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                        .collect(),
                )
            }),
            Request::Wait { id, .. } => run_mut(self, id).map(|r| r.mark()),
            Request::Create(c) => self.create(c),
            Request::Say { id, text } => run_mut(self, id)
                .and_then(|r| super::machine::say(r, &text))
                .map(|()| json!({})),
            Request::Approve { id } => run_mut(self, id)
                .and_then(super::machine::approve)
                .map(|()| json!({})),
            Request::Answer { id, text } => run_mut(self, id)
                .and_then(|r| super::machine::answer(r, &text))
                .map(|()| json!({})),
            Request::Limits {
                id,
                usd,
                minutes,
                repair_cap,
                once,
            } => run_mut(self, id)
                .and_then(|r| super::machine::raise_limits(r, usd, minutes, repair_cap, once))
                .map(|()| json!({})),
            Request::Retry { id } => run_mut(self, id)
                .and_then(super::machine::retry)
                .map(|()| json!({})),
            Request::Merge { id } => {
                let ctx_tx = self.ctx.tx.clone();
                run_mut(self, id)
                    .and_then(|r| super::machine::merge(r, ctx_tx))
                    .map(|()| json!({}))
            }
            Request::Stop { id, reason } => run_mut(self, id)
                .and_then(|r| super::machine::stop(r, reason))
                .map(|()| json!({})),
        };
        let id = match &out {
            Ok(v) => v["id"].as_u64(),
            Err(_) => None,
        };
        if let Some(id) = id {
            self.drive(id);
        } else {
            let ids: Vec<u64> = self.runs.keys().copied().collect();
            for id in ids {
                self.drive(id);
            }
        }
        match out {
            Ok(v) => Reply::Ok(v),
            Err(e) => Reply::Err(e),
        }
    }

    /// Starts a run (FR-032 setup check, then `run_created` and the first planning turn).
    fn create(&mut self, c: Create) -> Result<Value, String> {
        if c.task.trim().is_empty() {
            return Err("the task is empty".into());
        }
        let top = git_toplevel(&c.repo)?;
        let current = git::branch(&top);
        let target = match c.target.clone().or_else(|| current.clone()) {
            Some(t) => t,
            None => {
                return Err("the repository is on a detached HEAD: name the target branch".into());
            }
        };
        // The run starts from the target branch, which must be a local branch.
        let start_commit = git::rev(&top, &format!("refs/heads/{target}"))
            .map_err(|_| format!("there is no local branch {target:?} to start from"))?;
        // Uncommitted changes are left out only when they are on the branch the run starts from.
        let left_out = if current.as_deref() == Some(target.as_str()) {
            git::changes(&top).unwrap_or_default()
        } else {
            Vec::new()
        };
        // The squad file at the starting commit (contracts/config.md).
        let mut notices = Vec::new();
        let squad = match git::show(&top, &start_commit, crate::config::squad::SQUAD_FILE) {
            Ok(Some(text)) => Squad::parse(crate::config::squad::SQUAD_FILE, &text, &mut notices),
            _ => Squad::parse(crate::config::squad::SQUAD_FILE, "", &mut notices),
        };
        let s = &self.ctx.settings;
        let mut roles = s.roles.clone();
        for (role, choice) in &squad.roles {
            if let Some(base) = roles.get(role).cloned() {
                roles.insert(role.clone(), base.overlaid(choice));
            }
        }
        for (role, choice) in &c.roles {
            if let Some(base) = roles.get(role).cloned() {
                roles.insert(role.clone(), base.overlaid(choice));
            }
        }
        let mut missing = Vec::new();
        for (role, choice) in &roles {
            match s.agents.get(&choice.agent) {
                None => missing.push(format!(
                    "{role}: no agent named {:?} in the settings",
                    choice.agent
                )),
                Some(a) if crate::platform::find_program(&a.command).is_none() => missing.push(
                    format!("{role}: {} is not installed or not on the path", a.command),
                ),
                _ => {}
            }
        }
        for g in &squad.gates {
            if let Some(p) = g.command.split_whitespace().next() {
                if crate::platform::find_program(p).is_none() {
                    missing.push(format!("gate {}: {p} is not on the path", g.name));
                }
            }
        }
        if !missing.is_empty() {
            return Err(format!("the run cannot start: {}", missing.join("; ")));
        }
        let id = self.next_id();
        let dir = runs_dir(&self.data).join(id.to_string());
        let (mut log, _, _) = Log::open(&dir)?;
        let created = Event::RunCreated {
            repo: top.clone(),
            target,
            start_commit: start_commit.clone(),
            task: c.task.trim().to_string(),
            mode: Mode::Solo,
            autonomy: c.autonomy.or(squad.autonomy).unwrap_or(s.defaults.autonomy),
            limits: Limits {
                usd: c
                    .limit_usd
                    .or(squad.limit_usd)
                    .unwrap_or(s.defaults.limit_usd),
                minutes: c
                    .limit_min
                    .or(squad.limit_min)
                    .unwrap_or(s.defaults.limit_min),
                step_minutes: s.defaults.step_min,
            },
            roles,
            repair_cap: squad.repair_cap.unwrap_or(s.defaults.repair_cap),
            gates: squad
                .gates
                .iter()
                .map(|g| GateSpec {
                    name: g.name.clone(),
                    command: g.command.clone(),
                    timeout_min: g.timeout_min,
                    baseline: g.baseline,
                    failures: match &g.failures {
                        crate::config::Failures::Auto => "auto".into(),
                        crate::config::Failures::ExitCode => "exit-code".into(),
                        crate::config::Failures::Regex(p) => p.clone(),
                    },
                })
                .collect(),
            allow: squad.allow.clone(),
            test_patterns: squad.test_patterns.clone(),
            recorded: false,
            left_out,
        };
        let first = log.append(created)?;
        let state = RunState::fold(id, &[first])?;
        let mut run = Run {
            dir,
            log,
            state,
            busy: None,
            plan: None,
            stop: Arc::new(AtomicBool::new(false)),
            waiters: Vec::new(),
            activity: 0,
        };
        for n in notices {
            run.append(Event::Note {
                text: n.to_string(),
            })?;
        }
        super::machine::start_planning(&mut run)?;
        self.runs.insert(id, run);
        Ok(json!({ "id": id }))
    }

    fn next_id(&self) -> u64 {
        let path = self.data.join("next_id");
        let stored: u64 = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        let id = stored.max(self.runs.keys().next_back().map_or(1, |m| m + 1));
        let _ = std::fs::write(&path, (id + 1).to_string());
        id
    }
}

fn run_mut(e: &mut Engine, id: u64) -> Result<&mut Run, String> {
    e.runs
        .get_mut(&id)
        .ok_or_else(|| format!("there is no run {id}"))
}

fn git_toplevel(dir: &Path) -> Result<PathBuf, String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("{} is not a git repository", dir.display()));
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if git::head(Path::new(&p)).is_err() {
        return Err(format!("{p} has no commit yet"));
    }
    Ok(PathBuf::from(p))
}

/// One line of the runs list.
pub fn summary(r: &Run) -> Value {
    let s = &r.state;
    json!({
        "id": s.id,
        "repo": s.repo,
        "task": s.task,
        "stage": s.stage,
        "needs_engineer": needs(r),
        "cost_usd": s.cost_usd,
        "created": s.created,
        "seq": s.seq,
    })
}

/// What needs the engineer now, if anything.
pub fn needs(r: &Run) -> Option<String> {
    let s = &r.state;
    if let Some(n) = &s.needs {
        return Some(n.why.clone());
    }
    match s.stage {
        Stage::Planning if s.plan_approvable && r.busy.is_none() => {
            Some("The plan is ready to approve".into())
        }
        Stage::Planning if r.busy.is_none() => {
            Some("The planner is waiting for your answer".into())
        }
        Stage::Ready => Some("Ready to merge".into()),
        _ => None,
    }
}

/// The whole run for the page.
pub fn view(r: &Run) -> Value {
    let mut v = serde_json::to_value(&r.state).unwrap_or(Value::Null);
    v["needs_engineer"] = json!(needs(r));
    v["busy"] = json!(match &r.busy {
        Some(Busy::Step(n)) => format!("step {n}"),
        Some(Busy::Gates) => "gates".into(),
        Some(Busy::Merge) => "merge".into(),
        None => String::new(),
    });
    v["interventions"] = json!(r.state.interventions());
    v["minutes_used"] = json!(r.state.minutes_used(super::time::unix_now()));
    v["plan"] = serde_json::to_value(&r.plan).unwrap_or(Value::Null);
    v
}
