//! The engine: one thread that owns every run (006 research R-1, R-2).
//!
//! Requests from the page and the command line, and the results of agent steps, gates and
//! merges, arrive as messages. Each change is an event, written to the run's log before it is
//! applied; then the stage machine decides what happens next. Agent calls, gates and merges run
//! on threads of their own and report back here.

use std::collections::{BTreeMap, BTreeSet};
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
    /// 010: a publication ended (`by` is `engineer` or `when_verified`).
    Publish {
        by: String,
        result: Result<crate::publish::Done, crate::publish::Failed>,
    },
    /// 010: the pull request's state, as the remote reports it.
    PrState(Result<crate::publish::host::PrState, String>),
    /// 011: an update's merge computed (research R-2).
    Update {
        n: u32,
        result: Result<super::update::Computed, String>,
    },
    /// 011: what the target watch read (research R-1), for the verified commit `integrated`.
    Moved {
        integrated: String,
        result: Result<super::update::Moved, String>,
    },
    /// 011 (research R-7): the remote target fetched by sha, to bring a published loop up to
    /// date.
    Fetched(Result<String, String>),
    /// 011 (FR-012): the push forward of a published loop's branch after its update: `Ok(())`
    /// pushed; `Err(Ok(sha))` the remote target moved since the update started (nothing pushed);
    /// `Err(Err(why))` refused or failed, nothing pushed.
    PushForward {
        from: String,
        to: String,
        result: Result<(), Result<String, String>>,
        /// Pushed at the end of an update (a move then counts as overtaken), or a verified
        /// commit pushed later, its update gone (a move then is a fresh one).
        with_update: bool,
    },
}

/// 011: what a published loop's remote work leads to, on the engine thread.
enum After {
    Fetched(Result<String, String>),
    Pushed {
        from: String,
        to: String,
        result: Result<(), Result<String, String>>,
        with_update: bool,
    },
    PublishMoved(String),
}

/// 011: the target watch of one loop.
#[derive(Default)]
struct Watched {
    asked: Option<Instant>,
    running: bool,
    integrated: String,
    read: Option<super::update::Moved>,
    error: Option<String>,
}

/// 011 (FR-010): a loop locked Free brings itself up to date, unless `[updates] auto = false`.
fn update_by_itself(l: &super::state::LoopState) -> bool {
    l.level == Level::Free && l.switches.auto
}

/// 011: how a loop waiting for another says so (`Run::waiting`).
const WAITING_FOR: &str = "waiting for ";

/// 011 (FR-014, research R-8): a loop that waited from its lock and built nothing starts from
/// `tip` (the target as it is then). Changed documents are held as an update (FR-005, so "Lock
/// again" can take it); otherwise the merge is committed straight onto its branch (nothing built,
/// so no gates or checker now) and `StartedFrom` moves its base to `tip`, no update left in
/// progress (phase 7 reviews 2, F1).
fn start_from_target(r: &mut Run, tip: &str) -> Result<(), String> {
    let Some(l) = r.state.feature_loop.clone() else {
        return Ok(());
    };
    if !l.tasks.values().all(|t| t.commit.is_none()) || l.update.is_some() {
        return Ok(());
    }
    let repo = r.state.repo.clone();
    let head = git::rev(&repo, &format!("refs/heads/{}", l.build_branch))?;
    if git::is_ancestor(&repo, tip, &head) {
        // Merged before a restart cut it short: the base still moves (FR-017).
        // Only when the lock did not hold the tip already: a merge was made (review N2).
        if l.base_commit != tip
            && head != l.lock_commit
            && !git::is_ancestor(&repo, tip, &l.lock_commit)
        {
            r.append(Event::StartedFrom {
                target_commit: tip.to_string(),
                commit: head,
            })?;
        }
        return Ok(());
    }
    let n = l.updates + 1;
    let index = r
        .dir
        .join(format!("start-{}.index", super::time::unix_ms()));
    let computed = super::update::compute(
        &repo,
        &l.feature,
        n,
        &head,
        tip,
        &r.state.target,
        l.reconciled_target.as_deref(),
        &index,
    );
    let _ = std::fs::remove_file(&index);
    match computed? {
        c @ super::update::Computed::Conflicted { .. } => {
            super::update::discard(r, &Ok(c));
            Err(format!(
                "{} conflicts with what the lock committed (other than this feature's documents)",
                r.state.target
            ))
        }
        super::update::Computed::DocsChanged(paths) => {
            r.append(Event::Updated {
                n,
                target: r.state.target.clone(),
                target_commit: tip.to_string(),
                loop_head: head,
                remote: false,
                in_turn: false,
                for_publication: false,
            })?;
            r.append(Event::UpdateDocsChanged {
                n,
                paths: paths.clone(),
            })?;
            r.append(Event::NeedsEngineer {
                why: format!(
                    "{} changed this feature's documents while the loop waited ({}): lock again with them, or stop.",
                    r.state.target,
                    paths.join(", ")
                ),
                options: vec!["lock again".into(), "stop".into()],
            })
        }
        super::update::Computed::Clean(commit) => {
            let wt = r.worktrees().join("build");
            if wt.join(".git").exists() {
                git::checkout_forward(&wt, &l.build_branch, &commit)?;
            } else {
                git::set_branch(&repo, &l.build_branch, &commit)?;
            }
            r.append(Event::StartedFrom {
                target_commit: tip.to_string(),
                commit,
            })
        }
    }
}

/// 011: how a loop waiting in its merge line says so (`Run::waiting`).
const IN_LINE: &str = "in line to merge after ";

/// 011: the lock's settings beyond 010's.
#[derive(Debug, Clone, Default)]
pub struct LockExtra {
    pub update_limit: Option<u32>,
    pub run_after: Option<u64>,
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
        /// 011: "Merge without the latest check", only at the limit of overtaken updates.
        without_latest_check: bool,
    },
    /// 011: bring the loop up to date with its target (also "Verify again on the latest").
    Update {
        id: u64,
    },
    /// 011: "Leave the pull request as it is" at the limit of overtaken updates (FR-009).
    LeavePr {
        id: u64,
    },
    /// 011 (FR-013): the unfinished loops of the project whose files overlap what this
    /// feature's tasks name, read before locking.
    Overlap {
        project: String,
        feature: String,
    },
    /// 011 (FR-015): "Wait for <feature>": this loop starts no new step until loop `other` is
    /// merged, stopped or deleted.
    Wait {
        id: u64,
        other: u64,
    },
    /// 011: "Stop waiting".
    StopWaiting {
        id: u64,
    },
    /// 011: the target changed the feature's documents: unlock it and open its spec session
    /// with a message showing the target's version (FR-005).
    LockAgain {
        id: u64,
    },
    Stop {
        id: u64,
        reason: Option<String>,
    },
    /// 007: the dashboard's summary (contracts/web-api.md `GET /api/dashboard`).
    Dashboard,
    /// 012 (FR-040): git as last checked; `check` reads it again ("Check again").
    Git {
        check: bool,
    },
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
        /// 010: what the loop does when it ends verified; the level's preset when absent.
        when_verified: Option<WhenVerified>,
        /// 011: updates in a row that may be overtaken before the engineer decides (1 to 20,
        /// default 3).
        update_limit: Option<u32>,
        /// 011: the loop this one waits for ("Run after").
        run_after: Option<u64>,
    },
    /// 010: push the loop's verified commit and open a pull request (FR-008, FR-009).
    Publish {
        id: u64,
        /// 011: "Publish without the latest check" (T030).
        without_latest_check: bool,
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
    /// 012 (R-10): a project's refs as the watcher thread read them, and when.
    Refs {
        project: String,
        snap: crate::studio::refwatch::Snapshot,
        at: Instant,
    },
    /// 012 (R-10): the watcher thread went round every project once.
    RefsRound,
    /// 010: a project's remote (git) and its protection (the hosting tool), read on a thread
    /// (research R-1, R-3).
    Remote {
        project: String,
        found: crate::publish::Remote,
        read: Option<crate::publish::Read>,
    },
}

/// What Fallow itself runs for a run right now: its gates or a merge (never persisted: a restart
/// finds nothing running). Agent steps are in `Run::running`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    Gates,
    Merge,
    /// 010: pushing and opening a pull request.
    Publish,
    /// 011: computing an update's merge.
    Update,
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
    /// 011: the target's commits a merge without the latest check was not verified with.
    pub merge_unchecked: Vec<String>,
    /// 011: the same for a publication without the latest check.
    pub publish_unchecked: Vec<String>,
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
        // 011 FR-016b: while its pull request is open, a loop never offers "stop".
        let event = match event {
            Event::NeedsEngineer { why, options } if self.state.stage == Stage::Publishing => {
                Event::NeedsEngineer {
                    why,
                    options: options.into_iter().filter(|o| o != "stop").collect(),
                }
            }
            e => e,
        };
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
    /// 010: the remote of each project's branch (the one checked out, and each loop's target)
    /// as last read, when a read was last started, and how many reads failed in a row (they are
    /// spaced out). Keyed `<project>\n<branch>`.
    remotes: BTreeMap<String, (crate::publish::Remote, Option<Instant>, u32)>,
    /// 010: when each publishing loop's pull request was last read, and whether a read runs.
    pr_reads: BTreeMap<u64, (Instant, bool)>,
    /// 010: per loop, when its pull request was last read well, and why the last read failed.
    pr_checked: BTreeMap<u64, (Option<String>, Option<String>)>,
    /// 010: why a loop did not publish itself when it ended verified (shown on its page).
    auto_waits: BTreeMap<u64, String>,
    /// 010 FR-003: loops seen ready, so the remote is read once when one becomes ready.
    ready_seen: std::collections::BTreeSet<u64>,
    /// 011: the target watch per loop (research R-1): when it last asked, whether a read is
    /// running, for which verified commit, and what it read.
    watched: BTreeMap<u64, Watched>,
    /// 011: how many touched files each loop's overlaps were last computed for.
    overlap_seen: BTreeMap<u64, usize>,
    /// 011: loops ending their wait while the remote target is fetched: the loop waited for,
    /// and why the wait ends.
    starting: BTreeMap<u64, (u64, String)>,
    /// 012 (FR-040): git as last checked, at start, when a project is added, on "Check again".
    pub git: crate::verify::git::GitCheck,
    /// 012: each project's kind as last read for the gate, and when: reading it runs git, too
    /// slow for every request (SC-002); kept a few seconds, forgotten when the project changes.
    kinds: std::cell::RefCell<BTreeMap<String, (Instant, crate::project::Kind)>>,
    /// 012 (R-10): each project's ref watch, and the projects the watcher thread reads.
    pub(crate) watch: BTreeMap<String, crate::run::gitwatch::Watch>,
    pub(crate) watch_list: crate::run::gitwatch::WatchList,
}

/// 010: how often a project's protection is read again: a known value once a minute, an unknown
/// or stale one sooner (research R-6; FR-003).
const REMOTE_KNOWN: Duration = Duration::from_secs(60);
const REMOTE_UNKNOWN: Duration = Duration::from_secs(10);
/// 010: how often an open pull request's state is read (research R-6; SC-005: within 2 min).
const PR_POLL: Duration = Duration::from_secs(60);

