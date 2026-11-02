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
use crate::config::{Settings, Squad};
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
    /// 007: the dashboard's summary (contracts/web-api.md `GET /api/dashboard`).
    Dashboard,
    /// 007: waits for a change in a scope (`global`, `loop:<id>`, `conv:<id>`) after `after`
    /// (research S-4): answered at once if there is one.
    Events {
        scope: String,
        after: u64,
    },
    /// 007: projects, conversations, features and documents (US1).
    Studio(crate::studio::Req),
    /// 007: a tool call from an agent's MCP bridge; answered when it can be (an approval waits
    /// for the engineer).
    ToolCall {
        token: String,
        name: String,
        args: Value,
    },
    /// 007: locks a feature and creates its loop (FR-008).
    Lock {
        project: String,
        feature: String,
        pipeline: Option<String>,
        level: Option<Level>,
        budget_usd: Option<f64>,
        budget_minutes: Option<u64>,
    },
    /// 007: a loop's screen, its feed, and one task's story (US3).
    Loop {
        id: u64,
    },
    LoopActivity {
        id: u64,
        filter: super::activity::Filter,
    },
    Task {
        id: u64,
        task: String,
    },
    /// 007: the engineer's pause, "go on", and note for the next step (FR-019).
    Pause {
        id: u64,
    },
    GoOn {
        id: u64,
    },
    Note {
        id: u64,
        text: String,
        agent: Option<String>,
    },
    /// 007: unlocks a locked feature; its loop halts after the current step (FR-009).
    Unlock {
        project: String,
        feature: String,
    },
    /// 007: the stories a new lock would reopen, from the working copy (research S-17).
    RelockPreview {
        project: String,
        feature: String,
    },
    /// 007: the engineer answers a question relayed to them, or overrules the lead's answer
    /// (FR-021, FR-022).
    QuestionAnswer {
        id: u64,
        q: String,
        answer: String,
    },
    Overrule {
        id: u64,
        q: String,
        answer: String,
    },
    /// 007: the engineer confirms a decision of the lead (FR-020a).
    QuestionConfirm {
        id: u64,
        q: String,
    },
    /// 007: what Fallow keeps on disk, and deleting a loop's or conversation's data (FR-003b).
    Data,
    DeleteData {
        kind: String,
        id: String,
    },
    /// 008 T038: deletes a spec session; before the lock, its draft goes too.
    DeleteFeature {
        project: String,
        feature: String,
    },
}

#[derive(Debug)]
pub enum Reply {
    Ok(Value),
    Err(String),
}

#[derive(Debug)]
pub enum Msg {
    Request(Request, Sender<Reply>),
    Done {
        run: u64,
        done: Done,
    },
    Activity {
        run: u64,
        n: u32,
        line: Activity,
    },
    /// 007: a turn of the lead reports.
    Studio(crate::studio::Note),
    /// 007: the lead's call for an agent's question ended (research S-10).
    LeadAnswer {
        run: u64,
        q: String,
        outcome: agent::Outcome,
    },
    Tick,
    Shutdown,
    /// 010: a project's protection, as the hosting tool read it (research R-3).
    Remote {
        project: String,
        read: crate::publish::Read,
    },
}

/// What Fallow itself runs for a run right now: its gates or a merge (never persisted: a restart
/// finds nothing running). Agent steps are in `Run::running`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    Gates,
    Merge,
}

/// An agent step running now (008 US5: checkers marked `parallel_with` run at the same time).
pub struct Running {
    /// Ends this step early (stop, shutdown). An unlock lets it finish and halts after it.
    pub stop: Arc<AtomicBool>,
    /// Its process group, sampled for the memory budget (research S-19).
    pub group: Arc<crate::platform::job::Group>,
    /// Its MCP tools, and its `ask` waiting for the lead (research S-10).
    pub tool: super::questions::StepTool,
    pub asking: Option<super::questions::Asking>,
}

/// One run in memory.
pub struct Run {
    pub dir: PathBuf,
    pub log: Log,
    pub state: RunState,
    pub busy: Option<Busy>,
    /// Ends Fallow's own gates early (stop).
    pub stop: Arc<AtomicBool>,
    /// The agent steps running now, by number.
    pub running: BTreeMap<u32, Running>,
    /// Pages waiting for the next change (long polling), with when they started waiting.
    pub waiters: Vec<(Instant, Sender<Reply>)>,
    /// Activity lines seen since Fallow started, so a waiting page knows there are new ones.
    pub activity: u64,
    /// Why the next step has not started: no free slot, or not enough memory.
    pub waiting: Option<String>,
}

impl Run {
    /// Something runs: an agent step, the gates or a merge.
    pub fn is_busy(&self) -> bool {
        self.busy.is_some() || !self.running.is_empty()
    }

    /// Signals every running step, and the gates, to end now.
    pub fn stop_all(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for x in self.running.values() {
            x.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(a) = &x.asking {
                a.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

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

    /// Where the run is, for a waiting page: one number that grows with every event and every
    /// activity line (`seq`), and both parts.
    pub fn mark(&self) -> Value {
        json!({
            "seq": self.state.seq + self.activity,
            "event_seq": self.state.seq,
            "activity": self.activity,
        })
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
/// What a step expects to use when it has no sample yet, in MB (005's session size).
pub const STEP_MB: i64 = 700;

/// The free agent slots and memory room across loops and conversations (research S-19). The
/// engine sets them before it drives a run; a step takes its share as it starts.
#[derive(Debug)]
pub struct Slots {
    free: std::sync::atomic::AtomicI64,
    room_mb: std::sync::atomic::AtomicI64,
}

impl Default for Slots {
    fn default() -> Self {
        Slots {
            free: std::sync::atomic::AtomicI64::new(i64::MAX),
            room_mb: std::sync::atomic::AtomicI64::new(i64::MAX),
        }
    }
}

impl Slots {
    pub fn set(&self, free: i64, room_mb: i64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.free.store(free, Relaxed);
        self.room_mb.store(room_mb, Relaxed);
    }

    /// Takes a slot for a step about to start, or says why it waits.
    pub fn take(&self) -> Result<(), String> {
        use std::sync::atomic::Ordering::Relaxed;
        if self.free.load(Relaxed) <= 0 {
            return Err("waiting for a slot: the most agents allowed at once are running".into());
        }
        if self.room_mb.load(Relaxed) < STEP_MB {
            return Err("waiting for a slot: not enough memory for another agent".into());
        }
        self.free.fetch_sub(1, Relaxed);
        self.room_mb.fetch_sub(STEP_MB, Relaxed);
        Ok(())
    }
}

pub struct Ctx {
    /// 007: agent slots shared by every loop and conversation (research S-19).
    pub slots: Arc<Slots>,
    pub settings: Settings,
    pub tx: Sender<Msg>,
    /// Fallow's own executable and process id, for the step tools' bridge.
    pub exe: PathBuf,
    pub pid: u32,
    /// 007: the engineer's configuration folder (`~/.fallow`), for definitions.
    pub config: Option<PathBuf>,
    /// 007: the suffix of the pipe the step tools' bridge connects to (tests have their own).
    pub endpoint: Option<String>,
    /// 007: where desktop notifications go, their switch, and the page's address with its
    /// secret (research S-14).
    pub notify: Arc<dyn crate::platform::notify::Notify>,
    pub notify_on: Arc<AtomicBool>,
    pub page: Option<String>,
}

pub struct Engine {
    pub data: PathBuf,
    pub runs: BTreeMap<u64, Run>,
    pub ctx: Ctx,
    /// Messages for the console ("needs you" lines, problems on resume).
    pub console: Vec<String>,
    /// 007: the page's global scope (research S-4).
    pub bus: crate::service::bus::Bus,
    /// 007: projects, conversations and features.
    pub studio: crate::studio::Studio,
    /// What the bus last said about each run: its event number and what it needed.
    published: BTreeMap<u64, (u64, Option<String>)>,
    /// 007: "Needs you" items become notifications (FR-003a).
    notifier: crate::service::notify::Notifier,
    /// The ended loops whose record was written or tried: each is tried once per start.
    recorded: std::collections::BTreeSet<u64>,
    /// 010: each project's remote as last read, and when a read was last started.
    remotes: BTreeMap<String, (crate::publish::Remote, Option<Instant>)>,
}

/// 010: how often a project's protection is read again: a known value once a minute, an unknown
/// or stale one sooner (research R-6; FR-003).
const REMOTE_KNOWN: Duration = Duration::from_secs(60);
const REMOTE_UNKNOWN: Duration = Duration::from_secs(10);

pub fn runs_dir(data: &Path) -> PathBuf {
    data.join("runs")
}

impl Engine {
    /// Loads every run under `data/runs`, resuming those that are not finished (research R-2).
    pub fn load(data: PathBuf, ctx: Ctx) -> Result<Engine, String> {
        crate::platform::private_dir(&data)?;
        crate::platform::private_dir(&runs_dir(&data))?;
        let studio = crate::studio::Studio::load(&data, ctx.config.clone(), ctx.settings.clone())?;
        let notifier = crate::service::notify::Notifier::new(
            Arc::clone(&ctx.notify),
            Arc::clone(&ctx.notify_on),
            ctx.page.clone(),
            Some(data.join("notified.json")),
        );
        let mut e = Engine {
            notifier,
            recorded: std::collections::BTreeSet::new(),
            remotes: BTreeMap::new(),
            data,
            runs: BTreeMap::new(),
            ctx,
            console: Vec::new(),
            bus: crate::service::bus::Bus::default(),
            studio,
            published: BTreeMap::new(),
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
                    let run = Run {
                        dir,
                        log,
                        state,
                        busy: None,
                        stop: Arc::new(AtomicBool::new(false)),
                        waiters: Vec::new(),
                        activity: 0,
                        running: BTreeMap::new(),
                        waiting: None,
                    };
                    e.runs.insert(id, run);
                }
                Err(err) => e
                    .console
                    .push(format!("run {id} could not be loaded: {err}")),
            }
        }
        // What needed the engineer when Fallow stopped was raised then. On the first start
        // there is no record: what is there now is taken as raised.
        if !e.notifier.load() {
            let items = e.needs_items();
            e.notifier.seed(&items);
        }
        let ids: Vec<u64> = e.runs.keys().copied().collect();
        for id in ids {
            let run = e.runs.get_mut(&id).expect("loaded");
            if !run.state.stage.is_final() {
                if let Err(err) =
                    super::machine::resume(run).and_then(|()| super::questions::orphans(run))
                {
                    e.console.push(format!("run {id}: {err}"));
                }
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
        let mut last_tick = Instant::now();
        loop {
            // The tick comes every second even while messages keep arriving (found in 007's
            // tests: a page polling often starved the time-outs and the driving of runs).
            let wait = Duration::from_secs(1).saturating_sub(last_tick.elapsed());
            let msg = if wait.is_zero() {
                Msg::Tick
            } else {
                match rx.recv_timeout(wait) {
                    Ok(m) => m,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Msg::Tick,
                    Err(_) => break,
                }
            };
            if matches!(msg, Msg::Tick) {
                last_tick = Instant::now();
            }
            match msg {
                Msg::Request(Request::Events { scope, after }, reply) => {
                    self.events(&scope, after, reply);
                }
                Msg::Request(Request::ToolCall { token, name, args }, reply) => {
                    match self.run_of_token(&token) {
                        Some(id) => {
                            let lead = self.lead_of(id);
                            if let Some(r) = self.runs.get_mut(&id) {
                                super::questions::call(
                                    r, &self.ctx, lead, &token, &name, args, reply,
                                );
                            }
                        }
                        None => {
                            self.studio.tool_call(&token, &name, args, reply);
                            self.bus.publish(json!({ "type": "studio" }));
                        }
                    }
                }
                Msg::LeadAnswer { run, q, outcome } => {
                    let lead = self.lead_of(run).ok();
                    if let Some(r) = self.runs.get_mut(&run) {
                        if let Err(e) =
                            super::questions::lead_done(r, lead.as_ref(), &q, Ok(outcome))
                        {
                            self.console.push(format!("run {run}: {e}"));
                        }
                    }
                    self.drive(run);
                }
                Msg::Studio(note) => {
                    let done = matches!(note, crate::studio::Note::TurnDone { .. });
                    self.studio.note(note);
                    if done {
                        self.bus.publish(json!({ "type": "studio" }));
                    }
                }
                Msg::Request(r, reply) => {
                    let out = self.request(r);
                    let _ = reply.send(out);
                }
                Msg::Done { run, done } => {
                    if let Some(r) = self.runs.get_mut(&run) {
                        // A step's tools end with it; a question it asked is still answered.
                        match &done {
                            Done::Step { n, .. } => {
                                r.running.remove(n);
                            }
                            _ => r.busy = None,
                        }
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
                    self.bus.prune();
                    self.studio.tick();
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
                        r.stop_all();
                    }
                    break;
                }
                Msg::Remote { project, read } => self.remote_read(&project, read),
            }
            self.publish_changes();
            self.notify_changes();
            self.record_ended();
            self.console.append(&mut self.studio.console);
            for line in self.console.drain(..) {
                eprintln!("{line}");
            }
        }
    }

    /// What Fallow keeps on disk, per project: its loops, conversations and worktrees, with
    /// their sizes (FR-003b).
    fn data_view(&self) -> Value {
        use crate::project::data::size;
        let mut total = 0u64;
        let mut projects: Vec<Value> = Vec::new();
        let mut of = |id: Option<&str>, name: &str| {
            let loops: Vec<Value> = self
                .runs
                .values()
                .filter(|r| r.state.feature_loop.as_ref().map(|l| l.project.as_str()) == id)
                .map(|r| {
                    let bytes = size(&r.dir);
                    json!({
                        "id": r.id(), "feature": r.state.feature_loop.as_ref().map(|l| l.feature.clone()),
                        "task": r.state.task, "stage": r.state.stage, "running": !r.state.stage.is_final(),
                        "bytes": bytes, "worktrees": size(&r.worktrees()),
                    })
                })
                .collect();
            let convs: Vec<Value> = self
                .studio
                .convs
                .values()
                .filter(|c| Some(c.state.project.as_str()) == id)
                .map(|c| {
                    json!({
                        "id": c.state.id, "kind": c.state.kind, "feature": c.state.feature,
                        "busy": c.state.busy, "turns": c.state.turns.len(), "bytes": size(&c.dir),
                    })
                })
                .collect();
            let worktrees = id.map_or(0, |p| size(&self.studio.data.join("worktrees").join(p)));
            let sum: u64 = loops
                .iter()
                .chain(&convs)
                .filter_map(|v| v["bytes"].as_u64())
                .sum::<u64>()
                + worktrees;
            total += sum;
            json!({ "id": id, "name": name, "loops": loops, "conversations": convs, "worktrees": worktrees, "bytes": sum })
        };
        for p in &self.studio.store.projects {
            projects.push(of(Some(&p.id), &p.name));
        }
        // Runs of 006's single-task flow, outside any project.
        let other = of(None, "Runs outside a project");
        if other["loops"].as_array().is_some_and(|a| !a.is_empty()) {
            projects.push(other);
        }
        // The run records (numbers only): shown, and kept until the engineer deletes them.
        let records = crate::measure::record::list(&self.data).len();
        let rbytes = crate::project::data::size(&crate::measure::record::dir(&self.data));
        json!({ "projects": projects, "bytes": total + rbytes, "folder": self.data,
                "records": { "count": records, "bytes": rbytes } })
    }

    /// Deletes a loop's or a conversation's data, after the page's confirmation naming it
    /// (FR-003b). A loop still running, and a turn in progress, are refused.
    fn delete_data(&mut self, kind: &str, id: &str) -> Result<Value, String> {
        match kind {
            "loop" => {
                let n: u64 = id.parse().map_err(|_| format!("no loop {id}"))?;
                let r = self
                    .runs
                    .get(&n)
                    .ok_or_else(|| format!("there is no loop {n}"))?;
                if !r.state.stage.is_final() {
                    return Err("the loop is still running: stop it first".into());
                }
                // Stopped, its last step may still be ending, with its files open.
                if r.is_busy() {
                    return Err(
                        "the loop's last step is still ending: try again in a moment".into(),
                    );
                }
                let merged = r.state.merged.is_some();
                let repo = r.state.repo.clone();
                let dir = r.dir.clone();
                crate::project::data::remove(&dir, Some(&repo))?;
                self.runs.remove(&n);
                self.published.remove(&n);
                for p in self.studio.store.projects.iter_mut() {
                    for f in p.features.iter_mut().filter(|f| f.run == Some(n)) {
                        if merged {
                            f.ended = Some("merged".into());
                        }
                    }
                }
                self.studio.store.save()?;
                self.bus
                    .publish(json!({ "type": "loop_progress", "run": n }));
                Ok(json!({ "deleted": dir }))
            }
            "conv" => {
                let c = self
                    .studio
                    .convs
                    .get(id)
                    .ok_or_else(|| format!("there is no conversation {id}"))?;
                if c.state.busy {
                    return Err(
                        "the lead is answering in this conversation: wait for its turn to end"
                            .into(),
                    );
                }
                // The spec session of a feature not merged yet is what the lead answers the
                // loop's questions from (research S-10).
                if let Some(f) = c.state.feature.as_deref() {
                    let open = self
                        .studio
                        .store
                        .get(&c.state.project)
                        .and_then(|p| p.features.iter().find(|x| x.name() == f))
                        .is_some_and(|x| {
                            !(x.ended.is_some()
                                || x.run
                                    .and_then(|r| self.runs.get(&r))
                                    .is_some_and(|r| r.state.merged.is_some()))
                        });
                    if open {
                        return Err(format!(
                            "this is the spec session of {f}, which is not merged yet: the lead answers its loop's questions from it"
                        ));
                    }
                }
                let (dir, project) = self.remove_conv(id)?;
                self.studio.store.save()?;
                self.bus
                    .publish(json!({ "type": "project", "project": project }));
                Ok(json!({ "deleted": dir }))
            }
            _ => Err("kind: loop or conv".into()),
        }
    }

    /// Removes a conversation's data and its worktree, when Fallow made one (FR-003b), and every
    /// reference to it; the caller saves the store. Returns its folder and its project.
    fn remove_conv(&mut self, id: &str) -> Result<(PathBuf, String), String> {
        let c = self
            .studio
            .convs
            .get(id)
            .ok_or_else(|| format!("there is no conversation {id}"))?;
        let dir = c.dir.clone();
        let project = c.state.project.clone();
        let wt = c.state.workdir.clone();
        if wt.starts_with(self.data.join("worktrees")) {
            if let Some(p) = self.studio.store.get(&project) {
                let _ = git::worktree_remove(&p.path, &wt);
            }
            if wt.exists() {
                std::fs::remove_dir_all(&wt).map_err(|e| format!("{}: {e}", wt.display()))?;
            }
        }
        crate::project::data::remove(&dir, None)?;
        self.studio.convs.remove(id);
        if let Some(p) = self.studio.store.get_mut(&project) {
            if p.conversation.as_deref() == Some(id) {
                p.conversation = None;
            }
            for f in p
                .features
                .iter_mut()
                .filter(|f| f.conversation.as_deref() == Some(id))
            {
                f.conversation = None;
            }
        }
        Ok((dir, project))
    }

    /// Deletes a spec session (008 T038). Before the lock, its draft goes too: the chat, the
    /// feature's worktree and branch (or, in a plain folder, its `specs/NNN` folder), and the
    /// feature leaves the project. After a lock, only the chat goes; the documents stay where
    /// the loop and the merge put them. Refused while the lead answers or a loop still goes.
    fn delete_feature(&mut self, project: &str, feature: &str) -> Result<Value, String> {
        let p = self
            .studio
            .store
            .get(project)
            .ok_or_else(|| format!("there is no project {project}"))?;
        let f = p
            .features
            .iter()
            .find(|f| f.name() == feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?
            .clone();
        let repo = p.path.clone();
        let plain = p.kind() != crate::project::Kind::Git;
        if let Some(r) = f.run.and_then(|n| self.runs.get(&n)) {
            if !r.state.stage.is_final() {
                return Err(format!(
                    "the loop of {feature} is still going: stop it before deleting its spec session"
                ));
            }
        }
        if let Some(c) = f
            .conversation
            .as_deref()
            .and_then(|c| self.studio.convs.get(c))
        {
            if c.state.busy {
                return Err(
                    "the lead is answering in this session: wait for its turn to end".into(),
                );
            }
        }
        // The branch must be deletable before anything goes, so a delete never stops half-way.
        if f.locks.is_empty() && !plain {
            if let Some(b) = &f.branch {
                if git::branch(&repo).as_deref() == Some(b.as_str()) {
                    return Err(format!(
                        "{b} is checked out in {}: check out another branch there first",
                        repo.display()
                    ));
                }
            }
        }
        let mut removed: Vec<String> = Vec::new();
        if let Some(c) = f.conversation.as_deref() {
            if self.studio.convs.contains_key(c) {
                self.remove_conv(c)?;
                removed.push("the chat".into());
            }
        }
        if f.locks.is_empty() {
            // The draft: never locked, so nothing of it was built or merged.
            if let Some(b) = &f.branch {
                if !plain {
                    // The worktree's folder is gone; git forgets it, then lets the branch go.
                    git::worktree_prune(&repo);
                    if let Err(e) = git::delete_branch(&repo, b) {
                        self.studio.store.save()?;
                        return Err(format!(
                            "the chat is deleted, but the branch {b} could not be: {e}"
                        ));
                    }
                    removed.push(format!("the branch {b}"));
                }
            }
            if plain {
                let d = repo.join(&f.dir);
                if d.exists() {
                    std::fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
                    removed.push(f.dir.clone());
                }
            }
            if let Some(p) = self.studio.store.get_mut(project) {
                p.features.retain(|x| x.name() != feature);
            }
        } else if let Some(x) = self
            .studio
            .store
            .get_mut(project)
            .and_then(|p| p.feature_mut(feature))
        {
            // Unlocked, then left: with its session gone it is its last loop's again, not a spec
            // session without a chat.
            x.unlocked = false;
        }
        self.studio.store.save()?;
        self.bus
            .publish(json!({ "type": "project", "project": project }));
        Ok(json!({ "deleted": removed, "feature_removed": f.locks.is_empty() }))
    }

    /// The "Needs you" items of every loop (FR-003a), newest first.
    fn needs_items(&self) -> Vec<Value> {
        let name = |p: &str| {
            self.studio
                .store
                .get(p)
                .map(|x| x.name.clone())
                .unwrap_or_else(|| p.to_string())
        };
        let mut items: Vec<Value> = self
            .runs
            .values()
            .flat_map(|r| match r.state.feature_loop.as_ref() {
                Some(l) => super::dashboard::needs(r, &name(&l.project)),
                // A run of 006's single-task flow.
                None => needs(r)
                    .map(|why| {
                        json!({ "kind": "stopped", "key": format!("{}:needs:{why}", r.id()), "run": r.id(),
                                "text": why, "task": r.state.task, "at": r.state.stage_at })
                    })
                    .into_iter()
                    .collect(),
            })
            .collect();
        items.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
        items
    }

    /// Writes the record of each loop that has ended, once (research S-20). A record that
    /// cannot be written is said on the console; the loop is not affected.
    fn record_ended(&mut self) {
        for (id, r) in &self.runs {
            if !r.state.stage.is_final()
                || r.state.feature_loop.is_none()
                || !self.recorded.insert(*id)
            {
                continue;
            }
            if crate::measure::record::path(&self.data, *id).exists() {
                continue;
            }
            let lib = crate::defs::Library::load(self.ctx.config.as_deref(), Some(&r.state.repo));
            let runtime_of = |def: &str| lib.agent(def).map(|d| d.runtime.clone());
            if let Some(rec) = crate::measure::record::LoopRecord::of(&r.state, &runtime_of) {
                if let Err(e) = crate::measure::record::write(&self.data, *id, &rec) {
                    self.console.push(format!("loop {id}: no run record: {e}"));
                }
            }
        }
    }

    /// New "Needs you" items raise a notification each, unless the project turned them off.
    fn notify_changes(&mut self) {
        let items = self.needs_items();
        let store = &self.studio.store;
        self.notifier
            .update(&items, |p| store.get(p).is_none_or(|x| x.notifications));
    }

    /// Tells the global scope which loops changed and what started or stopped needing the
    /// engineer.
    fn publish_changes(&mut self) {
        let mut out = Vec::new();
        for (id, r) in &self.runs {
            let now = (r.state.seq, needs(r));
            let before = self.published.get(id).cloned();
            if before.as_ref() == Some(&now) {
                continue;
            }
            if before.as_ref().map(|b| b.0) != Some(now.0) {
                out.push(json!({ "type": "loop_progress", "run": id }));
            }
            let was = before.and_then(|b| b.1);
            match (&was, &now.1) {
                (None, Some(why)) => {
                    out.push(json!({ "type": "needs_you", "run": id, "why": why }))
                }
                (Some(_), None) => out.push(json!({ "type": "needs_you_cleared", "run": id })),
                (Some(a), Some(b)) if a != b => {
                    out.push(json!({ "type": "needs_you", "run": id, "why": b }))
                }
                _ => {}
            }
            self.published.insert(*id, now);
        }
        for e in out {
            self.bus.publish(e);
        }
    }

    /// Long polling by scope (research S-4).
    fn events(&mut self, scope: &str, after: u64, reply: Sender<Reply>) {
        if scope == "global" {
            return self.bus.wait(after, reply);
        }
        if let Some(id) = scope.strip_prefix("conv:") {
            match self.studio.convs.get_mut(id) {
                Some(c) => {
                    let mark = c.mark();
                    if mark != after {
                        let _ =
                            reply.send(Reply::Ok(json!({ "seq": mark, "reset": mark < after })));
                    } else {
                        c.waiters.push((Instant::now(), reply));
                    }
                }
                None => {
                    let _ = reply.send(Reply::Err(format!("there is no conversation {id}")));
                }
            }
            return;
        }
        if let Some(id) = scope
            .strip_prefix("loop:")
            .and_then(|s| s.parse::<u64>().ok())
        {
            // A loop's mark is its event number plus the activity lines since Fallow started.
            match self.runs.get_mut(&id) {
                Some(r) => {
                    let mark = r.state.seq + r.activity;
                    if mark != after {
                        let _ =
                            reply.send(Reply::Ok(json!({ "seq": mark, "reset": mark < after })));
                    } else {
                        r.waiters.push((Instant::now(), reply));
                    }
                }
                None => {
                    let _ = reply.send(Reply::Err(format!("there is no loop {id}")));
                }
            }
            return;
        }
        let _ = reply.send(Reply::Err(format!("no such scope {scope:?}")));
    }

    /// A feature's stage, from its locks and its loop (data-model.md, Feature).
    fn stage(&self, f: &crate::project::Feature) -> &'static str {
        if f.locks.is_empty() || f.unlocked {
            return "spec_session";
        }
        let Some(r) = f.run.and_then(|id| self.runs.get(&id)) else {
            // The loop's data was deleted (FR-003b).
            return if f.ended.as_deref() == Some("merged") {
                "merged"
            } else {
                "locked"
            };
        };
        if r.state.merged.is_some() {
            return "merged";
        }
        // Stopped by the engineer: whatever it had reached, it is not building nor mergeable.
        if r.state.stage == Stage::Stopped {
            return "stopped";
        }
        match r
            .state
            .feature_loop
            .as_ref()
            .and_then(|l| l.outcome.as_ref())
        {
            Some((true, _)) => "ready_to_merge",
            Some((false, _)) => "done_unverified",
            None => "building",
        }
    }

    fn feature_summary(&self, p: &crate::project::Project, f: &crate::project::Feature) -> Value {
        let run = f.run.and_then(|id| self.runs.get(&id));
        let now = super::time::unix_now();
        json!({
            // A loop that ended counts its time until then.
            "minutes": run.map(|r| {
                let at = if r.state.stage.is_final() {
                    super::time::parse_rfc3339(&r.state.stage_at).unwrap_or(now)
                } else {
                    now
                };
                r.state.minutes_used(at)
            }),
            "cost_usd": run.map(|r| r.state.cost_usd),
            "project": p.id,
            "project_name": p.name,
            "name": f.name(),
            "dir": f.dir,
            "title": f.title,
            "stage": self.stage(f),
            "step": f.step,
            "run": f.run,
            "conversation": f.conversation,
            // Deleting its spec session keeps the documents once it was locked (008 T038).
            "locked_before": !f.locks.is_empty(),
            "plain": p.kind() != crate::project::Kind::Git,
        })
    }

    fn project_summary(&self, p: &crate::project::Project) -> Value {
        json!({
            "id": p.id,
            "name": p.name,
            "path": p.path,
            "kind": p.kind(),
            "spec_kit": crate::project::install::version_in(&p.path),
            "features": p.features.len(),
            "conversation": p.conversation,
        })
    }

    /// A project as its screen shows it (mockup screen 5).
    /// 010: the project's remote (FR-001 to FR-003): its git part at once, its protection as last
    /// read; a new read is started on a thread when the last is old enough. `None` when the
    /// project is not on a branch of a git repository.
    pub fn remote_of(&mut self, p: &crate::project::Project) -> Option<crate::publish::Remote> {
        let target = git::branch(&p.path)?;
        let now = Instant::now();
        let slot = self
            .remotes
            .entry(p.id.clone())
            .or_insert_with(|| (p.remote_seen.clone().unwrap_or_else(|| crate::publish::remote::find(&p.path, &target)), None));
        let wait = if slot.0.known() && !slot.0.stale {
            REMOTE_KNOWN
        } else {
            REMOTE_UNKNOWN
        };
        let due = !slot.0.checking && slot.1.is_none_or(|t| now.duration_since(t) >= wait);
        if due {
            slot.1 = Some(now);
            let mut found = crate::publish::remote::find(&p.path, &target);
            // The last value stays shown while it is read again, for the same remote branch.
            if found.same_place(&slot.0) {
                found.protected = slot.0.protected.clone();
                found.account = slot.0.account.clone();
                found.checked_at = slot.0.checked_at.clone();
                found.stale = slot.0.stale;
                found.why = slot.0.why.clone();
            }
            if found.state == "known" {
                found.checking = true;
                if !found.known() {
                    found.protected = "checking".into();
                }
                let (tx, dir, settings, id, ask) = (
                    self.ctx.tx.clone(),
                    p.path.clone(),
                    self.ctx.settings.clone(),
                    p.id.clone(),
                    found.clone(),
                );
                let _ = std::thread::Builder::new()
                    .name(format!("remote-{id}"))
                    .spawn(move || {
                        let read = crate::publish::read_protection(&dir, &ask, &settings);
                        let _ = tx.send(Msg::Remote { project: id, read });
                    });
            }
            slot.0 = found;
            let seen = slot.0.clone();
            if !seen.checking {
                self.remember_remote(&p.id, seen);
            }
        }
        self.remotes.get(&p.id).map(|s| s.0.clone())
    }

    /// A read of the protection came back: kept, shown, and remembered for a restart.
    fn remote_read(&mut self, project: &str, read: crate::publish::Read) {
        let Some(slot) = self.remotes.get_mut(project) else {
            return;
        };
        slot.0.apply(read, super::time::rfc3339(super::time::unix_now()));
        let seen = slot.0.clone();
        self.remember_remote(project, seen);
        self.bus.publish(json!({ "type": "studio" }));
    }

    fn remember_remote(&mut self, project: &str, seen: crate::publish::Remote) {
        if let Some(p) = self.studio.store.get_mut(project) {
            if p.remote_seen.as_ref() != Some(&seen) {
                p.remote_seen = Some(seen);
                if let Err(e) = self.studio.store.save() {
                    self.console.push(format!("project {project}: {e}"));
                }
            }
        }
    }

    fn project_view(&mut self, id: &str) -> Result<Value, String> {
        let p = self
            .studio
            .store
            .get(id)
            .cloned()
            .ok_or_else(|| format!("there is no project {id}"))?;
        // Reading creates nothing: the page asks for the conversation with a POST.
        let conv = p.conversation.clone().unwrap_or_default();
        let mut v = self.project_summary(&p);
        v["pipeline"] = json!(p.pipeline);
        v["autonomy"] = json!(p.level);
        v["approved_commands"] = json!(p.approved_commands);
        v["budget_per_task"] = json!(p.budget_per_task);
        v["notifications"] = json!(p.notifications);
        // The project's lead (009): the default, and why it is not used if it cannot lead.
        let lead = p.lead.clone().unwrap_or_else(|| "lead".into());
        v["lead"] = match p
            .lead
            .as_deref()
            .and_then(|l| self.studio.lead_problem(&p.path, l))
        {
            Some(why) => {
                json!({ "name": lead, "problem": format!("{why}; new sessions use lead") })
            }
            None => json!({ "name": lead }),
        };
        v["branch"] = json!(git::branch(&p.path));
        v["remote"] = json!(self.remote_of(&p));
        v["feature_list"] = Value::Array(
            p.features
                .iter()
                .map(|f| self.feature_summary(&p, f))
                .collect(),
        );
        v["other_specs"] = json!(
            p.spec_dirs()
                .into_iter()
                .filter(|d| !p.features.iter().any(|f| &f.dir == d))
                .collect::<Vec<_>>()
        );
        let lib = self.studio.library(Some(&p.path));
        v["agent_overrides"] = json!(
            lib.agents
                .values()
                .filter(|a| a.source == crate::defs::Source::Project)
                .count()
        );
        v["conversation"] = self.studio.conv_view(&conv).unwrap_or(Value::Null);
        Ok(v)
    }

    // ---- 009 US5: the project's files ---------------------------------------------------------

    /// The versions of a project's files: the folder, then each feature's live copy (research R-8).
    fn file_versions(&self, project: &str) -> Result<Vec<crate::studio::files::Version>, String> {
        self.versions_of(project, None)
    }

    /// The folder, and each feature's version, or only `only`'s (a feature's name): a file view
    /// asks for one version every few seconds, and resolving every feature each time costs git
    /// calls on the engine's thread.
    fn versions_of(
        &self,
        project: &str,
        only: Option<&str>,
    ) -> Result<Vec<crate::studio::files::Version>, String> {
        use crate::studio::files::{self, FeatureCopy};
        let p = self
            .studio
            .store
            .get(project)
            .cloned()
            .ok_or_else(|| format!("there is no project {project}"))?;
        let git_project = p.kind() == crate::project::Kind::Git;
        let feats = p
            .features
            .iter()
            .filter(|f| only.is_none_or(|o| f.name() == o))
            .map(|f| {
                let conv = f
                    .conversation
                    .as_ref()
                    .and_then(|c| self.studio.convs.get(c));
                let loop_wt = f.run.and_then(|id| self.runs.get(&id)).map(|r| {
                    let who = r
                        .running
                        .keys()
                        .next()
                        .and_then(|n| r.state.steps.get(n))
                        .and_then(|s| s.def.clone());
                    (r.worktrees().join("build"), !r.state.stage.is_final(), who)
                });
                FeatureCopy {
                    name: f.name().to_string(),
                    dir: f.dir.clone(),
                    base_branch: f.base_branch.clone(),
                    branch: f.branch.clone(),
                    loop_wt,
                    spec_wt: conv.map(|c| c.state.workdir.clone()),
                    lead: conv.map_or_else(|| "lead".into(), |c| c.state.lead_name().to_string()),
                }
            })
            .collect();
        Ok(files::versions(&p.path, git_project, feats))
    }

    fn file_version(
        &self,
        project: &str,
        id: &str,
    ) -> Result<crate::studio::files::Version, String> {
        let id = if id.is_empty() { "folder" } else { id };
        // The folder needs no feature; a feature, only itself.
        let only = id.strip_prefix("feature:").unwrap_or("\u{0}");
        self.versions_of(project, Some(only))?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| crate::studio::files::GONE.to_string())
    }

    /// The files a message refers to with `@path`, optionally followed by `(on <feature>)`
    /// (009 T031): their text for the lead, within the file view's limits; a secret by its name
    /// only. In a spec session a file is the feature's; in the project conversation, the folder's.
    fn file_refs(&self, conv: &str, text: &str) -> Vec<crate::agent::prompts::FileRef> {
        use crate::studio::files;
        let Some(c) = self.studio.convs.get(conv) else {
            return Vec::new();
        };
        let project = c.state.project.clone();
        let own = c
            .state
            .feature
            .as_ref()
            .map_or_else(|| "folder".to_string(), |f| format!("feature:{f}"));
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find('@') {
            let at_start = i == 0 || rest[..i].ends_with(char::is_whitespace);
            let tail = &rest[i + 1..];
            let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            let token = tail[..end].trim_end_matches([',', ';', ':', ')', '!', '?', '.']);
            rest = &tail[end..];
            if !at_start || token.is_empty() || files::safe_rel(token).is_err() {
                continue;
            }
            let after = rest.trim_start();
            let version = match after.strip_prefix("(on ").and_then(|a| a.split_once(')')) {
                Some((f, _)) => format!("feature:{}", f.trim()),
                None => own.clone(),
            };
            let Ok(v) = self.file_version(&project, &version) else {
                continue;
            };
            let label = if v.id == "folder" {
                "the project folder".to_string()
            } else {
                format!("{} on its branch", v.label)
            };
            let mut r = crate::agent::prompts::FileRef {
                path: token.to_string(),
                version: label,
                text: None,
                note: None,
            };
            if files::is_secret(token) {
                r.note = Some(files::SECRET.to_string());
            } else {
                match files::file(&v, token, false, &|a, b| self.step_agent(a, b)) {
                    Ok(f) if f["kind"] == "text" || f["kind"] == "deleted" => {
                        let t = f["text"].as_str().unwrap_or_default();
                        let mut cut = t.len().min(files::REF_MAX);
                        while !t.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        r.text = Some(t[..cut].to_string());
                        if cut < t.len() || f["cut"] == true {
                            r.note = Some("only its beginning: the file is longer".into());
                        } else if f["kind"] == "deleted" {
                            r.note = Some("deleted on this version: as it was before".into());
                        }
                    }
                    Ok(f) if f["kind"] == "binary" => r.note = Some("a binary file".into()),
                    Ok(f) if f["kind"] == "secret" => r.note = Some(files::SECRET.to_string()),
                    _ => r.note = Some("not found".into()),
                }
            }
            out.push(r);
            if out.len() >= 5 {
                break;
            }
        }
        out
    }

    /// The agent definition of a loop's step, for "who changed it".
    fn step_agent(&self, run: u64, n: u32) -> Option<String> {
        self.runs.get(&run)?.state.steps.get(&n)?.def.clone()
    }

    /// "Open in my editor" (FR-032): the saved editor, or the engineer's choice saved first, or
    /// the editors found to choose from, with the file's path to copy.
    fn open_file(
        &self,
        project: &str,
        version: &str,
        path: &str,
        editor: Option<crate::studio::files::Editor>,
        command: Option<String>,
    ) -> Result<Value, String> {
        use crate::studio::files;
        let v = self.file_version(project, version)?;
        if v.rev.is_some() {
            return Err(
                "This version has no working copy any more: open the project folder's version"
                    .into(),
            );
        }
        let abs = files::safe_path(&v.dir, path)?;
        let data = &self.studio.data;
        let chosen = match (command, editor) {
            (Some(c), _) => Some(files::parse_command(&c)?),
            (None, Some(e)) => Some(e),
            (None, None) => None,
        };
        let ed = match chosen {
            Some(e) => files::set_editor(data, Some(e))?,
            None => files::editor(data),
        };
        match ed {
            Some(e) => files::open(&abs, &e).map(|()| json!({ "opened": true, "editor": e })),
            None => {
                Ok(json!({ "choose": files::found_editors(), "path": abs.display().to_string() }))
            }
        }
    }

    /// A feature as the spec session shows it (mockup screen 2).
    fn feature_view(&self, project: &str, feature: &str) -> Result<Value, String> {
        let p = self
            .studio
            .store
            .get(project)
            .ok_or_else(|| format!("there is no project {project}"))?;
        let f = p
            .feature(feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?;
        let wd = f
            .conversation
            .as_ref()
            .and_then(|c| self.studio.convs.get(c))
            .map(|c| c.state.workdir.clone())
            .unwrap_or_else(|| p.path.clone());
        let docs: Vec<Value> = crate::studio::Studio::docs(f)
            .into_iter()
            .map(|d| json!({ "path": d, "exists": wd.join(&d).is_file() }))
            .collect();
        let markers = std::fs::read_to_string(wd.join(&f.dir).join("spec.md"))
            .ok()
            .and_then(|t| crate::speckit::spec::read("spec.md", &t).ok())
            .map(|s| s.markers)
            .unwrap_or_default();
        let lock_check = match self.studio.lock_check(project, feature) {
            Ok(l) => {
                json!({ "ok": true, "tasks": l.phases.iter().map(|p| p.tasks.len()).sum::<usize>() })
            }
            Err(problems) => json!({ "ok": false, "problems": problems }),
        };
        let tasks: usize = std::fs::read_to_string(wd.join(&f.dir).join("tasks.md"))
            .ok()
            .and_then(|t| crate::speckit::tasks::read("tasks.md", &t).ok())
            .map(|ps| ps.iter().map(|p| p.tasks.len()).sum())
            .unwrap_or(0);
        let lib = self.studio.library(Some(&p.path));
        let mut v = self.feature_summary(p, f);
        v["documents"] = json!(docs);
        v["markers"] = json!(markers);
        v["lock_check"] = lock_check;
        v["locks"] = json!(f.locks);
        v["findings"] = json!(f.findings);
        v["analyzed"] = json!(f.analyzed);
        v["base_branch"] = json!(f.base_branch);
        v["branch"] = json!(f.branch);
        v["unlocked"] = json!(f.unlocked);
        v["kind"] = json!(p.kind());
        v["pipeline"] = json!(f.pipeline.clone().unwrap_or_else(|| p.pipeline.clone()));
        v["autonomy"] = json!(f.level.unwrap_or(p.level));
        v["pipelines"] = json!(lib
            .pipelines
            .values()
            .map(|pl| json!({ "name": pl.name, "verifying": pl.verifying, "missing": pl.missing }))
            .collect::<Vec<_>>());
        v["budget_suggestion"] = json!({
            "tasks": tasks,
            "usd": (tasks as f64 * p.budget_per_task.usd * 100.0).round() / 100.0,
            "minutes": tasks as u64 * p.budget_per_task.minutes,
        });
        v["steps"] = json!(crate::studio::STEPS);
        v["conversation"] = f
            .conversation
            .as_ref()
            .and_then(|c| self.studio.conv_view(c))
            .unwrap_or(Value::Null);
        Ok(v)
    }

    fn studio_request(&mut self, req: crate::studio::Req) -> Result<Value, String> {
        use crate::studio::Req;
        match req {
            Req::AddProject { path } => self.studio.add_project(&path),
            Req::RemoveProject { id } => {
                // Its loops still going would lose their spec sessions and vanish from the data
                // view while they build: they are stopped, or merged, first.
                let going: Vec<String> = self
                    .runs
                    .values()
                    .filter(|r| !r.state.stage.is_final())
                    .filter_map(|r| r.state.feature_loop.as_ref())
                    .filter(|l| l.project == id)
                    .map(|l| l.feature.clone())
                    .collect();
                if !going.is_empty() {
                    return Err(format!(
                        "the loops of {} are still going: stop or merge them first",
                        going.join(", ")
                    ));
                }
                self.studio.remove_project(&id)
            }
            Req::PatchProject { id, patch } => self.studio.patch_project(&id, &patch),
            Req::SetCommands { id, commands } => self.studio.set_commands(&id, commands),
            Req::GitInit { id } => self.studio.git_init(&id),
            Req::Project { id } => self.project_view(&id),
            Req::Conversation { id } => self
                .studio
                .conv_view(&id)
                .ok_or_else(|| format!("there is no conversation {id}")),
            Req::ProjectConversation { project } => self
                .studio
                .project_conversation(&project)
                .map(|id| json!({ "conversation": id })),
            Req::Say { conv, text } => {
                let refs = self.file_refs(&conv, &text);
                self.studio.say(&self.ctx, &conv, &text, refs)
            }
            Req::Cancel { conv } => self.studio.cancel(&conv),
            Req::Commands { conv } => self.studio.commands(&conv),
            Req::SetMode { conv, mode } => self.studio.set_mode(&conv, mode),
            Req::Approve {
                conv,
                id,
                answer,
                prefix,
            } => self.studio.approve(&conv, &id, answer, prefix),
            Req::NewFeature {
                project,
                title,
                lead,
            } => self.studio.new_feature(&project, &title, lead),
            Req::SetLead { conv, lead } => self.studio.set_lead(&conv, &lead),
            Req::FileVersions { project } => self
                .file_versions(&project)
                .map(|v| json!({ "versions": v })),
            Req::Files { project, version } => {
                let v = self.file_version(&project, &version)?;
                let (entries, fingerprint, changed) =
                    crate::studio::files::entries(&v, &|r, n| self.step_agent(r, n))?;
                Ok(
                    json!({ "version": v, "entries": entries, "fingerprint": fingerprint, "changed": changed }),
                )
            }
            Req::File {
                project,
                version,
                path,
                changes,
            } => {
                let v = self.file_version(&project, &version)?;
                crate::studio::files::file(&v, &path, changes, &|r, n| self.step_agent(r, n))
            }
            Req::OpenFile {
                project,
                version,
                path,
                editor,
                command,
            } => self.open_file(&project, &version, &path, editor, command),
            Req::GetEditor => {
                Ok(json!({ "editor": crate::studio::files::editor(&self.studio.data) }))
            }
            Req::SetEditor {
                editor,
                command,
                clear,
            } => {
                use crate::studio::files;
                let e = if clear {
                    None
                } else if let Some(c) = command {
                    Some(files::parse_command(&c)?)
                } else {
                    Some(editor.ok_or("say which program opens files")?)
                };
                files::set_editor(&self.studio.data, e).map(|e| json!({ "editor": e }))
            }
            Req::Feature { project, feature } => self.feature_view(&project, &feature),
            Req::Doc {
                project,
                feature,
                path,
            } => self.studio.doc(&project, &feature, &path),
            Req::PutDoc {
                project,
                feature,
                path,
                text,
                version,
            } => self
                .studio
                .put_doc(&project, &feature, &path, &text, &version),
            Req::SetStep {
                project,
                feature,
                to,
            } => self.studio.set_step(&project, &feature, &to),
            Req::Dismiss {
                project,
                feature,
                finding,
            } => self.studio.dismiss(&project, &feature, finding),
            Req::ToolOpen { token, pid } => match self.run_of_token(&token) {
                Some(id) => super::questions::open(&self.runs[&id], &token, pid),
                None => self.studio.tool_open(&token, pid),
            },
            Req::Agents { project } => self.studio.agents_view(project.as_deref()),
            Req::PutAgent { name, scope, text } => {
                let scope = crate::studio::defs::Scope::parse(&scope)?;
                let live = self.live_pipelines();
                self.studio.put_agent(&name, &scope, &text, &live)
            }
            Req::PutAgentFields {
                name,
                scope,
                fields,
                body,
            } => {
                let scope = crate::studio::defs::Scope::parse(&scope)?;
                let live = self.live_pipelines();
                self.studio
                    .put_agent_fields(&name, &scope, &fields, body.as_deref(), &live)
            }
            Req::DeleteAgent { name, scope } => {
                let scope = crate::studio::defs::Scope::parse(&scope)?;
                let live = self.live_pipelines();
                self.studio.delete_agent(&name, &scope, &live)
            }
            Req::Pipelines { project } => self.studio.pipelines_view(project.as_deref()),
            Req::CheckPipeline { project, toml } => {
                self.studio.check_pipeline(project.as_deref(), &toml)
            }
            Req::PutPipeline { name, scope, toml } => {
                let scope = crate::studio::defs::Scope::parse(&scope)?;
                self.studio.put_pipeline(&name, &scope, &toml)
            }
            Req::DeletePipeline { name, scope } => {
                let scope = crate::studio::defs::Scope::parse(&scope)?;
                self.studio.delete_pipeline(&name, &scope)
            }
            Req::Runtimes => Ok(self.runtimes()),
        }
    }

    /// The pipelines the loops still going recorded, with their repository: a definition
    /// change must not break them (their next step reads the agents again).
    fn live_pipelines(&self) -> Vec<(std::path::PathBuf, String)> {
        self.runs
            .values()
            .filter(|r| !r.state.stage.is_final())
            .filter_map(|r| {
                r.state
                    .feature_loop
                    .as_ref()
                    .map(|l| (r.state.repo.clone(), l.pipeline.clone()))
            })
            .collect()
    }

    /// `GET /api/runtimes`: the runtimes Fallow knows, whether each is installed, its version,
    /// and the models a definition may name.
    fn runtimes(&self) -> Value {
        Value::Array(
            crate::agent::registry::list(&self.ctx.settings)
                .into_iter()
                .map(|(name, a, models)| {
                    json!({ "name": name, "installed": a.installed, "version": a.version, "models": models })
                })
                .collect(),
        )
    }

    /// The loop of an unlocked feature that a new lock goes on with.
    fn live_loop(&self, project: &str, feature: &str) -> Option<u64> {
        let (_, f, _) = self.studio.feature_ctx(project, feature).ok()?;
        let id = f.run?;
        let r = self.runs.get(&id)?;
        (f.unlocked && r.state.feature_loop.is_some() && !r.state.stage.is_final()).then_some(id)
    }

    /// Unlocks a feature (FR-009): recorded on its loop, which halts after its current step,
    /// and the spec session opens again.
    fn unlock(&mut self, project: &str, feature: &str) -> Result<Value, String> {
        let (_, f, _) = self.studio.feature_ctx(project, feature)?;
        if f.locks.is_empty() || f.unlocked {
            return Err("the feature is not locked".into());
        }
        let id = f.run.ok_or("the feature has no loop")?;
        let r = self
            .runs
            .get_mut(&id)
            .ok_or("the feature's loop is missing")?;
        if r.state.stage.is_final() {
            return Err("the loop has ended: changes go in a new feature".into());
        }
        r.append(Event::Steering {
            kind: SteeringKind::Unlock,
            target: None,
            detail: None,
        })?;
        r.append(Event::Unlocked)?;
        self.studio.mark_unlocked(project, feature)?;
        Ok(json!({ "unlocked": true, "loop": id }))
    }

    /// The documents a new lock is compared with: the last lock, with the answers the loop
    /// committed to the feature branch since (T086: comparing with the lock alone counted the
    /// loop's own Clarifications as the engineer's change, and reopened every story).
    fn relock_base(&self, project: &str, feature: &str, repo: &Path, lock: &str) -> String {
        let answers: Vec<String> = self
            .live_loop(project, feature)
            .and_then(|id| self.runs.get(&id))
            .and_then(|r| r.state.feature_loop.as_ref())
            .map(|l| {
                l.questions
                    .values()
                    .filter_map(|q| q.commit.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut base = lock.to_string();
        for c in answers {
            if c != base && git::is_ancestor(repo, &base, &c) {
                base = c;
            }
        }
        base
    }

    /// What a new lock would reopen, from the documents as they are now (research S-17).
    fn relock_preview(&self, project: &str, feature: &str) -> Result<Value, String> {
        let (p, f, wd) = self.studio.feature_ctx(project, feature)?;
        let old = f.locks.last().ok_or("the feature was never locked")?;
        let base = self.relock_base(project, feature, &p.path, &old.commit);
        let docs = super::relock::docs(
            &p.path,
            &f.dir,
            &base,
            super::relock::Side::WorkingCopy(&wd),
        )?;
        let a = super::relock::affected(&docs);
        Ok(json!({ "affected": a.stories, "why": a.why }))
    }

    /// A new lock of an unlocked feature: the loop goes on, with the affected stories reopened
    /// (research S-17).
    fn relock(&mut self, project: &str, feature: &str, id: u64) -> Result<Value, String> {
        let (_, f, _) = self.studio.feature_ctx(project, feature)?;
        let old = f
            .locks
            .last()
            .cloned()
            .ok_or("the feature was never locked")?;
        let prep = self
            .studio
            .lock_prepare(project, feature)
            .map_err(|problems| {
                format!(
                    "the feature cannot be locked yet:\n- {}",
                    problems.join("\n- ")
                )
            })?;
        let base = self.relock_base(project, feature, &prep.project.path, &old.commit);
        let docs = super::relock::docs(
            &prep.project.path,
            &prep.feature.dir,
            &base,
            super::relock::Side::Commit(&prep.commit),
        )?;
        let a = super::relock::affected(&docs);
        let r = self
            .runs
            .get_mut(&id)
            .ok_or("the feature's loop is missing")?;
        r.append(Event::Relocked {
            lock: prep.lock,
            commit: prep.commit.clone(),
            affected: a.stories.clone(),
            phases: prep.phases.clone(),
        })?;
        for w in &a.why {
            r.append(Event::Note { text: w.clone() })?;
        }
        if r.state.stage == Stage::Ready {
            r.append(Event::Stage {
                to: Stage::Building,
                why: format!(
                    "lock {}: the affected stories are built and checked again",
                    prep.lock
                ),
            })?;
        }
        let rec = crate::project::LockRec {
            n: prep.lock,
            commit: prep.commit.clone(),
            at: super::time::rfc3339(super::time::unix_now()),
            affected: a.stories.clone(),
            ..old
        };
        self.studio.lock_record(project, feature, rec, id)?;
        self.drive(id);
        Ok(json!({ "lock": prep.lock, "commit": prep.commit, "loop": id, "affected": a.stories }))
    }

    /// Locks a feature and creates its loop (FR-008, FR-016a; research S-9).
    fn lock(
        &mut self,
        project: &str,
        feature: &str,
        pipeline: Option<String>,
        level: Option<Level>,
        budget_usd: Option<f64>,
        budget_minutes: Option<u64>,
    ) -> Result<Value, String> {
        if let Some(id) = self.live_loop(project, feature) {
            return self.relock(project, feature, id);
        }
        let prep = self
            .studio
            .lock_prepare(project, feature)
            .map_err(|problems| {
                format!(
                    "the feature cannot be locked yet:\n- {}",
                    problems.join("\n- ")
                )
            })?;
        let p = &prep.project;
        let f = &prep.feature;
        let pipeline_name = pipeline
            .or_else(|| f.pipeline.clone())
            .unwrap_or_else(|| p.pipeline.clone());
        let pl = prep
            .library
            .pipelines
            .get(&pipeline_name)
            .cloned()
            .ok_or_else(|| format!("there is no pipeline {pipeline_name:?}"))?;
        let level = level.or(f.level).unwrap_or(p.level);
        let tasks: usize = prep.phases.iter().map(|ph| ph.tasks.len()).sum();
        let usd = budget_usd
            .filter(|u| *u > 0.0)
            .unwrap_or(tasks as f64 * p.budget_per_task.usd);
        let minutes = budget_minutes
            .filter(|m| *m > 0)
            .unwrap_or(tasks as u64 * p.budget_per_task.minutes);
        let base = f
            .base_branch
            .clone()
            .ok_or("the feature has no branch to merge into")?;
        let build = format!("fallow/{}/build", f.name());
        if prep.lock == 1 && git::rev(&p.path, &format!("refs/heads/{build}")).is_ok() {
            return Err(format!(
                "the branch {build} already exists: remove it to lock this feature"
            ));
        }
        git::create_branch(&p.path, &build, &prep.commit)?;
        let mut notices = Vec::new();
        let squad = match git::show(&p.path, &prep.commit, crate::config::squad::SQUAD_FILE) {
            Ok(Some(text)) => Squad::parse(crate::config::squad::SQUAD_FILE, &text, &mut notices),
            _ => Squad::parse(crate::config::squad::SQUAD_FILE, "", &mut notices),
        };
        let s = &self.ctx.settings;
        let id = self.next_id();
        let dir = runs_dir(&self.data).join(id.to_string());
        let (mut log, _, _) = Log::open(&dir)?;
        let created = Event::RunCreated {
            repo: p.path.clone(),
            target: base.clone(),
            start_commit: prep.commit.clone(),
            task: f.title.clone(),
            mode: Mode::Solo,
            autonomy: s.defaults.autonomy,
            limits: Limits {
                usd,
                minutes,
                step_minutes: s.defaults.step_min,
            },
            roles: s.roles.clone(),
            repair_cap: pl.caps.task_repairs,
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
            left_out: Vec::new(),
        };
        let first = log.append(created)?;
        let state = RunState::fold(id, &[first])?;
        let mut run = Run {
            dir,
            log,
            state,
            busy: None,
            stop: Arc::new(AtomicBool::new(false)),
            waiters: Vec::new(),
            activity: 0,
            running: BTreeMap::new(),
            waiting: None,
        };
        run.append(Event::LoopCreated {
            project: p.id.clone(),
            feature: f.name().to_string(),
            lock: prep.lock,
            lock_commit: prep.commit.clone(),
            base_branch: base,
            build_branch: build,
            pipeline: pl.to_toml(),
            level,
            budget_usd: usd,
            budget_minutes: minutes,
            phases: prep.phases.clone(),
            when_verified: crate::run::events::WhenVerified::preset(level),
        })?;
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("lock {} of {}", prep.lock, f.name()),
        })?;
        for n in notices {
            run.append(Event::Note {
                text: n.to_string(),
            })?;
        }
        let rec = crate::project::LockRec {
            n: prep.lock,
            commit: prep.commit.clone(),
            at: super::time::rfc3339(super::time::unix_now()),
            pipeline: pipeline_name,
            level,
            budget_usd: usd,
            budget_minutes: minutes,
            affected: Vec::new(),
            when_verified: crate::run::events::WhenVerified::preset(level),
        };
        let lock_n = prep.lock;
        let commit = prep.commit.clone();
        self.runs.insert(id, run);
        self.studio.lock_record(project, feature, rec, id)?;
        Ok(json!({ "lock": lock_n, "commit": commit, "loop": id }))
    }

    /// The dashboard's summary (contracts/web-api.md).
    fn dashboard(&self) -> Value {
        let today = super::time::rfc3339(super::time::unix_now());
        let today = today.get(..10).unwrap_or_default();
        let needs_you = self.needs_items();
        let loops: Vec<Value> = self
            .runs
            .values()
            .rev()
            .filter(|r| !r.state.stage.is_final())
            .map(|r| {
                let name = r
                    .state
                    .feature_loop
                    .as_ref()
                    .and_then(|l| self.studio.store.get(&l.project))
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                super::dashboard::card(r, &self.ctx, &name).unwrap_or_else(|| summary(r))
            })
            .collect();
        let agents_running = self.runs.values().map(|r| r.running.len()).sum::<usize>();
        let cost_today: f64 = self
            .runs
            .values()
            .filter(|r| r.state.created.starts_with(today))
            .map(|r| r.state.cost_usd)
            .sum::<f64>()
            // The lead's turns of today count too (T086: the dashboard showed loops only).
            + self
                .studio
                .convs
                .values()
                .flat_map(|c| c.state.turns.iter())
                .filter(|t| t.started.starts_with(today))
                .filter_map(|t| t.cost_usd)
                .sum::<f64>();
        let projects: Vec<Value> = self
            .studio
            .store
            .projects
            .iter()
            .map(|p| self.project_summary(p))
            .collect();
        let features: Vec<Value> = self
            .studio
            .store
            .projects
            .iter()
            .flat_map(|p| p.features.iter().map(move |f| (p, f)))
            .map(|(p, f)| self.feature_summary(p, f))
            .collect();
        json!({
            "projects": projects,
            "features": features,
            "needs_you": needs_you,
            "loops": loops,
            "agents_running": agents_running,
            "cost_today": cost_today,
        })
    }

    /// The slots and memory room left for loop steps (research S-19). The lead's turns in a chat
    /// always start, so they go first; they count against what loops may use.
    fn refresh_slots(&self) {
        let d = &self.ctx.settings.defaults;
        let mut running = 0i64;
        let mut used = 0i64;
        let mut count = |g: Option<&crate::platform::job::Group>| {
            running += 1;
            used += g.map_or(0, |g| g.memory_mb() as i64).max(STEP_MB);
        };
        for r in self.runs.values() {
            for x in r.running.values() {
                count(Some(&x.group));
            }
        }
        let turns: Vec<&crate::studio::Tool> = self.studio.tools.values().collect();
        for c in self.studio.convs.values().filter(|c| c.state.busy) {
            let g = turns
                .iter()
                .find(|t| matches!(&t.owner, crate::studio::Owner::Conv(id) if *id == c.id()));
            count(g.map(|t| &*t.group));
        }
        let budget = match d.memory_budget_mb {
            0 => crate::platform::total_memory_mb().map_or(i64::MAX, |t| (t * 3 / 4) as i64),
            b => b as i64,
        };
        self.ctx
            .slots
            .set(d.max_parallel as i64 - running, budget.saturating_sub(used));
    }

    fn drive(&mut self, id: u64) {
        self.refresh_slots();
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
            Request::Dashboard => Ok(self.dashboard()),

            Request::Events { .. } | Request::ToolCall { .. } => {
                Err("answered by the engine's loop".into())
            }
            Request::Studio(req) => {
                use crate::studio::Req;
                let reads = matches!(
                    req,
                    Req::Project { .. }
                        | Req::Conversation { .. }
                        | Req::Feature { .. }
                        | Req::Doc { .. }
                        | Req::Commands { .. }
                        | Req::FileVersions { .. }
                        | Req::Files { .. }
                        | Req::File { .. }
                        | Req::GetEditor
                        | Req::ToolOpen { .. }
                );
                let out = self.studio_request(req);
                if !reads {
                    self.bus.publish(json!({ "type": "studio" }));
                }
                out
            }
            Request::Lock {
                project,
                feature,
                pipeline,
                level,
                budget_usd,
                budget_minutes,
            } => {
                let out = self.lock(
                    &project,
                    &feature,
                    pipeline,
                    level,
                    budget_usd,
                    budget_minutes,
                );
                self.bus.publish(json!({ "type": "studio" }));
                out
            }
            Request::Unlock { project, feature } => {
                let out = self.unlock(&project, &feature);
                self.bus.publish(json!({ "type": "studio" }));
                out
            }
            Request::RelockPreview { project, feature } => self.relock_preview(&project, &feature),
            Request::Answer { id, text } => {
                // A question relayed to the engineer is answered as such: committed first.
                let waiting = self
                    .runs
                    .get(&id)
                    .and_then(|r| r.state.feature_loop.as_ref())
                    .and_then(super::questions::waiting)
                    .map(|q| q.id.clone());
                match waiting {
                    Some(q) => self.question_answer(id, &q, &text, false),
                    None => run_mut(self, id)
                        .and_then(|r| super::machine::answer(r, &text))
                        .map(|()| json!({})),
                }
            }
            Request::QuestionAnswer { id, q, answer } => {
                self.question_answer(id, &q, &answer, false)
            }
            Request::Overrule { id, q, answer } => self.question_answer(id, &q, &answer, true),
            Request::QuestionConfirm { id, q } => {
                run_mut(self, id).and_then(|r| super::questions::confirm(r, &q))
            }
            Request::Data => Ok(self.data_view()),
            Request::DeleteData { kind, id } => self.delete_data(&kind, &id),
            Request::DeleteFeature { project, feature } => self.delete_feature(&project, &feature),
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
            Request::Loop { id } => {
                let ctx = &self.ctx;
                self.runs
                    .get(&id)
                    .ok_or_else(|| format!("there is no loop {id}"))
                    .and_then(|r| super::activity::view(r, ctx))
            }
            Request::LoopActivity { id, filter } => self
                .runs
                .get(&id)
                .ok_or_else(|| format!("there is no loop {id}"))
                .map(|r| super::activity::activity(r, &filter)),
            Request::Task { id, task } => self
                .runs
                .get(&id)
                .ok_or_else(|| format!("there is no loop {id}"))
                .and_then(|r| super::activity::task(r, &task)),
            Request::Pause { id } => run_mut(self, id)
                .and_then(super::machine::pause)
                .map(|()| json!({})),
            Request::GoOn { id } => run_mut(self, id)
                .and_then(super::machine::go_on)
                .map(|()| json!({})),
            Request::Note { id, text, agent } => run_mut(self, id)
                .and_then(|r| super::machine::note(r, &text, agent))
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

impl Engine {
    /// The loop whose running step holds this tool token.
    fn run_of_token(&self, token: &str) -> Option<u64> {
        self.runs
            .iter()
            .find(|(_, r)| r.running.values().any(|x| x.tool.token == token))
            .map(|(id, _)| *id)
    }

    /// The lead of a loop's feature: its spec session, worktree and approved commands.
    fn lead_of(&self, id: u64) -> Result<super::questions::Lead, String> {
        let l = self
            .runs
            .get(&id)
            .and_then(|r| r.state.feature_loop.as_ref())
            .ok_or("not a feature loop")?;
        self.studio.lead_of(&l.project, &l.feature)
    }

    fn question_answer(
        &mut self,
        id: u64,
        q: &str,
        answer: &str,
        overrule: bool,
    ) -> Result<Value, String> {
        let lead = self.lead_of(id)?;
        let r = run_mut(self, id)?;
        let out = if overrule {
            super::questions::overrule(r, &lead, q, answer)
        } else {
            super::questions::engineer_answer(r, &lead, q, answer)
        };
        self.drive(id);
        out
    }
}

fn run_mut(e: &mut Engine, id: u64) -> Result<&mut Run, String> {
    e.runs
        .get_mut(&id)
        .ok_or_else(|| format!("there is no run {id}"))
}

/// One line of the runs list.
pub fn summary(r: &Run) -> Value {
    let s = &r.state;
    json!({
        "id": s.id,
        "repo": s.repo,
        "task": s.task,
        "feature": s.feature_loop.as_ref().map(|l| l.feature.clone()),
        "project": s.feature_loop.as_ref().map(|l| l.project.clone()),
        "stage": s.stage,
        "needs_engineer": needs(r),
        "cost_usd": s.cost_usd,
        "created": s.created,
        "seq": s.seq,
        "waiting": r.waiting,
    })
}

/// What needs the engineer now, if anything.
pub fn needs(r: &Run) -> Option<String> {
    let s = &r.state;
    if let Some(n) = &s.needs {
        return Some(n.why.clone());
    }
    if let Some(l) = &s.feature_loop {
        if s.stage != Stage::Ready {
            return None;
        }
        // An unverified result never in the words of a verified one (constitution).
        return Some(match &l.outcome {
            Some((false, missing)) => format!(
                "Done, not verified by Fallow ({}): review it before merging",
                missing.join(", ")
            ),
            Some((true, _)) => "Ready to merge".into(),
            None => return None,
        });
    }
    match s.stage {
        Stage::Planning if s.plan_approvable && !r.is_busy() => {
            Some("The plan is ready to approve".into())
        }
        Stage::Planning if !r.is_busy() => Some("The planner is waiting for your answer".into()),
        Stage::Ready => Some("Ready to merge".into()),
        _ => None,
    }
}

/// The whole run for the page.
pub fn view(r: &Run) -> Value {
    let mut v = serde_json::to_value(&r.state).unwrap_or(Value::Null);
    v["needs_engineer"] = json!(needs(r));
    let steps: Vec<String> = r.running.keys().map(u32::to_string).collect();
    v["busy"] = json!(match &r.busy {
        Some(Busy::Gates) => "gates".into(),
        Some(Busy::Merge) => "merge".into(),
        None if steps.len() == 1 => format!("step {}", steps[0]),
        None if !steps.is_empty() => format!("steps {}", steps.join(", ")),
        None => String::new(),
    });
    v["waiting"] = json!(r.waiting);
    v["interventions"] = json!(r.state.interventions());
    v["minutes_used"] = json!(r.state.minutes_used(super::time::unix_now()));
    v
}