/// `PR_POLL`, or `FALLOW_PR_POLL_MS` for tests (a test cannot wait a minute per read).
fn pr_poll() -> Duration {
    static P: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *P.get_or_init(|| {
        std::env::var("FALLOW_PR_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map_or(PR_POLL, Duration::from_millis)
    })
}

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
            pr_reads: BTreeMap::new(),
            pr_checked: BTreeMap::new(),
            auto_waits: BTreeMap::new(),
            ready_seen: std::collections::BTreeSet::new(),
            starting: BTreeMap::new(),
            overlap_seen: BTreeMap::new(),
            watched: BTreeMap::new(),
            data,
            runs: BTreeMap::new(),
            ctx,
            console: Vec::new(),
            bus: crate::service::bus::Bus::default(),
            studio,
            published: BTreeMap::new(),
            git: crate::verify::git::git_check(),
            kinds: std::cell::RefCell::new(BTreeMap::new()),
            watch: BTreeMap::new(),
            watch_list: Default::default(),
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
                        merge_unchecked: Vec::new(),
                        publish_unchecked: Vec::new(),
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
                let tx = e.ctx.tx.clone();
                if let Err(err) = super::machine::resume(run)
                    .and_then(|()| super::questions::orphans(run))
                    .and_then(|()| super::update::resume(run, tx))
                {
                    e.console.push(format!("run {id}: {err}"));
                }
            }
        }
        e.reconcile_publications();
        Ok(e)
    }

    /// Runs the engine until `Shutdown`.
    pub fn serve(mut self, rx: Receiver<Msg>) {
        // 012 (R-10): the ref watcher, every 2 s.
        self.refresh_watch_list();
        crate::run::gitwatch::spawn(self.ctx.tx.clone(), std::sync::Arc::clone(&self.watch_list));
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
                Msg::Refs { project, snap, at } => self.on_refs(&project, snap, at),
                Msg::RefsRound => self.refresh_watch_list(),
                Msg::Studio(note) => {
                    // 012 (R-10): a session's git command ended: its window closes.
                    let closing = match &note {
                        crate::studio::Note::Turn {
                            conv,
                            event: crate::agent::Event::ToolResult { id, .. },
                            ..
                        } => {
                            let done = self.studio.convs.get_mut(conv).and_then(|c| c.open_call.as_mut()).is_some_and(|o| {
                                // A command without an id waits for its turn's end (review, 5).
                                o.ids.retain(|x| x != id);
                                o.ids.is_empty()
                            });
                            done.then(|| conv.clone())
                        }
                        crate::studio::Note::TurnDone { conv, .. } => Some(conv.clone()),
                        _ => None,
                    };
                    if let Some(conv) = closing {
                        self.close_call(&conv);
                    }
                    let done = matches!(
                        note,
                        crate::studio::Note::TurnDone { .. } | crate::studio::Note::GatesDone { .. }
                    );
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
                    // 011: a published loop's fetch and push forward go on here (an update may
                    // start from them, which needs the engine).
                    let after = match &done {
                        Done::Fetched(r) => Some(After::Fetched(r.clone())),
                        Done::PushForward {
                            from,
                            to,
                            result,
                            with_update,
                        } => Some(After::Pushed {
                            from: from.clone(),
                            to: to.clone(),
                            result: result.clone(),
                            with_update: *with_update,
                        }),
                        Done::Publish { result: Err(f), .. } => {
                            f.moved_to.clone().map(After::PublishMoved)
                        }
                        _ => None,
                    };
                    if let Done::Moved { integrated, result } = &done {
                        let w = self.watched.entry(run).or_default();
                        w.running = false;
                        if w.integrated == *integrated {
                            match result {
                                Ok(m) => {
                                    w.read = Some(m.clone());
                                    w.error = None;
                                }
                                Err(e) => w.error = Some(e.clone()),
                            }
                        }
                    }
                    if let Done::PrState(r) = &done {
                        self.pr_reads.insert(run, (Instant::now(), false));
                        let e = self.pr_checked.entry(run).or_default();
                        match r {
                            Ok(_) => {
                                *e = (Some(super::time::rfc3339(super::time::unix_now())), None)
                            }
                            Err(why) => e.1 = Some(why.clone()),
                        }
                    }
                    if let Some(r) = self.runs.get_mut(&run) {
                        // A step's tools end with it; a question it asked is still answered.
                        match &done {
                            Done::Step { n, .. } => {
                                r.running.remove(n);
                            }
                            Done::PrState(_) | Done::Moved { .. } => {}
                            // A fetch for a start after a wait marked nothing busy; a published
                            // loop's fetch marked the update (review N1).
                            Done::Fetched(_) => {
                                if r.busy == Some(Busy::Update) {
                                    r.busy = None;
                                }
                            }
                            _ => r.busy = None,
                        }
                        if let Err(e) = super::machine::on_done(r, &self.ctx, done) {
                            self.console.push(format!("run {run}: {e}"));
                        }
                    }
                    if let Some(a) = after {
                        self.after_remote(run, a);
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
                    self.prune_watch_waiters();
                    self.studio.tick();
                    // 012 (R-9, R-13): waiting messages of solo sessions.
                    self.refresh_slots();
                    self.studio.solo_tick(&self.ctx);
                    self.send_waiting();
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
                Msg::Remote {
                    project,
                    found,
                    read,
                } => self.remote_read(&project, found, read),
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
    /// feature's worktree and branch, and the
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
        if f.locks.is_empty() {
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
                Some(l) => {
                    // 011: an overlap item only while an overlapping loop is unfinished.
                    let live = l.overlaps.iter().any(|o| {
                        self.runs
                            .get(&o.with)
                            .is_some_and(|x| !x.state.stage.is_final())
                    });
                    let mut v = super::dashboard::needs(r, &name(&l.project));
                    v.retain(|i| live || i["kind"] != "overlap");
                    for i in v.iter_mut().filter(|i| i["kind"] == "overlap") {
                        let open: Vec<&super::state::OverlapRec> = l
                            .overlaps
                            .iter()
                            .filter(|o| {
                                self.runs
                                    .get(&o.with)
                                    .is_some_and(|x| !x.state.stage.is_final())
                            })
                            .collect();
                        let files: usize = open.iter().map(|o| o.files.len()).sum();
                        i["key"] = json!(format!("{}:overlap:{files}", r.id()));
                        i["text"] = json!(open
                            .iter()
                            .map(|o| {
                                let f = self
                                    .runs
                                    .get(&o.with)
                                    .and_then(|x| x.state.feature_loop.as_ref())
                                    .map_or_else(|| format!("loop {}", o.with), |x| x.feature.clone());
                                format!("{f} changes {} too", o.files.join(", "))
                            })
                            .collect::<Vec<_>>()
                            .join("; "));
                    }
                    v
                }
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
        let mut items = self.needs_items();
        // 010: a pull request merged on the remote, once (not a "needs you" item).
        for r in self.runs.values() {
            let Some(l) = r.state.feature_loop.as_ref() else {
                continue;
            };
            if let Some(pb) = l
                .publication
                .as_ref()
                .filter(|p| p.state == crate::publish::host::PrStatus::Merged)
            {
                let name = self
                    .studio
                    .store
                    .get(&l.project)
                    .map_or_else(|| l.project.clone(), |p| p.name.clone());
                items.push(json!({
                    "kind": "pr_merged", "key": format!("{}:pr:{}:merged", r.id(), pb.pr_number),
                    "run": r.id(), "project": l.project, "project_name": name, "feature": l.feature,
                    "at": pb.merged_at,
                }));
            }
        }
        // 011: a merge without the latest check, once (FR-009).
        for r in self.runs.values() {
            let Some(l) = r.state.feature_loop.as_ref() else {
                continue;
            };
            if r.state.merged.is_none() || l.unchecked.is_empty() {
                continue;
            }
            let name = self
                .studio
                .store
                .get(&l.project)
                .map_or_else(|| l.project.clone(), |p| p.name.clone());
            items.push(json!({
                "kind": "merged_unchecked", "key": format!("{}:merged_unchecked", r.id()),
                "run": r.id(), "project": l.project, "project_name": name, "feature": l.feature,
                "at": r.state.stage_at, "unchecked": l.unchecked.len(),
            }));
        }
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
        if let Some(p) = scope.strip_prefix("project:") {
            return self.wait_project(p, after, reply);
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
        // 010: its pull request waits for the engineer's merge on the remote.
        if r.state.stage == Stage::Publishing {
            return "pull_request";
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
    /// 010: the project's remote (FR-001 to FR-003), as last read; a new read (git, then the
    /// hosting tool) is started on a thread when the last one is old enough, or at once with
    /// `now` (the engineer opened the project). The engine thread never runs git or the tool for
    /// it. `None` when the project is not on a branch of a git repository.
    pub fn remote_of(
        &mut self,
        p: &crate::project::Project,
        now: bool,
    ) -> Option<crate::publish::Remote> {
        let target = git::branch(&p.path)?;
        Some(self.remote_at(p, &target, now, true))
    }

    /// The remote of `target` in the project (a loop's own target, whatever is checked out:
    /// review of US2); `shown` for the project page's own branch, remembered in projects.json.
    fn remote_at(
        &mut self,
        p: &crate::project::Project,
        target: &str,
        now: bool,
        shown: bool,
    ) -> crate::publish::Remote {
        let key = format!("{}\n{target}", p.id);
        let target = target.to_string();
        let at = Instant::now();
        let seen = if shown { p.remote_seen.clone() } else { None };
        let slot = self.remotes.entry(key.clone()).or_insert_with(|| {
            let mut r = seen.unwrap_or_else(|| crate::publish::Remote {
                state: "checking".into(),
                remote: None,
                service: None,
                repo: None,
                target: None,
                protected: "checking".into(),
                why: None,
                account: None,
                checked_at: None,
                stale: false,
                checking: false,
            });
            r.checking = false;
            (r, None, 0)
        });
        let wait = if slot.0.known() && !slot.0.stale {
            REMOTE_KNOWN
        } else {
            // 10 s, doubled after each read that failed in a row, up to a minute.
            (REMOTE_UNKNOWN * 2u32.pow(slot.2.min(3))).min(REMOTE_KNOWN)
        };
        let fresh = slot
            .1
            .is_some_and(|t| at.duration_since(t) < Duration::from_secs(5));
        let due = !slot.0.checking
            && (slot.1.is_none_or(|t| at.duration_since(t) >= wait) || (now && !fresh));
        if due {
            slot.1 = Some(at);
            slot.0.checking = true;
            let (tx, dir, settings, id) = (
                self.ctx.tx.clone(),
                p.path.clone(),
                self.ctx.settings.clone(),
                key.clone(),
            );
            let spawned = std::thread::Builder::new()
                .name(format!("remote-{id}"))
                .spawn(move || {
                    let found = crate::publish::remote::find(&dir, &target);
                    let read = (found.state == "known").then(|| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            crate::publish::read_protection(&dir, &found, &settings)
                        }))
                        .unwrap_or_else(|_| crate::publish::Read {
                            account: None,
                            protected: Err("reading the protection failed".into()),
                        })
                    });
                    let _ = tx.send(Msg::Remote {
                        project: id,
                        found,
                        read,
                    });
                });
            if let Err(e) = spawned {
                let found = slot.0.clone();
                self.remote_read(
                    &key,
                    found,
                    Some(crate::publish::Read {
                        account: None,
                        protected: Err(format!("cannot start a thread: {e}")),
                    }),
                );
            }
        }
        self.remotes
            .get(&key)
            .map(|s| s.0.clone())
            .unwrap_or_else(|| crate::publish::Remote {
                state: "checking".into(),
                remote: None,
                service: None,
                repo: None,
                target: None,
                protected: "checking".into(),
                why: None,
                account: None,
                checked_at: None,
                stale: false,
                checking: true,
            })
    }

    /// A read came back: the git part as found, the protection as read (the last value kept,
    /// marked stale, when it could not be read again for the same remote branch); shown, and
    /// remembered for a restart.
    fn remote_read(
        &mut self,
        project: &str,
        mut found: crate::publish::Remote,
        read: Option<crate::publish::Read>,
    ) {
        let Some(slot) = self.remotes.get_mut(project) else {
            return;
        };
        if found.same_place(&slot.0) {
            found.protected = slot.0.protected.clone();
            found.account = slot.0.account.clone();
            found.checked_at = slot.0.checked_at.clone();
            found.stale = slot.0.stale;
            found.why = slot.0.why.clone();
        }
        if let Some(read) = read {
            slot.2 = if read.protected.is_ok() {
                0
            } else {
                slot.2 + 1
            };
            if found.state == "checking" {
                // Not even the git part could be read (a thread that could not start).
                found.state = "unreadable".into();
            }
            if !found.known() {
                found.protected = "unknown".into();
            }
            found.apply(read, super::time::rfc3339(super::time::unix_now()));
        }
        found.checking = false;
        slot.0 = found;
        let seen = slot.0.clone();
        // Only the project's own branch is remembered (the key's branch is the checked-out one).
        if let Some((id, branch)) = project.split_once('\n') {
            let shown = self
                .studio
                .store
                .get(id)
                .is_some_and(|p| git::branch(&p.path).as_deref() == Some(branch));
            if shown {
                let id = id.to_string();
                self.remember_remote(&id, seen);
            }
        }
        self.bus.publish(json!({ "type": "studio" }));
    }

    /// Kept in projects.json when what it says changed (not merely when it was read again).
    fn remember_remote(&mut self, project: &str, seen: crate::publish::Remote) {
        let Some(p) = self.studio.store.get_mut(project) else {
            return;
        };
        let same = p.remote_seen.as_ref().is_some_and(|old| {
            old.same_place(&seen)
                && old.protected == seen.protected
                && old.why == seen.why
                && old.stale == seen.stale
                && old.account == seen.account
        });
        if !same {
            p.remote_seen = Some(seen);
            if let Err(e) = self.studio.store.save() {
                self.console.push(format!("project {project}: {e}"));
            }
        }
    }

    /// 010: a publication that Fallow stopping cut short is settled from the remote (research
    /// R-8): a pull request from the loop's branch counts, never a second one; none puts the loop
    /// back to ready, saying so.
    fn reconcile_publications(&mut self) {
        let ids: Vec<u64> = self
            .runs
            .iter()
            .filter(|(_, r)| {
                r.state.stage == Stage::Ready
                    && r.state
                        .feature_loop
                        .as_ref()
                        .is_some_and(|l| l.publish_pending)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(p) = self.loop_project(id) else {
                continue;
            };
            let Some(r) = self.runs.get_mut(&id) else {
                continue;
            };
            let target = r.state.target.clone();
            let (branch, commit) = (
                r.state
                    .feature_loop
                    .as_ref()
                    .map(|l| l.build_branch.clone())
                    .unwrap_or_default(),
                r.state.integrated.clone().unwrap_or_default(),
            );
            r.busy = Some(Busy::Publish);
            let (tx, settings, dir) = (
                self.ctx.tx.clone(),
                self.ctx.settings.clone(),
                p.path.clone(),
            );
            let spawned = std::thread::Builder::new()
                .name(format!("reconcile-{id}"))
                .spawn(move || {
                    let interrupted = || crate::publish::Failed {
                        moved_to: None,
                        step: "pull_request".into(),
                        why: "publishing was interrupted when Fallow stopped; try again".into(),
                        pushed: None,
                    };
                    let found = crate::publish::remote::find(&dir, &target);
                    let result = (|| {
                        let (Some(name), Some(service), Some(repo), Some(t)) =
                            (&found.remote, &found.service, &found.repo, &found.target)
                        else {
                            return Err(interrupted());
                        };
                        let host = crate::publish::host::host_for(service, &settings)
                            .ok_or_else(interrupted)?;
                        for n in 1..=9 {
                            let b = if n == 1 {
                                branch.clone()
                            } else {
                                format!("{branch}-{n}")
                            };
                            // Only this loop's: its head at exactly the verified commit.
                            if let Ok(Some((
                                pr,
                                crate::publish::host::PrStatus::Open
                                | crate::publish::host::PrStatus::Merged,
                                Some(head),
                            ))) = host.find(&dir, repo, &b)
                            {
                                if head != commit {
                                    continue;
                                }
                                return Ok(crate::publish::Done {
                                    remote: name.clone(),
                                    service: service.clone(),
                                    repo: repo.slug(),
                                    branch: b,
                                    commit: commit.clone(),
                                    target: t.clone(),
                                    pr,
                                });
                            }
                        }
                        // What the push left on the remote, named (FR-012).
                        let mut f = interrupted();
                        f.pushed = crate::publish::push::pushed_at(&dir, name, &branch, &commit)
                            .map(|b| (b, commit.clone()));
                        Err(f)
                    })();
                    let _ = tx.send(Msg::Done {
                        run: id,
                        done: Done::Publish {
                            by: "engineer".into(),
                            result,
                        },
                    });
                });
            if spawned.is_err() {
                if let Some(r) = self.runs.get_mut(&id) {
                    r.busy = None;
                }
            }
        }
    }

    /// 010: the project of a feature loop.
    fn loop_project(&self, id: u64) -> Option<crate::project::Project> {
        let r = self.runs.get(&id)?;
        let l = r.state.feature_loop.as_ref()?;
        self.studio.store.get(&l.project).cloned()
    }

    /// 010: the remote of a loop's own target (never merely the branch checked out).
    fn loop_remote(&mut self, id: u64, now: bool) -> Option<crate::publish::Remote> {
        let p = self.loop_project(id)?;
        let target = self.runs.get(&id)?.state.target.clone();
        Some(self.remote_at(&p, &target, now, false))
    }

    /// 011: reads the loop's target on a thread when due (research R-1): every 30 s for a ready
    /// loop, at most every 10 s when `now` (its page is read), and at once after its verified
    /// commit changed.
    fn watch(&mut self, id: u64, now: bool) {
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        if r.state.stage != Stage::Ready || l.update.is_some() {
            return;
        }
        let Some(integrated) = r.state.integrated.clone() else {
            return;
        };
        let w = self.watched.entry(id).or_default();
        if w.integrated != integrated {
            *w = Watched {
                integrated: integrated.clone(),
                ..Watched::default()
            };
        }
        let every = if now {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(30)
        };
        if w.running || w.asked.is_some_and(|t| t.elapsed() < every) {
            return;
        }
        w.asked = Some(Instant::now());
        w.running = true;
        // Whose merge each target commit is: the other loops' merges and verified commits.
        let mut merges: Vec<(String, String)> = Vec::new();
        for (oid, o) in &self.runs {
            if *oid == id || o.state.repo != r.state.repo {
                continue;
            }
            if let Some(ol) = o.state.feature_loop.as_ref() {
                for c in o.state.merged.iter().chain(o.state.integrated.iter()) {
                    merges.push((c.clone(), ol.feature.clone()));
                }
                if let Some(p) = &ol.publication {
                    merges.push((p.commit.clone(), ol.feature.clone()));
                }
            }
        }
        let (tx, repo, feature, target, reconciled, scratch) = (
            self.ctx.tx.clone(),
            r.state.repo.clone(),
            l.feature.clone(),
            r.state.target.clone(),
            l.reconciled_target.clone(),
            r.worktrees(),
        );
        let spawned = std::thread::Builder::new()
            .name(format!("watch-{id}"))
            .spawn(move || {
                let result = super::update::watch(
                    &repo,
                    &feature,
                    &target,
                    &integrated,
                    reconciled.as_deref(),
                    &merges,
                    &scratch,
                );
                let _ = tx.send(Msg::Done {
                    run: id,
                    done: Done::Moved { integrated, result },
                });
            });
        if spawned.is_err() {
            if let Some(w) = self.watched.get_mut(&id) {
                w.running = false;
            }
        }
    }

    /// 011: the files the loop's target would conflict in, as the watch last read it for the
    /// commit the loop verified (empty when it did not move, moved cleanly, or was not read).
    fn conflicts_known(&self, id: u64) -> Vec<String> {
        let (Some(r), Some(w)) = (self.runs.get(&id), self.watched.get(&id)) else {
            return Vec::new();
        };
        w.read
            .as_ref()
            .filter(|m| m.moved && r.state.integrated.as_deref() == Some(w.integrated.as_str()))
            .map(|m| m.conflicts.clone())
            .unwrap_or_default()
    }

    /// 011: whether the watch found the loop's target branch gone.
    fn target_gone(&self, id: u64) -> bool {
        self.watched
            .get(&id)
            .and_then(|w| w.read.as_ref())
            .is_some_and(|m| m.gone)
    }

    /// 011: the loop view's `target` and `offer` (contracts/web-api.md).
    fn target_view(&self, id: u64, publish: &Value) -> (Value, Value) {
        let Some(r) = self.runs.get(&id) else {
            return (Value::Null, json!([]));
        };
        let l = r.state.feature_loop.as_ref();
        // 011 (research R-7): a loop with its pull request open: the remote target, as the pull
        // request shows it; never "stop" nor "lock again" while it is open (FR-016b).
        if let Some(pb) = self
            .published(id)
            .filter(|_| r.state.stage == Stage::Publishing)
        {
            let l = l.expect("a publication belongs to a feature's loop");
            let mut offer: Vec<&str> = Vec::new();
            let at_limit = l.overtaken > 0 && l.overtaken >= l.update_limit;
            if l.update.is_none() && !r.is_busy() {
                if at_limit {
                    offer.extend(["verify_again", "leave_pr"]);
                } else if pb.conflicting || (pb.behind && pb.strict == Some(true)) {
                    offer.push("bring_up_to_date");
                }
            }
            let target = json!({
                "remote": true,
                "moved": pb.conflicting || pb.behind,
                "pr": { "conflicting": pb.conflicting, "behind": pb.behind, "required": pb.strict },
                "docs_changed": l.update.as_ref().map(|u| u.docs_changed.clone()).unwrap_or_default(),
                "target_commit": l.overtaken_tip,
            });
            return (target, json!(offer));
        }
        let w = self.watched.get(&id);
        let same = w.filter(|w| r.state.integrated.as_deref() == Some(w.integrated.as_str()));
        let read = same.and_then(|w| w.read.as_ref());
        // A read that failed: offers as before (the merge checks the target again) and the
        // page says why (review C).
        let unreadable = same.and_then(|w| w.error.clone());
        let target = match read {
            Some(m) => json!({
                "moved": m.moved, "gone": m.gone, "commits": m.commits, "conflicts": m.conflicts,
                "docs_changed": m.docs_changed, "remote": false, "target_commit": m.target_commit,
            }),
            None => json!({ "moved": false, "reading": unreadable.is_none(), "error": unreadable }),
        };
        let mut offer: Vec<&str> = Vec::new();
        let stage = r.state.stage;
        let updating = l.is_some_and(|l| l.update.is_some());
        let held = l
            .and_then(|l| l.update.as_ref())
            .is_some_and(|u| !u.docs_changed.is_empty());
        if held {
            offer.extend(["lock_again", "stop"]);
        } else if stage == Stage::Ready && !updating && read.is_none() && unreadable.is_none() {
            // The target is being read: nothing is offered to merge until it is (scenario 1).
            offer.push("stop");
        } else if stage == Stage::Ready && !updating {
            match read {
                // The target branch is gone: nothing is merged; stop, or unlock and lock the
                // feature from another branch (spec edge case).
                Some(m) if m.gone => offer.push("stop"),
                Some(m) if m.moved && !m.docs_changed.is_empty() => {
                    offer.extend(["lock_again", "stop"])
                }
                Some(m) if m.moved => offer.extend(["bring_up_to_date", "stop"]),
                _ => {
                    for o in publish["offer"].as_array().into_iter().flatten() {
                        match o.as_str() {
                            Some("local_merge") => offer.push("merge"),
                            // Not while the target is unreadable: a publication does not
                            // check the target again yet (review L3).
                            Some("pull_request") if unreadable.is_none() => offer.push("publish"),
                            _ => {}
                        }
                    }
                    offer.push("stop");
                }
            }
        } else if !stage.is_final() && stage != Stage::Publishing {
            offer.push("stop");
        }
        (target, json!(offer))
    }

    /// 011: "Lock again" (FR-005): the feature is unlocked (its loop waits, the update dropped
    /// at the new lock) and the page opens its spec session with this message pre-filled: the
    /// target's version of the changed documents against the loop's, for the engineer and the
    /// lead to bring in, then lock again.
    fn lock_again(&mut self, id: u64) -> Result<Value, String> {
        let r = self
            .runs
            .get(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        if r.state.stage == Stage::Publishing || r.busy == Some(Busy::Publish) {
            return Err("its pull request is open: close it on the remote first (FR-016b)".into());
        }
        let l = r
            .state
            .feature_loop
            .as_ref()
            .ok_or("only a feature's loop")?;
        let held = l.update.as_ref().filter(|u| !u.docs_changed.is_empty());
        let read = self
            .watched
            .get(&id)
            .and_then(|w| w.read.as_ref())
            .filter(|m| m.moved && !m.docs_changed.is_empty());
        let (tip, paths) = match (held, read) {
            (Some(u), _) => (u.target_commit.clone(), u.docs_changed.clone()),
            (None, Some(m)) => (m.target_commit.clone(), m.docs_changed.clone()),
            (None, None) => return Err("the target did not change this feature's documents".into()),
        };
        let repo = r.state.repo.clone();
        let ours = git::rev(&repo, &format!("refs/heads/{}", l.build_branch))?;
        let (project, feature, target) =
            (l.project.clone(), l.feature.clone(), r.state.target.clone());
        let mut msg = format!(
            "Fallow: {target} changed this feature's documents since the loop was locked. Bring in what should stay, then lock again.\n"
        );
        for p in &paths {
            let diff = git::diff_between(&repo, &ours, &tip, p).unwrap_or_default();
            msg.push_str(&format!(
                "\n{p} ({target}'s version against the locked one):\n```diff\n{}\n```\n",
                crate::agent::clip(&diff, 6_000)
            ));
        }
        if !l.unlocked {
            self.unlock(&project, &feature)?;
        }
        if let Some(r) = self.runs.get_mut(&id) {
            r.append(Event::Steering {
                kind: SteeringKind::LockAgain,
                target: Some(target),
                detail: Some(tip),
            })?;
        }
        Ok(json!({ "project": project, "feature": feature, "message": msg }))
    }

    /// 011: brings a ready loop up to date with its target (FR-004): recorded as the engineer's
    /// steering, then the update starts.
    fn update(&mut self, id: u64) -> Result<(), String> {
        if self
            .runs
            .get(&id)
            .is_some_and(|r| r.state.stage == Stage::Publishing)
        {
            return self.update_published(id);
        }
        // A publication refused because the remote target moved (FR-011a): the update merges the
        // target as the remote has it, never lines up for a local merge (review 2).
        let for_publication = self.runs.get(&id).is_some_and(|r| {
            r.state.stage == Stage::Ready
                && r.state
                    .feature_loop
                    .as_ref()
                    .is_some_and(|l| l.publish_turn || l.publish_moved.is_some())
        });
        if for_publication {
            let r = self.runs.get(&id).ok_or("no such loop")?;
            super::update::can_start(r, false)?;
            self.publish_remote(id)?;
            let r = self.runs.get_mut(&id).ok_or("no such loop")?;
            r.append(Event::Steering {
                kind: SteeringKind::Update,
                target: Some(r.state.target.clone()),
                detail: None,
            })?;
            return self.fetch_remote_target(id);
        }
        let read = self.watched.get(&id).and_then(|w| w.read.as_ref());
        if read.is_some_and(|m| m.gone) {
            return Err("the target branch is gone: stop the loop, or unlock the feature and lock it from another branch".into());
        }
        let tip = read.filter(|m| m.moved).map(|m| m.target_commit.clone());
        let tx = self.ctx.tx.clone();
        let r = self
            .runs
            .get_mut(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        let tip = match tip {
            Some(t) => t,
            None => git::rev(&r.state.repo, &format!("refs/heads/{}", r.state.target))?,
        };
        let integrated = r.state.integrated.clone().unwrap_or_default();
        if git::is_ancestor(&r.state.repo, &tip, &integrated) {
            return Err("the target has not moved since this loop was verified".into());
        }
        // Refused before anything is recorded (review 1).
        super::update::can_start(r, false)?;
        let at_limit = r
            .state
            .feature_loop
            .as_ref()
            .is_some_and(|l| l.overtaken > 0 && l.overtaken >= l.update_limit);
        r.append(Event::Steering {
            kind: SteeringKind::Update,
            target: Some(r.state.target.clone()),
            detail: Some(tip.clone()),
        })?;
        // "Verify again on the latest <target>" at the limit, or "Bring up to date" for a merge
        // asked for: the loop takes its turn again at the end of its line and its turn starts
        // the update, so the line holds while it runs and a later move counts (R-6, M1).
        let merge_pending = r
            .state
            .feature_loop
            .as_ref()
            .is_some_and(|l| l.merge_pending);
        if at_limit || merge_pending {
            // Still in line (asked a moment ago): its turn makes the update (review 3).
            let lined = r
                .state
                .feature_loop
                .as_ref()
                .is_some_and(|l| l.lined_at.is_some());
            if !lined {
                r.append(Event::Lined {
                    at_ms: super::time::unix_ms(),
                })?;
            }
            self.turns();
            return Ok(());
        }
        super::update::start(r, tx, &tip, false, false, false)?;
        self.watched.remove(&id);
        Ok(())
    }

    /// 011 (FR-013, research R-8): the unfinished loops of `project` whose changed or named
    /// files overlap the paths `feature`'s tasks name.
    fn overlap_read(&mut self, project: &str, feature: &str) -> Result<Value, String> {
        let (p, f, wd) = self.studio.feature_ctx(project, feature)?;
        // FR-016c: `[updates] overlap = false` turns the warnings off.
        let file = crate::config::squad::SQUAD_FILE;
        let text = std::fs::read_to_string(p.root_dir().join(file)).unwrap_or_default();
        if !Squad::parse(file, &text, &mut Vec::new()).updates.overlap {
            return Ok(json!({ "overlaps": [], "off": true }));
        }
        // The documents as the spec session has them (its working copy), else the project's.
        let tasks = std::fs::read_to_string(wd.join(&f.dir).join("tasks.md"))
            .or_else(|_| std::fs::read_to_string(p.path.join(&f.dir).join("tasks.md")))
            .unwrap_or_default();
        let mine: BTreeSet<String> = tasks
            .lines()
            .flat_map(crate::speckit::tasks::paths_in)
            .collect();
        let mut out = Vec::new();
        for (id, r) in &self.runs {
            let Some(l) = r.state.feature_loop.as_ref() else {
                continue;
            };
            if l.project != project || l.feature == feature || r.state.stage.is_final() {
                continue;
            }
            let theirs: BTreeSet<&String> = l.touched.iter().chain(l.task_paths.iter()).collect();
            let files: Vec<&String> = mine.iter().filter(|m| theirs.contains(m)).collect();
            if files.is_empty() {
                continue;
            }
            let named = files.iter().any(|f| l.task_paths.contains(*f));
            out.push(json!({
                "run": id, "feature": l.feature, "stage": r.state.stage, "files": files,
                "named_in_tasks": named,
            }));
        }
        Ok(json!({
            "overlaps": out,
            "forecast": "compared with the files this feature's tasks name and the files the other loops changed so far or name in their tasks",
        }))
    }

    /// 011 (FR-015): after a step of loop `id` changed files, a new overlap with another
    /// unfinished loop of the project is recorded on both, once per pair and file set.
    fn overlaps_of(&mut self, id: u64) {
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        if !l.switches.overlap || r.state.stage.is_final() || l.touched.is_empty() {
            return;
        }
        if self.overlap_seen.get(&id) == Some(&l.touched.len()) {
            return;
        }
        self.overlap_seen.insert(id, l.touched.len());
        let (project, mine) = (l.project.clone(), l.touched.clone());
        let mut found: Vec<(u64, Vec<String>)> = Vec::new();
        for (other, o) in &self.runs {
            let Some(ol) = o.state.feature_loop.as_ref() else {
                continue;
            };
            if *other == id
                || ol.project != project
                || o.state.stage.is_final()
                || !ol.switches.overlap
            {
                continue;
            }
            let known: BTreeSet<&String> = l
                .overlaps
                .iter()
                .filter(|x| x.with == *other)
                .flat_map(|x| x.files.iter())
                .collect();
            let files: Vec<String> = mine
                .iter()
                .filter(|f| ol.touched.contains(*f) && !known.contains(f))
                .cloned()
                .collect();
            if !files.is_empty() {
                found.push((*other, files));
            }
        }
        for (other, files) in found {
            if let Some(r) = self.runs.get_mut(&id) {
                let _ = r.append(Event::Overlap {
                    with: other,
                    files: files.clone(),
                });
            }
            if let Some(o) = self.runs.get_mut(&other) {
                let _ = o.append(Event::Overlap { with: id, files });
            }
        }
    }

    /// 011 (FR-014, FR-015): waits end when the other loop is merged, stopped or deleted; a
    /// waiting loop says for whom; a loop that waited from its lock and built nothing starts from
    /// the target as it is then.
    fn waits(&mut self) {
        let waiting: Vec<(u64, u64, crate::run::events::WaitFrom)> = self
            .runs
            .iter()
            // A loop that ended waits for nothing (review 3).
            .filter(|(_, r)| !r.state.stage.is_final())
            .filter_map(|(id, r)| {
                let (other, from) = r.state.feature_loop.as_ref()?.waits_for?;
                Some((*id, other, from))
            })
            .collect();
        for (id, other, from) in waiting {
            let (ended, feature, remote_merged) = match self.runs.get(&other) {
                None => (
                    Some("it was deleted".to_string()),
                    format!("loop {other}"),
                    false,
                ),
                Some(o) => {
                    let ol = o.state.feature_loop.as_ref();
                    let name = ol.map_or_else(|| format!("loop {other}"), |l| l.feature.clone());
                    // Merged through its pull request: the target moved on the remote (review 5).
                    let via_pr = ol
                        .and_then(|l| l.publication.as_ref())
                        .is_some_and(|p| p.state == crate::publish::host::PrStatus::Merged);
                    let why = match o.state.stage {
                        Stage::Done => Some(format!("{name} was merged")),
                        Stage::Stopped => Some(format!("{name} was stopped")),
                        _ => None,
                    };
                    (why, name, via_pr)
                }
            };
            let Some(why) = ended else {
                if let Some(r) = self.runs.get_mut(&id) {
                    r.waiting = Some(format!("{WAITING_FOR}{feature}"));
                }
                continue;
            };
            // The target to start from: the remote's when the other loop merged there, fetched on
            // a thread (review F4); the start and the end of the wait follow it.
            if from == crate::run::events::WaitFrom::Lock && remote_merged {
                if !self.starting.contains_key(&id) {
                    if let Ok((remote, target)) = self.publish_remote(id) {
                        if let Some(repo) = self.runs.get(&id).map(|r| r.state.repo.clone()) {
                            self.starting.insert(id, (other, why.clone()));
                            let tx = self.ctx.tx.clone();
                            let spawned = std::thread::Builder::new()
                                .name(format!("start-{id}"))
                                .spawn(move || {
                                    let r = crate::publish::fetch_target(&repo, &remote, &target);
                                    let _ = tx.send(Msg::Done {
                                        run: id,
                                        done: Done::Fetched(r),
                                    });
                                });
                            if spawned.is_ok() {
                                continue;
                            }
                            self.starting.remove(&id);
                        }
                    }
                } else {
                    continue;
                }
            }
            let tip = if from == crate::run::events::WaitFrom::Lock {
                self.runs.get(&id).and_then(|r| {
                    git::rev(&r.state.repo, &format!("refs/heads/{}", r.state.target)).ok()
                })
            } else {
                None
            };
            let Some(r) = self.runs.get_mut(&id) else {
                continue;
            };
            if r.waiting
                .as_deref()
                .is_some_and(|w| w.starts_with(WAITING_FOR))
            {
                r.waiting = None;
            }
            // The start comes first, then the end of the wait: a restart in between starts again
            // (FR-017, review 6).
            let started = match tip {
                Some(t) => start_from_target(r, &t),
                None => Ok(()),
            };
            let _ = r.append(Event::WaitEnded {
                run: other,
                why: why.clone(),
            });
            if let Err(e) = started {
                let _ = r.append(Event::NeedsEngineer {
                    why: format!(
                        "The loop could not start from the latest {}: {e}. Unlock the feature and lock it again, or stop.",
                        r.state.target
                    ),
                    options: vec!["stop".into()],
                });
            }
        }
    }

    /// 011 (FR-015): "Wait for <feature>": refused for a loop whose pull request is open
    /// (FR-016b), for itself, and when it would close a cycle.
    fn wait_for(&mut self, id: u64, other: u64) -> Result<(), String> {
        if id == other {
            return Err("a loop cannot wait for itself".into());
        }
        let r = self
            .runs
            .get(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        if r.state.stage == Stage::Publishing {
            return Err("its pull request is open: close it on the remote first (FR-016b)".into());
        }
        // While it builds (FR-015): a ready loop has nothing more to start.
        if r.state.stage != Stage::Building {
            return Err("only a loop that is building waits for another".into());
        }
        let project = r.state.feature_loop.as_ref().map(|l| l.project.clone());
        let o = self
            .runs
            .get(&other)
            .ok_or_else(|| format!("there is no loop {other}"))?;
        if o.state.stage.is_final() {
            return Err("that loop is over".into());
        }
        if o.state.feature_loop.as_ref().map(|l| l.project.clone()) != project {
            return Err("that loop is not of this project".into());
        }
        // A cycle: following the waits from the other loop reaches this one.
        let mut at = other;
        for _ in 0..64 {
            let next = self
                .runs
                .get(&at)
                .and_then(|x| x.state.feature_loop.as_ref())
                .and_then(|l| l.waits_for)
                .map(|(n, _)| n);
            match next {
                Some(n) if n == id => {
                    let name = self
                        .runs
                        .get(&at)
                        .and_then(|x| x.state.feature_loop.as_ref())
                        .map_or_else(|| format!("loop {at}"), |l| l.feature.clone());
                    return Err(format!(
                        "{name} already waits for this loop: waiting for it would close a cycle"
                    ));
                }
                Some(n) => at = n,
                None => break,
            }
        }
        let feature = o
            .state
            .feature_loop
            .as_ref()
            .map_or_else(|| format!("loop {other}"), |l| l.feature.clone());
        let r = self.runs.get_mut(&id).ok_or("no such loop")?;
        r.append(Event::Steering {
            kind: SteeringKind::WaitFor,
            target: Some(feature),
            detail: Some(other.to_string()),
        })?;
        r.append(Event::WaitsFor {
            run: other,
            from: crate::run::events::WaitFrom::Engineer,
        })?;
        self.waits();
        Ok(())
    }

    /// 011: "Stop waiting".
    fn stop_waiting(&mut self, id: u64) -> Result<(), String> {
        let r = self
            .runs
            .get_mut(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        let Some((other, _)) = r.state.feature_loop.as_ref().and_then(|l| l.waits_for) else {
            return Err("the loop is not waiting".into());
        };
        r.append(Event::Steering {
            kind: SteeringKind::StopWaiting,
            target: None,
            detail: Some(other.to_string()),
        })?;
        r.append(Event::WaitEnded {
            run: other,
            why: "you stopped waiting".into(),
        })?;
        if r.waiting
            .as_deref()
            .is_some_and(|w| w.starts_with(WAITING_FOR))
        {
            r.waiting = None;
        }
        Ok(())
    }

    /// 011 (research R-7): the remote of a published loop: its name, target, branch and the
    /// commit Fallow pushed.
    fn published(&self, id: u64) -> Option<super::state::Publication> {
        let r = self.runs.get(&id)?;
        r.state
            .feature_loop
            .as_ref()?
            .publication
            .clone()
            .filter(|p| p.state == crate::publish::host::PrStatus::Open)
    }

    /// 011: fetches the remote target of a published loop on a thread (`Done::Fetched`).
    /// 011: the remote and target branch a loop publishes to.
    fn publish_remote(&mut self, id: u64) -> Result<(String, String), String> {
        match self.published(id) {
            Some(pb) => Ok((pb.remote, pb.target)),
            None => {
                let rm = self.loop_remote(id, false).ok_or("no remote")?;
                Ok((
                    rm.remote.ok_or("no remote")?,
                    rm.target.ok_or("no target on the remote")?,
                ))
            }
        }
    }

    fn fetch_remote_target(&mut self, id: u64) -> Result<(), String> {
        let (remote, target) = self.publish_remote(id)?;
        let r = self.runs.get_mut(&id).ok_or("no such loop")?;
        r.busy = Some(Busy::Update);
        let (tx, repo) = (self.ctx.tx.clone(), r.state.repo.clone());
        std::thread::Builder::new()
            .name(format!("fetch-{id}"))
            .spawn(move || {
                let r = crate::publish::fetch_target(&repo, &remote, &target);
                let _ = tx.send(Msg::Done {
                    run: id,
                    done: Done::Fetched(r),
                });
            })
            .map(|_| ())
            .map_err(|e| {
                r.busy = None;
                format!("cannot start a thread: {e}")
            })
    }

    /// 011: brings a published loop up to date (FR-011, FR-012): the remote target is fetched,
    /// then the update runs while the loop stays publishing; refused while one runs.
    fn update_published(&mut self, id: u64) -> Result<(), String> {
        let r = self.runs.get(&id).ok_or("no such loop")?;
        super::update::can_start(r, true)?;
        self.published(id).ok_or("no open pull request")?;
        let r = self.runs.get_mut(&id).ok_or("no such loop")?;
        r.append(Event::Steering {
            kind: SteeringKind::Update,
            target: Some(r.state.target.clone()),
            detail: None,
        })?;
        self.fetch_remote_target(id)
    }

    /// 011: a published loop whose pull request conflicts, or is behind a target that requires
    /// it up to date (research R-7): Free brings itself up to date; Normal and Strict ask, never
    /// offering "stop" (FR-016b).
    fn pr_fell_behind(&mut self, id: u64) {
        let Some(pb) = self.published(id) else {
            return;
        };
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        // A verified commit not on the pull request yet is pushed first (push_forward_due).
        let unpushed = l.outcome.is_some()
            && !l.left_pr
            && r.state.integrated.as_deref() != Some(pb.commit.as_str());
        if r.state.stage != Stage::Publishing
            || l.update.is_some()
            || r.state.needs.is_some()
            || r.is_busy()
            || unpushed
            || (l.overtaken > 0 && l.overtaken >= l.update_limit)
            || !(pb.conflicting || (pb.behind && pb.strict == Some(true)))
        {
            return;
        }
        let what = if pb.conflicting {
            format!(
                "Pull request #{} conflicts with {}",
                pb.pr_number, pb.target
            )
        } else {
            format!(
                "Pull request #{} is behind {}, which requires it to be up to date",
                pb.pr_number, pb.target
            )
        };
        if update_by_itself(l) {
            if let Err(e) = self.fetch_remote_target(id) {
                if let Some(r) = self.runs.get_mut(&id) {
                    let _ = r.append(Event::NeedsEngineer {
                        why: format!("{what}. It could not bring itself up to date: {e}."),
                        options: vec!["bring up to date".into()],
                    });
                }
            }
            return;
        }
        if let Some(r) = self.runs.get_mut(&id) {
            let _ = r.append(Event::NeedsEngineer {
                why: format!(
                    "{what}. Bring it up to date: {} as the remote has it is merged into the loop's own branch in its own working copy, verified again, and pushed forward on the same branch (never forced).",
                    pb.target
                ),
                options: vec!["bring up to date".into()],
            });
        }
    }

    /// 011 (FR-012): a published loop whose update is verified: the remote target is checked
    /// once more, then its branch is pushed forward, on a thread (`Done::PushForward`).
    fn push_forward_due(&mut self, id: u64) {
        let Some(pb) = self.published(id) else {
            return;
        };
        let tx = self.ctx.tx.clone();
        let Some(r) = self.runs.get_mut(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        // The update's merged result, or (its update dropped after a push that failed) the
        // verified commit the pull request does not have yet (review F1); never after the pull
        // request was left as it is.
        let base = match &l.update {
            Some(u) if u.merged && u.remote => Some(u.target_commit.clone()),
            Some(_) => return,
            None => None,
        };
        let Some(to) = r.state.integrated.clone() else {
            return;
        };
        if r.state.stage != Stage::Publishing
            || l.outcome.is_none()
            || r.state.needs.is_some()
            || r.is_busy()
            || to == pb.commit
            || (base.is_none() && l.left_pr)
        {
            return;
        }
        let (repo, from) = (r.state.repo.clone(), pb.commit.clone());
        let with_update = base.is_some();
        r.busy = Some(Busy::Publish);
        let spawned = std::thread::Builder::new()
            .name(format!("push-forward-{id}"))
            .spawn(move || {
                let result = match crate::publish::fetch_target(&repo, &pb.remote, &pb.target) {
                    Err(e) => Err(Err(e)),
                    // FR-011a: moved since the update started, nothing is pushed.
                    Ok(at)
                        if base.as_deref() != Some(at.as_str())
                            && !git::is_ancestor(&repo, &at, &to) =>
                    {
                        Err(Ok(at))
                    }
                    Ok(_) => crate::publish::push::push_forward(
                        &repo, &pb.remote, &pb.branch, &from, &to, &pb.target,
                    )
                    .map_err(Err),
                };
                let _ = tx.send(Msg::Done {
                    run: id,
                    done: Done::PushForward {
                        from,
                        to,
                        result,
                        with_update,
                    },
                });
            });
        if spawned.is_err() {
            r.busy = None;
        }
    }

    /// 011: what a published loop's fetch, push forward, or a first publication refused because
    /// the remote target moved, leads to.
    fn after_remote(&mut self, id: u64, a: After) {
        // 011 (FR-014): the remote target fetched for a loop that ends its wait.
        if let (Some((other, why)), After::Fetched(fetched)) = (self.starting.remove(&id), &a) {
            let Some(r) = self.runs.get_mut(&id) else {
                return;
            };
            // Still waiting for that loop: the engineer may have stopped waiting, stopped the
            // loop or unlocked it meanwhile (review N1).
            let still = r
                .state
                .feature_loop
                .as_ref()
                .is_some_and(|l| l.waits_for.is_some_and(|(o, _)| o == other) && !l.unlocked)
                && !r.state.stage.is_final();
            if !still {
                return;
            }
            if r.waiting
                .as_deref()
                .is_some_and(|w| w.starts_with(WAITING_FOR))
            {
                r.waiting = None;
            }
            let local = git::rev(&r.state.repo, &format!("refs/heads/{}", r.state.target)).ok();
            let tip = match fetched {
                Ok(sha) => Some(sha.clone()),
                Err(e) => {
                    let _ = r.append(Event::Note {
                        text: format!(
                            "{} could not be read on the remote ({e}): the loop starts from the local {}, which may not have the other loop's merge yet.",
                            r.state.target, r.state.target
                        ),
                    });
                    local
                }
            };
            let started = match tip {
                Some(t) => start_from_target(r, &t),
                None => Ok(()),
            };
            let _ = r.append(Event::WaitEnded { run: other, why });
            if let Err(e) = started {
                let _ = r.append(Event::NeedsEngineer {
                    why: format!(
                        "The loop could not start from the latest {}: {e}. Unlock the feature and lock it again, or stop.",
                        r.state.target
                    ),
                    options: vec!["stop".into()],
                });
            }
            return;
        }
        let tx = self.ctx.tx.clone();
        let open = self.published(id).is_some();
        let Some(r) = self.runs.get_mut(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.clone() else {
            return;
        };
        let target = r.state.target.clone();
        let stage = r.state.stage;
        // A published loop's work counts only while its pull request is open (review 9).
        let published = stage == Stage::Publishing && open;
        let first = stage == Stage::Ready && (l.publish_turn || l.publish_moved.is_some());
        let result = match a {
            After::Fetched(_) if !published && !first => Ok(()),
            After::Fetched(Err(e)) => r.append(Event::NeedsEngineer {
                why: format!("{target} could not be read on the remote: {e}"),
                options: vec!["bring up to date".into()],
            }),
            After::Fetched(Ok(sha)) => {
                let integrated = r.state.integrated.clone().unwrap_or_default();
                let unpushed = published
                    && l.publication
                        .as_ref()
                        .is_some_and(|p| p.commit != integrated)
                    && l.outcome.is_some()
                    && !l.left_pr;
                if git::is_ancestor(&r.state.repo, &sha, &integrated) {
                    if unpushed {
                        // The push forward goes on at the next pass (review F1).
                        Ok(())
                    } else if published {
                        // The pull request still says otherwise: said, not tried again (10).
                        r.append(Event::NeedsEngineer {
                            why: format!(
                                "The loop holds {target} as the remote has it; the pull request may need a moment to show it."
                            ),
                            options: vec!["bring up to date".into()],
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    super::update::start(r, tx, &sha, published, false, first)
                }
            }
            After::Pushed { .. } if !published => Ok(()),
            After::Pushed {
                from,
                to,
                result: Ok(()),
                ..
            } => r.append(Event::PushedForward { from, to }),
            After::Pushed {
                result: Err(Err(why)),
                ..
            } => {
                // Nothing pushed: the update is dropped, so it can be made again (review 3).
                super::update::drop_update(r, "nothing was pushed").and_then(|()| {
                    r.append(Event::NeedsEngineer {
                        why: format!("Nothing was pushed: {why}."),
                        options: vec!["bring up to date".into()],
                    })
                })
            }
            After::Pushed {
                result: Err(Ok(sha)),
                with_update: false,
                ..
            } => {
                // Its update was gone: the remote target moved while the loop waited, which is
                // no overtaking (FR-007); asked or made as any move is (FR-010, review N1).
                if let Some((which, used)) = r.state.limit_hit(super::time::unix_now()) {
                    r.append(Event::NeedsEngineer {
                        why: format!(
                            "{target} moved on the remote: nothing was pushed. The {which} limit is used up ({used}): raise it to bring the loop up to date."
                        ),
                        options: vec!["raise the limit".into()],
                    })
                } else if update_by_itself(&l) {
                    super::update::start(r, tx, &sha, true, false, false)
                } else {
                    r.append(Event::NeedsEngineer {
                        why: format!(
                            "{target} moved on the remote: nothing was pushed. Bring it up to date: it is merged into the loop's own branch in its own working copy, verified again, and pushed forward on the same branch."
                        ),
                        options: vec!["bring up to date".into()],
                    })
                }
            }
            After::Pushed {
                result: Err(Ok(sha)),
                ..
            } => {
                // FR-007: any move of the remote target during a publication's update counts.
                let at_limit = l.overtaken + 1 >= l.update_limit;
                r.append(Event::Overtaken {
                    n: l.updates,
                    target_commit: sha.clone(),
                })
                .and_then(|()| super::update::drop_update(r, "overtaken on the remote"))
                .and_then(|()| {
                    if at_limit {
                        // The update is dropped first, so "Verify again" can start (review 4).
                        r.append(Event::NeedsEngineer {
                            why: format!(
                                "{}: updated {} times and overtaken {} in a row by changes of {target} on the remote; your decision",
                                l.feature,
                                l.updates,
                                l.overtaken + 1
                            ),
                            options: vec![
                                format!("verify again on the latest {target}"),
                                "leave the pull request as it is".into(),
                            ],
                        })
                    } else if let Some((which, used)) = r.state.limit_hit(super::time::unix_now())
                    {
                        r.append(Event::NeedsEngineer {
                            why: format!(
                                "{target} moved on the remote again: nothing was pushed. The {which} limit is used up ({used}): raise it to go on."
                            ),
                            options: vec!["raise the limit".into()],
                        })
                    } else {
                        super::update::start(r, tx, &sha, true, false, false)
                    }
                })
            }
            After::PublishMoved(sha) => {
                let counted = l.publish_turn;
                let at_limit = counted && l.overtaken + 1 >= l.update_limit;
                let appended = if counted {
                    r.append(Event::Overtaken {
                        n: l.updates,
                        target_commit: sha.clone(),
                    })
                } else {
                    Ok(())
                };
                match appended {
                    Err(e) => Err(e),
                    Ok(()) if at_limit => {
                        let integrated = r.state.integrated.clone().unwrap_or_default();
                        let clean = git::merge_tree(&r.state.repo, &integrated, &sha)
                            .is_ok_and(|m| m.conflicts.is_empty());
                        let mut options = vec![format!("verify again on the latest {target}")];
                        if clean {
                            options.push("publish without the latest check".into());
                        }
                        options.push("stop".into());
                        r.append(Event::NeedsEngineer {
                            why: format!(
                                "{}: updated {} times and overtaken {} in a row by changes of {target} on the remote; your decision",
                                l.feature,
                                l.updates,
                                l.overtaken + 1
                            ),
                            options,
                        })
                    }
                    // Free brings itself up to date for the publication; an update the
                    // engineer already started for it goes on (review 5).
                    Ok(()) if r.state.limit_hit(super::time::unix_now()).is_some() => {
                        let (which, used) =
                            r.state.limit_hit(super::time::unix_now()).unwrap_or_default();
                        r.append(Event::NeedsEngineer {
                            why: format!(
                                "{target} on the remote moved: nothing was pushed. The {which} limit is used up ({used}): raise it to bring the loop up to date, or stop."
                            ),
                            options: vec!["raise the limit".into(), "stop".into()],
                        })
                    }
                    Ok(()) if update_by_itself(&l) || counted => {
                        super::update::start(r, tx, &sha, false, false, true)
                    }
                    // Normal and Strict ask first (FR-010).
                    Ok(()) => r.append(Event::NeedsEngineer {
                        why: format!(
                            "{target} on the remote moved since this loop was verified: nothing was pushed. Bring it up to date: {target} as the remote has it is merged into the loop's own branch in its own working copy, then the gates and every story are checked again, and the publication goes on."
                        ),
                        options: vec!["bring up to date".into(), "stop".into()],
                    }),
                }
            }
        };
        if let Err(e) = result {
            self.console.push(format!("run {id}: {e}"));
        }
    }

    /// 011: a loop brought up to date for a publication publishes again once verified.
    fn publish_again(&mut self, id: u64) {
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        if !l.publish_turn
            || r.state.stage != Stage::Ready
            || l.outcome.is_none()
            || l.update.is_some()
            || r.state.needs.is_some()
            || r.is_busy()
            || l.unlocked
        {
            return;
        }
        if let Err(e) = self.publish(id, "engineer") {
            if let Some(r) = self.runs.get_mut(&id) {
                // Recorded as a failed publication: its turn ends (review F2).
                let _ = r.append(Event::PublishFailed {
                    step: "check".into(),
                    why: e.clone(),
                    pushed: None,
                    by: "engineer".into(),
                    moved_to: None,
                });
                let _ = r.append(Event::NeedsEngineer {
                    why: format!("The publication could not go on: {e}"),
                    options: vec!["stop".into()],
                });
            }
        }
    }

    /// 011 FR-009: "Publish without the latest check", for a first publication at the limit.
    fn publish_unchecked(&mut self, id: u64) -> Result<(), String> {
        let r = self.runs.get(&id).ok_or("no such loop")?;
        let l = r
            .state
            .feature_loop
            .as_ref()
            .ok_or("only a feature's loop")?;
        if !(l.publish_turn && l.overtaken > 0 && l.overtaken >= l.update_limit) {
            return Err("publishing without the latest check is offered only at the limit of overtaken updates".into());
        }
        let tip = l
            .overtaken_tip
            .clone()
            .ok_or("the target commit is not known")?;
        let integrated = r.state.integrated.clone().ok_or("nothing is integrated")?;
        if !git::merge_tree(&r.state.repo, &integrated, &tip)?
            .conflicts
            .is_empty()
        {
            return Err(format!(
                "it would conflict with {}: verify again on the latest instead",
                r.state.target
            ));
        }
        let unchecked = git::first_parent_commits(&r.state.repo, &integrated, &tip);
        self.publish_with(id, "engineer", unchecked)
    }

    /// 011 FR-009: "Leave the pull request as it is" at the limit: the pull request stays as
    /// published, recorded with the target's commits it was not verified with.
    fn leave_pr(&mut self, id: u64) -> Result<(), String> {
        let pb = self.published(id).ok_or("no open pull request")?;
        let r = self.runs.get_mut(&id).ok_or("no such loop")?;
        let l = r
            .state
            .feature_loop
            .clone()
            .ok_or("only a feature's loop")?;
        if !(l.overtaken > 0 && l.overtaken >= l.update_limit) {
            return Err("offered only at the limit of overtaken updates".into());
        }
        if r.is_busy() {
            return Err("a step or a push is running".into());
        }
        let tip = l
            .overtaken_tip
            .clone()
            .ok_or("the target commit is not known")?;
        let unchecked = git::first_parent_commits(&r.state.repo, &pb.commit, &tip);
        r.append(Event::Steering {
            kind: SteeringKind::LeavePullRequest,
            target: Some(r.state.target.clone()),
            detail: Some(pb.commit.clone()),
        })?;
        super::update::drop_update(r, "the pull request is left as it is")?;
        r.append(Event::LeftPullRequest { unchecked })
    }

    /// 011 (research R-4, FR-010): the watch found the target moved for a ready loop, outside
    /// its merge line: a Free loop brings itself up to date, unless a limit is used up; Normal and
    /// Strict (or `[updates] auto = false`) ask first; documents changed on the target ask whatever
    /// the level. A question stands until the engineer acts; then the next read decides again.
    fn target_moved_for(&mut self, id: u64) {
        let Some(m) = self
            .watched
            .get(&id)
            .filter(|w| {
                self.runs
                    .get(&id)
                    .is_some_and(|r| r.state.integrated.as_deref() == Some(w.integrated.as_str()))
            })
            .and_then(|w| w.read.clone())
        else {
            return;
        };
        if !m.moved || m.gone {
            return;
        }
        let tx = self.ctx.tx.clone();
        let Some(r) = self.runs.get_mut(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.clone() else {
            return;
        };
        if r.state.stage != Stage::Ready
            || l.outcome.is_none()
            || l.update.is_some()
            || l.lined_at.is_some()
            || l.unlocked
            || r.state.needs.is_some()
            || r.is_busy()
            // The limit's own question decides (L3).
            || (l.overtaken > 0 && l.overtaken >= l.update_limit)
        {
            return;
        }
        let target = r.state.target.clone();
        if !m.docs_changed.is_empty() {
            // FR-005: nothing merged, no builder; the engineer locks again or stops (L1).
            let _ = r.append(Event::NeedsEngineer {
                why: format!(
                    "{target} changed this feature's documents since the loop was locked ({}): nothing is merged. Lock again with them, or stop.",
                    m.docs_changed.join(", ")
                ),
                options: vec!["lock again".into(), "stop".into()],
            });
            return;
        }
        let n = m.commits.len();
        let moved = format!(
            "{target} moved since this loop was verified ({n} commit{}{})",
            if n == 1 { "" } else { "s" },
            if m.conflicts.is_empty() {
                String::new()
            } else {
                format!(", conflicting in {}", m.conflicts.join(", "))
            }
        );
        let ask = |r: &mut Run, why: String| {
            r.append(Event::NeedsEngineer {
                why,
                options: vec!["bring up to date".into(), "stop".into()],
            })
        };
        let result = if let Some((which, used)) = r.state.limit_hit(super::time::unix_now()) {
            // FR-010: a limit used up asks whatever the level, naming it.
            r.append(Event::NeedsEngineer {
                why: format!(
                    "{moved}. The {which} limit is used up ({used}): raise it to bring the loop up to date, or stop."
                ),
                options: vec!["raise the limit".into(), "stop".into()],
            })
        } else if update_by_itself(&l) {
            match super::update::start(r, tx, &m.target_commit, false, false, false) {
                Ok(()) => Ok(()),
                // A start that failed says why instead of trying at every read (L2).
                Err(e) => ask(
                    r,
                    format!("{moved}. It could not bring itself up to date: {e}."),
                ),
            }
        } else {
            ask(
                r,
                format!(
                    "{moved}. Bring it up to date: {target} is merged into the loop's own branch in its own working copy{}, then the gates and every story are checked again.",
                    if m.conflicts.is_empty() {
                        ""
                    } else {
                        ", a builder resolves the conflicts"
                    }
                ),
            )
        };
        if let Err(e) = result {
            self.console.push(format!("run {id}: {e}"));
        }
    }

    /// 011: a merge the engineer asked for (FR-009a, research R-5): it joins its target's merge
    /// line and runs, checked again, when its turn comes; "without the latest check" only at the
    /// limit of overtaken updates and when it would not conflict (FR-009).
    fn merge_request(&mut self, id: u64, without_latest_check: bool) -> Result<Value, String> {
        let tx = self.ctx.tx.clone();
        let r = self
            .runs
            .get_mut(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        let Some(l) = r.state.feature_loop.clone() else {
            // Not a feature's loop: merged as before.
            return super::machine::merge(r, tx, Vec::new()).map(|()| json!({}));
        };
        if r.state.stage != Stage::Ready {
            return Err("the run is not ready to merge".into());
        }
        if l.unlocked {
            return Err("the feature is unlocked: lock it again before merging".into());
        }
        if l.outcome.is_none() {
            return Err("the loop has not finished checking this result".into());
        }
        if l.update.is_some() {
            return Err("the loop is being brought up to date".into());
        }
        if r.is_busy() {
            return Err("a merge or a step is already running".into());
        }
        let integrated = r.state.integrated.clone().ok_or("nothing is integrated")?;
        if without_latest_check {
            if l.overtaken < l.update_limit {
                return Err("merging without the latest check is offered only at the limit of overtaken updates".into());
            }
            let repo = r.state.repo.clone();
            let tip = git::rev(&repo, &format!("refs/heads/{}", r.state.target))?;
            let conflicts = git::merge_tree(&repo, &integrated, &tip)?.conflicts;
            if !conflicts.is_empty() {
                return Err(format!(
                    "it would conflict in {}: verify again on the latest {} instead",
                    conflicts
                        .iter()
                        .map(|c| c.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    r.state.target
                ));
            }
            // The target's commits the result was not verified with (constitution I: said).
            let unchecked = git::first_parent_commits(&repo, &integrated, &tip);
            r.append(Event::Steering {
                kind: SteeringKind::MergeUnchecked,
                target: Some(r.state.target.clone()),
                detail: Some(integrated),
            })?;
            if l.lined_at.is_some() {
                r.append(Event::Unlined {
                    why: "merging without the latest check".into(),
                })?;
            }
            return super::machine::merge(r, tx, unchecked).map(|()| json!({}));
        }
        r.append(Event::Steering {
            kind: SteeringKind::Merge,
            target: Some(r.state.target.clone()),
            detail: Some(integrated),
        })?;
        if l.lined_at.is_none() {
            r.append(Event::Lined {
                at_ms: super::time::unix_ms(),
            })?;
        }
        self.turns();
        Ok(match self.line_of(id) {
            Some((_, Some(after))) => json!({ "in_line_after": after }),
            _ => json!({}),
        })
    }

    /// 011: the merge lines (research R-5): per repository and target, the loops whose merge was
    /// asked for, by when, then run id; a loop alone when its line switch is off.
    fn lines(&self) -> BTreeMap<String, Vec<(String, u64)>> {
        let mut lines: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
        for (id, r) in &self.runs {
            let Some(l) = r.state.feature_loop.as_ref() else {
                continue;
            };
            let Some(at) = l.lined_at.clone() else {
                continue;
            };
            let mut key = format!("{}\n{}", r.state.repo.display(), r.state.target);
            if !l.switches.line {
                key.push_str(&format!("\n{id}"));
            }
            lines.entry(key).or_default().push((at, *id));
        }
        for line in lines.values_mut() {
            line.sort();
        }
        lines
    }

    /// 011: the loop's place in its line (0 holds the turn) and the feature right before it.
    fn line_of(&self, id: u64) -> Option<(usize, Option<String>)> {
        self.lines().into_values().find_map(|line| {
            let pos = line.iter().position(|(_, i)| *i == id)?;
            let before = pos
                .checked_sub(1)
                .and_then(|p| self.runs.get(&line[p].1))
                .and_then(|r| r.state.feature_loop.as_ref())
                .map(|l| l.feature.clone());
            Some((pos, before))
        })
    }

    /// 011: the lines move on (FR-009a): a loop waiting for anything leaves its line; the first
    /// of each line takes its turn; the others wait, saying after whom.
    fn turns(&mut self) {
        let ids: Vec<u64> = self
            .runs
            .iter()
            .filter(|(_, r)| {
                r.state
                    .feature_loop
                    .as_ref()
                    .is_some_and(|l| l.lined_at.is_some())
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(r) = self.runs.get_mut(&id) else {
                continue;
            };
            let l = r.state.feature_loop.as_ref();
            let why = if r.state.stage.is_final() {
                Some("ended")
            } else if r.state.stage == Stage::Publishing || r.busy == Some(Busy::Publish) {
                Some("publishing")
            } else if r.state.needs.is_some() {
                Some("waiting for you")
            } else if super::feature::paused_by_engineer(r) {
                Some("paused")
            } else if l.is_some_and(|l| l.unlocked) {
                Some("unlocked")
            } else if r.state.limit_reached.is_some() {
                Some("at its limit")
            } else if l.is_some_and(|l| l.waits_for.is_some()) {
                Some("waiting for another loop")
            } else {
                None
            };
            if let Some(why) = why {
                if r.waiting.as_deref().is_some_and(|w| w.starts_with(IN_LINE)) {
                    r.waiting = None;
                }
                let _ = r.append(Event::Unlined { why: why.into() });
            }
        }
        // A merge asked for and not made: when the loop goes on after a wait, it takes its turn
        // again at the end of its line (FR-009a).
        let back: Vec<u64> = self
            .runs
            .iter()
            .filter(|(_, r)| {
                r.state.feature_loop.as_ref().is_some_and(|l| {
                    l.merge_pending
                        && l.lined_at.is_none()
                        && l.update.is_none()
                        && l.outcome.is_some()
                        && !l.unlocked
                        && l.waits_for.is_none()
                }) && r.state.stage == Stage::Ready
                    && r.state.needs.is_none()
                    && r.state.limit_reached.is_none()
                    && !super::feature::paused_by_engineer(r)
                    && !r.is_busy()
            })
            .map(|(id, _)| *id)
            .collect();
        for id in back {
            if let Some(r) = self.runs.get_mut(&id) {
                let _ = r.append(Event::Lined {
                    at_ms: super::time::unix_ms(),
                });
            }
        }
        for line in self.lines().into_values() {
            let mut before: Option<String> = None;
            for (pos, (_, id)) in line.iter().enumerate() {
                let feature = self
                    .runs
                    .get(id)
                    .and_then(|r| r.state.feature_loop.as_ref())
                    .map(|l| l.feature.clone())
                    .unwrap_or_default();
                if pos == 0 {
                    if let Some(r) = self.runs.get_mut(id) {
                        if r.waiting.as_deref().is_some_and(|w| w.starts_with(IN_LINE)) {
                            r.waiting = None;
                        }
                    }
                    self.take_turn(*id);
                } else if let Some(r) = self.runs.get_mut(id) {
                    r.waiting = Some(format!("{IN_LINE}{}", before.clone().unwrap_or_default()));
                }
                before = Some(feature);
            }
        }
    }

    /// 011: the loop holding its line's turn: merges what it verified when the target has not
    /// moved (the merge checks it once more); when it moved, it is brought up to date first, and
    /// a move after an update made in this turn is an overtaken update, up to the lock's limit,
    /// where the engineer decides (FR-007, research R-6).
    fn take_turn(&mut self, id: u64) {
        let tx = self.ctx.tx.clone();
        let Some(r) = self.runs.get_mut(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.clone() else {
            return;
        };
        if r.is_busy()
            || l.update.is_some()
            || r.state.stage != Stage::Ready
            || l.outcome.is_none()
            || r.state.needs.is_some()
        {
            // Being brought up to date, merging, or repairing after its update: it holds.
            return;
        }
        let repo = r.state.repo.clone();
        let target = r.state.target.clone();
        let Some(integrated) = r.state.integrated.clone() else {
            return;
        };
        let tip = match git::rev(&repo, &format!("refs/heads/{target}")) {
            Ok(t) => t,
            Err(e) => {
                let _ = r.append(Event::NeedsEngineer {
                    why: format!("{target} cannot be read: {e}. Nothing was merged."),
                    options: vec!["merge again".into(), "stop".into()],
                });
                return;
            }
        };
        let result = if git::is_ancestor(&repo, &tip, &integrated) {
            super::machine::merge(r, tx, Vec::new())
        } else {
            // The engineer's own "bring up to date" is no overtaking (phase 5 review L2).
            let counted = (!l.switches.line || l.turn_updates > 0) && !l.update_asked;
            let at_limit = counted && l.overtaken + 1 >= l.update_limit;
            if counted {
                if let Err(e) = r.append(Event::Overtaken {
                    n: l.updates,
                    target_commit: tip.clone(),
                }) {
                    self.console.push(format!("run {id}: {e}"));
                    return;
                }
            }
            if at_limit {
                let conflicts = git::merge_tree(&repo, &integrated, &tip)
                    .map(|m| !m.conflicts.is_empty())
                    .unwrap_or(true);
                let mut options = vec![format!("verify again on the latest {target}")];
                if !conflicts {
                    options.push("merge without the latest check".into());
                }
                options.push("stop".into());
                let by = if l.switches.line {
                    "from outside Fallow"
                } else {
                    "(the merge line is off: other loops' merges count too)"
                };
                r.append(Event::NeedsEngineer {
                    why: format!(
                        "{}: updated {} times and overtaken {} in a row by commits on {target} {by}; your decision",
                        l.feature,
                        l.updates,
                        l.overtaken + 1
                    ),
                    options,
                })
            } else if let Some((which, used)) = r.state.limit_hit(super::time::unix_now()) {
                r.append(Event::NeedsEngineer {
                    why: format!(
                        "{target} moved before the merge. The {which} limit is used up ({used}): raise it to bring the loop up to date, or stop."
                    ),
                    options: vec!["raise the limit".into(), "stop".into()],
                })
            } else if update_by_itself(&l) || l.update_asked {
                super::update::start(r, tx, &tip, false, true, false)
            } else {
                // Normal and Strict ask first; the loop leaves the line while it waits, its merge
                // still asked for (FR-009a, FR-010).
                r.append(Event::NeedsEngineer {
                    why: format!(
                        "{target} moved before the merge. Bring it up to date: {target} is merged into the loop's own branch in its own working copy, then the gates and every story are checked again, and the merge goes on."
                    ),
                    options: vec!["bring up to date".into(), "stop".into()],
                })
            }
        };
        if let Err(e) = result {
            if let Some(r) = self.runs.get_mut(&id) {
                if r.state.needs.is_none() {
                    let _ = r.append(Event::NeedsEngineer {
                        why: format!("The merge could not go on: {e}"),
                        options: vec!["merge again".into(), "stop".into()],
                    });
                }
            }
        }
        self.watched.remove(&id);
    }

    /// 010: what publishing a loop offers, why, the remote, the account, the pull request and
    /// the last failure (contracts/web-api.md). `None` for a run that is no feature loop.
    fn publish_view(&mut self, id: u64) -> Option<Value> {
        let remote = self.loop_remote(id, false);
        let r = self.runs.get(&id)?;
        let l = r.state.feature_loop.as_ref()?;
        let (offer, why) = crate::publish::offer(remote.as_ref());
        let open = l
            .publication
            .as_ref()
            .is_some_and(|x| x.state == crate::publish::host::PrStatus::Open);
        let offer: Vec<&str> = if open { Vec::new() } else { offer };
        let read = self.pr_reads.get(&id);
        Some(json!({
            "offer": offer, "why": why, "remote": remote,
            "account": remote.as_ref().and_then(|x| x.account.clone()),
            "when_verified": l.when_verified,
            "publication": l.publication, "failed": l.publish_failed,
            "pending": l.publish_pending,
            "waits": l.publish_waited.clone().or_else(|| self.auto_waits.get(&id).cloned()),
            "pr_read": self.pr_checked.get(&id).map(|(ok, err)| json!({ "ok_at": ok, "failed": err })),
            "pr_checking": read.is_some_and(|(_, running)| *running),
        }))
    }

    /// 010: pushes the loop's verified commit and opens a pull request, on a thread (FR-008 to
    /// FR-012); `by` is `engineer` or `when_verified`.
    fn publish(&mut self, id: u64, by: &str) -> Result<(), String> {
        self.publish_with(id, by, Vec::new())
    }

    fn publish_with(&mut self, id: u64, by: &str, unchecked: Vec<String>) -> Result<(), String> {
        let remote = self
            .loop_remote(id, false)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        let (offer, why) = crate::publish::offer(Some(&remote));
        if !offer.contains(&"pull_request") {
            return Err(format!("no pull request here: {why}"));
        }
        let ctx = &self.ctx;
        let run = self
            .runs
            .get(&id)
            .ok_or_else(|| format!("there is no loop {id}"))?;
        let view = super::activity::view(run, ctx)?;
        let mut body = crate::publish::body::body(&view);
        // 011 FR-009: the pull request says which target commits it was not verified with.
        if !unchecked.is_empty() {
            body.push_str(&format!(
                "\n\n**Published without the latest check**: not verified on top of {} as it is now. Not checked with {} commit{}: {}.\n",
                run.state.target,
                unchecked.len(),
                if unchecked.len() == 1 { "" } else { "s" },
                unchecked.iter().map(|c| &c[..c.len().min(7)]).collect::<Vec<_>>().join(", ")
            ));
        }
        let run = self.runs.get_mut(&id).ok_or("no such loop")?;
        super::machine::publish(run, &self.ctx, by, &remote, &body, unchecked)
    }

    /// 010: a loop that ended verified publishes itself when that was chosen at the lock
    /// (FR-020): once, and only with a supported remote whose protection was read; otherwise why
    /// it waits is kept for its page.
    fn auto_publish(&mut self, id: u64) {
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        let Some(l) = r.state.feature_loop.as_ref() else {
            return;
        };
        if r.state.stage != Stage::Ready
            || l.outcome.is_none()
            || l.publish_tried
            || l.when_verified != WhenVerified::PullRequest
            || r.is_busy()
            || l.unlocked
            // 011: a merge the engineer asked for comes first; a question waits for them.
            || l.merge_pending
            || r.state.needs.is_some()
        {
            return;
        }
        let verified = l.outcome.as_ref().is_some_and(|o| o.0);
        let choice = l.when_verified;
        if !verified {
            if let Some(r) = self.runs.get_mut(&id) {
                let _ = r.append(Event::PublishWaited {
                    why: "not verified by Fallow: it waits for you".into(),
                });
            }
            return;
        }
        // Read again when the loop has just become ready (FR-003).
        let first = !self.auto_waits.contains_key(&id);
        let remote = self.loop_remote(id, first);
        if remote
            .as_ref()
            .is_some_and(|x| x.checking || x.state == "checking")
        {
            self.auto_waits
                .insert(id, "reading the remote first".into());
            return;
        }
        self.auto_waits.remove(&id);
        let decided = match crate::publish::when_verified(choice, verified, remote.as_ref()) {
            Ok(()) => self.publish(id, "when_verified").err(),
            Err(why) => Some(why),
        };
        // Decided once (FR-020): a loop that did not publish itself waits for the engineer from
        // then on, also after a later sign-in or a restart.
        if let Some(why) = decided {
            if let Some(r) = self.runs.get_mut(&id) {
                let _ = r.append(Event::PublishWaited { why });
            }
        }
    }

    /// 010: reads a publishing loop's pull request on a thread when the last read is older than
    /// `every` (research R-6).
    fn poll_pull_request(&mut self, id: u64, every: Duration) {
        let Some(r) = self.runs.get(&id) else {
            return;
        };
        if r.state.stage != Stage::Publishing {
            return;
        }
        let Some(pb) = r
            .state
            .feature_loop
            .as_ref()
            .and_then(|l| l.publication.clone())
        else {
            return;
        };
        let every = every.min(pr_poll());
        if let Some((at, running)) = self.pr_reads.get(&id) {
            if *running || at.elapsed() < every {
                return;
            }
        }
        let Some(p) = self.loop_project(id) else {
            return;
        };
        let Some(host) = crate::publish::host::host_for(&pb.service, &self.ctx.settings) else {
            return;
        };
        let Some((owner, name)) = pb.repo.split_once('/') else {
            return;
        };
        let repo = crate::publish::host::Repo {
            owner: owner.into(),
            name: name.into(),
        };
        self.pr_reads.insert(id, (Instant::now(), true));
        let tx = self.ctx.tx.clone();
        let dir = p.path.clone();
        let number = pb.pr_number;
        let target = pb.target.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("pr-{id}"))
            .spawn(move || {
                // 011 (research R-7): behind its target, whether the target requires it to be
                // up to date.
                let r = host.state(&dir, &repo, number).map(|mut st| {
                    if st.behind {
                        st.strict = host.strict(&dir, &repo, &target).ok();
                    }
                    st
                });
                let _ = tx.send(Msg::Done {
                    run: id,
                    done: Done::PrState(r),
                });
            });
        if spawned.is_err() {
            self.pr_reads.insert(id, (Instant::now(), false));
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
        // 012 (FR-039, contracts/web-api.md): what the folder is, and its repository's top.
        let kind = p.kind();
        v["git"] = json!({
            "kind": kind,
            "setup_left": kind == crate::project::Kind::Git && self.studio.setup_left(&p),
            "top": (kind == crate::project::Kind::NeedsMove)
                .then(|| crate::project::top_of(&p.path))
                .flatten(),
        });
        v["spec_root"] = json!(p.spec_root);
        // 012 (FR-001): the session open last, if it is still open.
        v["session_last"] = json!(p.last_session.as_ref().filter(|s| {
            self.studio
                .convs
                .get(*s)
                .and_then(|c| c.state.solo.as_ref())
                .is_some_and(|x| x.closed.is_none())
        }));
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
        v["remote"] = json!(self.remote_of(&p, false));
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
        // 012 (FR-035): the project conversation is retired; an earlier one is listed with the
        // sessions and opens read-only.
        let _ = conv;
        Ok(v)
    }

    // ---- 009 US5: the project's files ---------------------------------------------------------

    /// The versions of a project's files: the folder, then each feature's live copy (research R-8).
    fn file_versions(&self, project: &str) -> Result<Vec<crate::studio::files::Version>, String> {
        let mut v = self.versions_of(project, None)?;
        // 012 (FR-019): each open session's working copy.
        let sessions = self.studio.store.get(project).map(|p| p.sessions.clone()).unwrap_or_default();
        for s in sessions {
            if let Ok(x) = self.file_version(project, &format!("solo:{s}")) {
                v.push(x);
            }
        }
        Ok(v)
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
        Ok(files::versions(&p.path, feats))
    }

    fn file_version(
        &self,
        project: &str,
        id: &str,
    ) -> Result<crate::studio::files::Version, String> {
        let id = if id.is_empty() { "folder" } else { id };
        // 012: a session's working copy (US3 lists these versions; "Open in my editor" uses it
        // now, FR-004).
        if let Some(sid) = id.strip_prefix("solo:") {
            let c = self
                .studio
                .convs
                .get(sid)
                .filter(|c| c.state.project == project)
                .ok_or_else(|| crate::studio::files::GONE.to_string())?;
            let s = c
                .state
                .solo
                .as_ref()
                .filter(|s| s.closed.is_none())
                .ok_or_else(|| crate::studio::files::GONE.to_string())?;
            return Ok(crate::studio::files::Version {
                id: id.to_string(),
                label: s.name.clone().unwrap_or_else(|| "Session".into()),
                kind: "solo".into(),
                running: c.state.busy,
                base: Some(s.base().mark),
                dir: s.worktree.clone(),
                rev: None,
                feature_dir: None,
                git: true,
                lead: "assistant".into(),
                at_work: c.state.busy.then(|| "assistant".to_string()),
            });
        }
        // The folder needs no feature; a feature, only itself.
        let only = id.strip_prefix("feature:").unwrap_or("\u{0}");
        self.versions_of(project, Some(only))?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| crate::studio::files::GONE.to_string())
    }

    /// 012 (R-9, T045): sends each session's oldest waiting message that may go now, its `@`
    /// files read as it is sent; the turns started, by session.
    fn send_waiting(&mut self) -> Vec<(String, u32)> {
        let mut started = Vec::new();
        for (conv, qid, text) in self.studio.waiting_ready() {
            let refs = self.file_refs(&conv, &text);
            if let Some(n) = self.studio.send_waiting(&self.ctx, &conv, &qid, &text, refs) {
                started.push((conv, n));
            }
        }
        started
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
        // 012 (FR-004): `.` is the version's whole working copy ("Open in my editor").
        let abs = if path.is_empty() || path == "." {
            v.dir.clone()
        } else {
            files::safe_path(&v.dir, path)?
        };
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
        // 012 (FR-034): where it was started from, the link back.
        v["origin"] = match f.origin.as_deref() {
            Some(o) => match o.split_once(':') {
                Some(("solo", id)) => json!({
                    "kind": "session",
                    "id": id,
                    "name": self.studio.convs.get(id).and_then(|c| c.state.solo.as_ref()).and_then(|s| s.name.clone()),
                }),
                Some(("conv", id)) => json!({ "kind": "conversation", "id": id }),
                _ => Value::Null,
            },
            None => Value::Null,
        };
        v["conversation"] = f
            .conversation
            .as_ref()
            .and_then(|c| self.studio.conv_view(c))
            .unwrap_or(Value::Null);
        Ok(v)
    }

    /// 012 (FR-039, FR-040): the project a request is about, when it is about one.
    fn project_of(&self, req: &crate::studio::Req) -> Option<String> {
        use crate::studio::Req;
        let conv = |c: &str| self.studio.convs.get(c).map(|c| c.state.project.clone());
        match req {
            Req::RemoveProject { id }
            | Req::PatchProject { id, .. }
            | Req::SetCommands { id, .. }
            | Req::GitInit { id, .. }
            | Req::GitScan { id }
            | Req::Move { id, .. }
            | Req::Project { id }
            | Req::CheckRemote { id } => Some(id.clone()),
            Req::ProjectConversation { project }
            | Req::NewSession { project, .. }
            | Req::Sessions { project }
            | Req::NewFeature { project, .. }
            | Req::FileVersions { project }
            | Req::Files { project, .. }
            | Req::File { project, .. }
            | Req::OpenFile { project, .. }
            | Req::Feature { project, .. }
            | Req::Doc { project, .. }
            | Req::PutDoc { project, .. }
            | Req::SetStep { project, .. }
            | Req::Dismiss { project, .. }
            | Req::GitChanges { project } => Some(project.clone()),
            Req::Agents { project } | Req::Pipelines { project } | Req::CheckPipeline { project, .. } => {
                project.clone()
            }
            Req::PutAgent { scope, .. }
            | Req::PutAgentFields { scope, .. }
            | Req::DeleteAgent { scope, .. }
            | Req::PutPipeline { scope, .. }
            | Req::DeletePipeline { scope, .. } => scope.strip_prefix("project:").map(str::to_string),
            Req::Conversation { id } => conv(id),
            Req::Say { conv: c, .. }
            | Req::Cancel { conv: c }
            | Req::Commands { conv: c }
            | Req::SetMode { conv: c, .. }
            | Req::Approve { conv: c, .. }
            | Req::SetLead { conv: c, .. } => conv(c),
            Req::SessionMode { session, .. }
            | Req::RenameSession { session, .. }
            | Req::OpenSession { session }
            | Req::SoloPending { session, .. }
            | Req::UndoPreview { session, .. }
            | Req::Undo { session, .. }
            | Req::SoloChanges { session }
            | Req::SoloGates { session }
            | Req::SoloAttach { session, .. }
            | Req::SoloAttachment { session, .. }
            | Req::SoloSay { session, .. }
            | Req::SoloQueue { session, .. }
            | Req::SoloWeb { session, .. }
            | Req::FeatureDraft { session, .. }
            | Req::FeatureFromSession { session, .. } => conv(session),
            Req::FeatureFromConversation { conv: c, .. } => conv(c),
            _ => None,
        }
    }

    /// 012 (FR-039 to FR-041): why a request about `project` is refused: git is missing or too
    /// old, or the project is not a git repository yet (only its page, making it one, moving it
    /// to its repository's top folder and removing it are open then).
    fn project_refusal(&self, project: &str, open: bool) -> Option<String> {
        self.project_refusal_for(project, open, true)
    }

    /// `project_refusal`; `new_work: false` for a loop already made, which is not held back
    /// by its project's setup left (third review, 2).
    fn project_refusal_for(&self, project: &str, open: bool, new_work: bool) -> Option<String> {
        if !self.git.ok {
            return Some(crate::project::GIT_NEEDED.to_string());
        }
        let p = self.studio.store.get(project)?;
        let kind = {
            let mut k = self.kinds.borrow_mut();
            match k.get(project) {
                Some((at, kind)) if at.elapsed() < Duration::from_secs(5) => *kind,
                _ => {
                    let kind = p.kind();
                    k.insert(project.to_string(), (Instant::now(), kind));
                    kind
                }
            }
        };
        match kind {
            crate::project::Kind::Git if !open && new_work && self.studio.setup_left(p) => Some(format!(
                "{} is not set up yet: give its features their branches first",
                p.name
            )),
            crate::project::Kind::Git | crate::project::Kind::Missing => None,
            _ if open => None,
            crate::project::Kind::NeedsMove => Some(format!(
                "{} is inside a bigger repository: use the repository's top folder first",
                p.name
            )),
            crate::project::Kind::Unknown => Some(crate::project::GIT_NEEDED.to_string()),
            _ => Some(format!("{} is not a git repository yet: make it one first", p.name)),
        }
    }

    fn studio_request(&mut self, req: crate::studio::Req) -> Result<Value, String> {
        use crate::studio::Req;
        if let Some(p) = self.project_of(&req) {
            let open = matches!(
                req,
                Req::Project { .. }
                    | Req::GitInit { .. }
                    | Req::GitScan { .. }
                    | Req::Move { .. }
                    | Req::RemoveProject { .. }
            );
            if let Some(why) = self.project_refusal(&p, open) {
                return Err(why);
            }
        }
        match req {
            Req::AddProject { path } => {
                // Git is read again when a project is added (FR-040).
                self.git = crate::verify::git::git_check();
                if !self.git.ok {
                    return Err(crate::project::GIT_NEEDED.to_string());
                }
                self.studio.add_project(&path)
            }
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
            Req::GitInit { id, gitignore } => self.studio.git_init(&id, gitignore),
            Req::GitScan { id } => self.studio.git_scan(&id),
            Req::Move { id, confirmed } => self.studio.move_project(&id, confirmed),
            Req::Project { id } => self.project_view(&id),
            // 010: the engineer opened the project: its remote is read again now (FR-003).
            Req::CheckRemote { id } => {
                let p = self
                    .studio
                    .store
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| format!("there is no project {id}"))?;
                Ok(json!({ "remote": self.remote_of(&p, true) }))
            }
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
                let out = self.studio.say(&self.ctx, &conv, &text, refs);
                // 012 (R-9): the engineer's message is the go-ahead for those waiting.
                let mut out = out;
                for (c, n) in self.send_waiting() {
                    if c == conv {
                        if let Ok(v) = out.as_mut() {
                            if v.get("turn").is_none() {
                                v["turn"] = json!(n);
                            }
                        }
                    }
                }
                out
            }
            Req::Cancel { conv } => self.studio.cancel(&conv),
            Req::NewSession {
                project,
                agent,
                text,
            } => self.studio.new_session(&self.ctx, &project, agent, &text, Vec::new()),
            Req::SessionMode { session, mode } => self.studio.set_solo_mode(&session, &mode),
            Req::Sessions { project } => self.studio.list_sessions(&project),
            Req::RenameSession { session, name } => self.studio.rename_session(&session, &name),
            Req::OpenSession { session } => self.studio.open_session(&session),
            Req::SoloPending {
                session,
                choice,
                confirmed,
            } => self.studio.solo_pending(&session, &choice, confirmed),
            Req::UndoPreview { session, turn } => self.studio.undo_preview(&session, turn),
            Req::Undo {
                session,
                turn,
                confirmed,
            } => self.studio.undo_turns(&session, turn, confirmed),
            Req::SoloChanges { session } => self.studio.solo_changes(&session),
            Req::GitChanges { project } => self.git_changes(&project),
            Req::FeatureDraft { session, suggestion } => self.studio.feature_draft(&session, suggestion),
            Req::FeatureFromSession { session, title, brief, lead } => {
                self.studio.feature_from_session(&self.ctx, &session, &title, &brief, lead)
            }
            Req::FeatureFromConversation { conv, title, brief, lead } => {
                self.studio.feature_from_conversation(&self.ctx, &conv, &title, &brief, lead)
            }
            Req::SoloGates { session } => self.studio.run_solo_gates(&self.ctx, &session),
            Req::SoloAttach { session, name, bytes } => self.studio.solo_attach(&self.ctx, &session, &name, &bytes),
            Req::SoloAttachment { session, sha } => self.studio.solo_attachment(&session, &sha),
            Req::SoloSay { session, text, attachments } => {
                let refs = self.file_refs(&session, &text);
                let mut out = self.studio.solo_say_with(&self.ctx, &session, &text, refs, &attachments);
                for (c, n) in self.send_waiting() {
                    if let (true, Ok(v)) = (c == session, out.as_mut()) {
                        if v.get("turn").is_none() {
                            v["turn"] = json!(n);
                        }
                    }
                }
                out
            }
            Req::SoloQueue { session, id, text, remove, send } => {
                let out = self.studio.solo_queue(&session, &id, text, remove, send);
                self.send_waiting();
                out
            }
            Req::SoloWeb { session, on } => self.studio.solo_web(&self.ctx, &session, on),
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
            } => {
                // 012 (R-4): edits made in the session's copy while a turn works count as the
                // turn's; the session says so.
                if let Some(sid) = version.strip_prefix("solo:") {
                    if let Some(c) = self.studio.convs.get_mut(sid).filter(|c| c.state.busy) {
                        c.editor_during_turn = true;
                        c.tick();
                    }
                }
                self.open_file(&project, &version, &path, editor, command)
            }
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
                    // 012 (FR-010a): what it can do, as its adapter declares it.
                    let caps = crate::agent::registry::capabilities(&name, &self.ctx.settings);
                    json!({ "name": name, "installed": a.installed, "version": a.version,
                            "models": models, "capabilities": caps })
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
        // 010: its pull request is the verified work as locked; it ends first (merged or closed).
        if r.busy == Some(Busy::Publish) {
            return Err("the loop is being published; unlock it after that".into());
        }
        if r.state.stage == Stage::Publishing {
            return Err(
                "its pull request is open: close it on the remote first, then unlock".into(),
            );
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
        // 011: an update's step or computation still running would be pulled from under it
        // (review 4): the lock waits for it, or the engineer stops the loop.
        if let Some(r) = self.runs.get(&id) {
            let updating = r
                .state
                .feature_loop
                .as_ref()
                .is_some_and(|l| l.update.is_some());
            if updating && (r.busy.is_some() || !r.running.is_empty()) {
                return Err("the loop is being brought up to date and a step is running: lock again when it ends, or stop the loop".into());
            }
        }
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
        // 011: a "Lock again" after the target changed the documents: what the engineer
        // reconciled against is recorded, so the same change is not flagged again (R-2).
        let watched_tip = self
            .watched
            .get(&id)
            .and_then(|w| w.read.as_ref())
            .filter(|m| m.moved && !m.docs_changed.is_empty())
            .map(|m| m.target_commit.clone());
        let r = self
            .runs
            .get_mut(&id)
            .ok_or("the feature's loop is missing")?;
        let held = r
            .state
            .feature_loop
            .as_ref()
            .and_then(|l| l.update.clone())
            .filter(|u| !u.docs_changed.is_empty())
            .map(|u| u.target_commit);
        let reconciled_target = held.clone().or(watched_tip);
        if r.state
            .feature_loop
            .as_ref()
            .is_some_and(|l| l.update.is_some())
        {
            super::update::drop_update(r, "locked again")?;
        }
        r.append(Event::Relocked {
            lock: prep.lock,
            commit: prep.commit.clone(),
            affected: a.stories.clone(),
            phases: prep.phases.clone(),
            reconciled_target: reconciled_target.clone(),
        })?;
        // What the watch read before is for the old lock: read again.
        self.watched.remove(&id);
        let r = self
            .runs
            .get_mut(&id)
            .ok_or("the feature's loop is missing")?;
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
            reconciled_target: reconciled_target.or(old.reconciled_target.clone()),
            ..old
        };
        self.studio.lock_record(project, feature, rec, id)?;
        self.drive(id);
        Ok(json!({ "lock": prep.lock, "commit": prep.commit, "loop": id, "affected": a.stories }))
    }

    /// Locks a feature and creates its loop (FR-008, FR-016a; research S-9).
    #[allow(clippy::too_many_arguments)]
    fn lock(
        &mut self,
        project: &str,
        feature: &str,
        pipeline: Option<String>,
        level: Option<Level>,
        budget_usd: Option<f64>,
        budget_minutes: Option<u64>,
        when_verified: Option<WhenVerified>,
        extra: LockExtra,
    ) -> Result<Value, String> {
        if let Some(id) = self.live_loop(project, feature) {
            return self.relock(project, feature, id);
        }
        let update_limit = extra
            .update_limit
            .unwrap_or_else(crate::run::events::update_limit);
        if !(1..=20).contains(&update_limit) {
            return Err(format!(
                "updates overtaken before you decide: {update_limit} is not between 1 and 20"
            ));
        }
        // 011 FR-014: "Run after" an unfinished loop of the same project.
        if let Some(other) = extra.run_after {
            let o = self
                .runs
                .get(&other)
                .ok_or_else(|| format!("there is no loop {other} to run after"))?;
            let same = o
                .state
                .feature_loop
                .as_ref()
                .is_some_and(|l| l.project == project);
            if !same || o.state.stage.is_final() {
                return Err(format!(
                    "loop {other} is not an unfinished loop of {project}"
                ));
            }
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
        let squad = match git::show(&p.path, &prep.commit, &p.in_root(crate::config::squad::SQUAD_FILE)) {
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
            merge_unchecked: Vec::new(),
            publish_unchecked: Vec::new(),
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
            when_verified: when_verified.unwrap_or(WhenVerified::preset(level)),
            update_limit,
            run_after: extra.run_after,
            switches: crate::run::events::Switches {
                auto: squad.updates.auto,
                line: squad.updates.line,
                overlap: squad.updates.overlap,
            },
            spec_root: p.spec_root.clone(),
        })?;
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("lock {} of {}", prep.lock, f.name()),
        })?;
        if let Some(other) = extra.run_after {
            run.append(Event::WaitsFor {
                run: other,
                from: crate::run::events::WaitFrom::Lock,
            })?;
        }
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
            when_verified: when_verified.unwrap_or(WhenVerified::preset(level)),
            update_limit: Some(update_limit),
            run_after: extra.run_after,
            reconciled_target: None,
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
        self.auto_publish(id);
        self.poll_pull_request(id, PR_POLL);
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
        // 010 FR-003: the remote is read again when a loop reaches ready, so what the page
        // offers (and a merge asked for right away) does not wait on a first read.
        let ready = self
            .runs
            .get(&id)
            .is_some_and(|r| r.state.stage == Stage::Ready && r.state.feature_loop.is_some());
        if ready {
            self.watch(id, false);
        }
        self.turns();
        self.overlaps_of(id);
        self.waits();
        self.target_moved_for(id);
        self.push_forward_due(id);
        self.pr_fell_behind(id);
        self.publish_again(id);
        let was_ready = self.ready_seen.contains(&id);
        if ready && !was_ready {
            self.ready_seen.insert(id);
            let _ = self.loop_remote(id, true);
        } else if !ready && was_ready {
            self.ready_seen.remove(&id);
        }
    }

    fn request(&mut self, r: Request) -> Reply {
        // 012 (FR-039, FR-040): a loop's requests are project requests too.
        let of_run = |id: &u64| {
            self.runs
                .get(id)
                .and_then(|r| r.state.feature_loop.as_ref())
                .map(|l| l.project.clone())
        };
        let new_work = matches!(
            &r,
            Request::Lock { .. }
                | Request::Unlock { .. }
                | Request::RelockPreview { .. }
                | Request::DeleteFeature { .. }
                | Request::Overlap { .. }
        );
        let about = match &r {
            Request::Lock { project, .. }
            | Request::Unlock { project, .. }
            | Request::RelockPreview { project, .. }
            | Request::DeleteFeature { project, .. }
            | Request::Overlap { project, .. } => Some(project.clone()),
            Request::Answer { id, .. }
            | Request::Publish { id, .. }
            | Request::Limits { id, .. }
            | Request::Retry { id }
            | Request::Merge { id, .. }
            | Request::Update { id }
            | Request::LeavePr { id }
            | Request::Wait { id, .. }
            | Request::StopWaiting { id }
            | Request::LockAgain { id } => of_run(id),
            _ => None,
        };
        if let Some(why) = about.and_then(|p| self.project_refusal_for(&p, false, new_work)) {
            return Reply::Err(why);
        }
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
            Request::Git { check } => {
                if check {
                    self.git = crate::verify::git::git_check();
                    self.bus.publish(json!({ "type": "studio" }));
                }
                Ok(json!(self.git))
            }

            Request::Events { .. } | Request::ToolCall { .. } => {
                Err("answered by the engine's loop".into())
            }
            Request::Studio(req) => {
                use crate::studio::Req;
                let reads = matches!(
                    req,
                    Req::Project { .. }
                        | Req::UndoPreview { .. }
                        | Req::SoloChanges { .. }
                        | Req::GitChanges { .. }
                        | Req::FeatureDraft { .. }
                        | Req::SoloAttachment { .. }
                        | Req::GitScan { .. }
                        | Req::Move { confirmed: false, .. }
                        | Req::CheckRemote { .. }
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
                // What changes a project's kind forgets the one kept for the gate, once done.
                let changes = matches!(
                    req,
                    Req::AddProject { .. }
                        | Req::GitInit { .. }
                        | Req::Move { .. }
                        | Req::RemoveProject { .. }
                        | Req::PatchProject { .. }
                );
                let out = self.studio_request(req);
                if changes {
                    self.kinds.borrow_mut().clear();
                    // 012 (R-10): a project added or made usable is watched at once.
                    self.refresh_watch_list();
                }
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
                when_verified,
                update_limit,
                run_after,
            } => {
                let out = self.lock(
                    &project,
                    &feature,
                    pipeline,
                    level,
                    budget_usd,
                    budget_minutes,
                    when_verified,
                    LockExtra {
                        update_limit,
                        run_after,
                    },
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
            Request::Merge { id, .. } if self.target_gone(id) => Err(format!(
                "the target branch {} is gone: nothing is merged. Stop the loop, or unlock the feature and lock it from another branch.",
                self.runs
                    .get(&id)
                    .map(|r| r.state.target.clone())
                    .unwrap_or_default()
            )),
            // FR-003: a merge that would conflict is not tried; one that moved without a
            // conflict joins the line and is brought up to date first (FR-009a).
            Request::Merge { id, .. } if !self.conflicts_known(id).is_empty() => Err(format!(
                "the target moved since this loop was verified and would conflict in {}: bring it up to date first",
                self.conflicts_known(id).join(", ")
            )),
            Request::Update { id } => self.update(id).map(|()| json!({})),
            Request::LockAgain { id } => self.lock_again(id),
            Request::Merge {
                id,
                without_latest_check,
            } => {
                // 010: a protected target takes a pull request, not a local merge (FR-006).
                let refused = self.publish_view(id).and_then(|p| {
                    let offer = p["offer"].as_array().cloned().unwrap_or_default();
                    let open = p["publication"]["state"] == "open";
                    (p["remote"].is_object() && !open && !offer.iter().any(|o| o == "local_merge"))
                        .then(|| p["why"].as_str().unwrap_or("").to_string())
                });
                match refused {
                    Some(why) => Err(format!("no local merge here: {why}")),
                    None => self.merge_request(id, without_latest_check),
                }
            }
            Request::Publish {
                id,
                without_latest_check: true,
            } => self.publish_unchecked(id).map(|()| json!({})),
            Request::Publish { id, .. } => self.publish(id, "engineer").map(|()| json!({})),
            Request::LeavePr { id } => self.leave_pr(id).map(|()| json!({})),
            Request::Overlap { project, feature } => self.overlap_read(&project, &feature),
            Request::Wait { id, other } => self.wait_for(id, other).map(|()| json!({})),
            Request::StopWaiting { id } => self.stop_waiting(id).map(|()| json!({})),
            Request::Stop { id, reason } => run_mut(self, id)
                .and_then(|r| super::machine::stop(r, reason))
                .map(|()| json!({})),
            Request::Loop { id } => {
                let ctx = &self.ctx;
                let v = self
                    .runs
                    .get(&id)
                    .ok_or_else(|| format!("there is no loop {id}"))
                    .and_then(|r| super::activity::view(r, ctx));
                self.watch(id, true);
                v.map(|mut v| {
                    // 010: what publishing offers, and the pull request (contracts/web-api.md).
                    if let Some(p) = self.publish_view(id) {
                        v["publish"] = p;
                    }
                    // 011: the target and what can succeed (contracts/web-api.md).
                    let (target, offer) = self.target_view(id, &v["publish"]);
                    v["target"] = target;
                    v["offer"] = offer;
                    if let Some(r) = self.runs.get(&id) {
                        v["resolutions"] = update_resolutions(r);
                    }
                    // 011: the merge line (FR-009a) and a merge without the latest check (FR-009).
                    v["line"] = match self.line_of(id) {
                        Some((pos, after)) => json!({ "position": pos + 1, "after": after }),
                        None => Value::Null,
                    };
                    if let Some(l) = self
                        .runs
                        .get(&id)
                        .and_then(|r| r.state.feature_loop.as_ref())
                    {
                        v["unchecked"] = json!(l.unchecked);
                    }
                    // 011 FR-015: overlaps with other loops, and a wait (contracts/web-api.md).
                    let name = |other: u64| {
                        self.runs
                            .get(&other)
                            .and_then(|o| o.state.feature_loop.as_ref())
                            .map_or_else(|| format!("loop {other}"), |l| l.feature.clone())
                    };
                    if let Some(r) = self.runs.get(&id) {
                        if let Some(l) = r.state.feature_loop.as_ref() {
                            // Only with loops still unfinished (review 7).
                            let live = |other: u64| {
                                self.runs
                                    .get(&other)
                                    .is_some_and(|o| !o.state.stage.is_final())
                            };
                            v["overlaps"] = json!(l
                                .overlaps
                                .iter()
                                .filter(|o| live(o.with))
                                .map(|o| json!({ "run": o.with, "feature": name(o.with), "files": o.files }))
                                .collect::<Vec<_>>());
                            v["waits_for"] = match l.waits_for {
                                Some((other, from)) => {
                                    json!({ "run": other, "feature": name(other), "from": from })
                                }
                                None => Value::Null,
                            };
                            let open = r.state.stage == Stage::Publishing;
                            if let Some(offer) = v["offer"].as_array_mut() {
                                let over = r.state.stage.is_final();
                                if l.waits_for.is_some() && !over {
                                    offer.push(json!("stop_waiting"));
                                } else if l.overlaps.iter().any(|o| live(o.with))
                                    && !open
                                    && r.state.stage == Stage::Building
                                {
                                    offer.push(json!("wait_for"));
                                }
                            }
                        }
                    }
                    if let Some(l) = self
                        .runs
                        .get(&id)
                        .and_then(|r| r.state.feature_loop.as_ref())
                    {
                        v["update"] = json!(l.update);
                        v["updates"] = json!(l.updates);
                        v["update_limit"] = json!(l.update_limit);
                        v["overtaken"] = json!(l.overtaken);
                    }
                    self.poll_pull_request(id, Duration::from_secs(10));
                    v
                })
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
        Some(Busy::Publish) => "publish".into(),
        Some(Busy::Update) => "update".into(),
        None if steps.len() == 1 => format!("step {}", steps[0]),
        None if !steps.is_empty() => format!("steps {}", steps.join(", ")),
        None => String::new(),
    });
    v["waiting"] = json!(r.waiting);
    v["interventions"] = json!(r.state.interventions());
    v["minutes_used"] = json!(r.state.minutes_used(super::time::unix_now()));
    v
}

/// 011: the builder's own account of the non-text conflicts of the update in progress, else of
/// the last one: its last accepted repair's `resolutions` (shown, never taken as evidence).
fn update_resolutions(r: &Run) -> Value {
    let Some(l) = r.state.feature_loop.as_ref() else {
        return json!([]);
    };
    let n = l.update.as_ref().map_or(l.updates, |u| u.n);
    let part = format!("update-{n}");
    r.state
        .steps
        .values()
        .rev()
        .filter(|s| s.part.as_deref() == Some(part.as_str()))
        .filter(|s| s.verified.as_ref().is_some_and(|v| v.0))
        .find_map(|s| match &s.end {
            Some(StepEnd::Result { value }) => value.get("resolutions").cloned(),
            _ => None,
        })
        .unwrap_or_else(|| json!([]))
}
