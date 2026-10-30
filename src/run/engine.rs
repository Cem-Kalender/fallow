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
pub struct Ctx {
    pub settings: Settings,
    pub tx: Sender<Msg>,
    /// Fallow's own executable and process id, for the step tools' bridge.
    pub exe: PathBuf,
    pub pid: u32,
    /// 007: the engineer's configuration folder (`~/.fallow`), for definitions.
    pub config: Option<PathBuf>,
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
}

pub fn runs_dir(data: &Path) -> PathBuf {
    data.join("runs")
}

impl Engine {
    /// Loads every run under `data/runs`, resuming those that are not finished (research R-2).
    pub fn load(data: PathBuf, ctx: Ctx) -> Result<Engine, String> {
        crate::platform::private_dir(&data)?;
        crate::platform::private_dir(&runs_dir(&data))?;
        let studio = crate::studio::Studio::load(&data, ctx.config.clone())?;
        let mut e = Engine {
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
                    self.studio.tool_call(&token, &name, args, reply);
                    self.bus.publish(json!({ "type": "studio" }));
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
                        r.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    break;
                }
            }
            self.publish_changes();
            self.console.append(&mut self.studio.console);
            for line in self.console.drain(..) {
                eprintln!("{line}");
            }
        }
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
            return "locked";
        };
        if r.state.merged.is_some() {
            return "merged";
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
        json!({
            "project": p.id,
            "project_name": p.name,
            "name": f.name(),
            "dir": f.dir,
            "title": f.title,
            "stage": self.stage(f),
            "step": f.step,
            "run": f.run,
            "conversation": f.conversation,
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
        v["branch"] = json!(git::branch(&p.path));
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
        v["conversation"] = self
            .studio
            .convs
            .get(&conv)
            .map(|c| c.view())
            .unwrap_or(Value::Null);
        Ok(v)
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
            .and_then(|c| self.studio.convs.get(c))
            .map(|c| c.view())
            .unwrap_or(Value::Null);
        Ok(v)
    }

    fn studio_request(&mut self, req: crate::studio::Req) -> Result<Value, String> {
        use crate::studio::Req;
        match req {
            Req::AddProject { path } => self.studio.add_project(&path),
            Req::RemoveProject { id } => self.studio.remove_project(&id),
            Req::PatchProject { id, patch } => self.studio.patch_project(&id, &patch),
            Req::SetCommands { id, commands } => self.studio.set_commands(&id, commands),
            Req::GitInit { id } => self.studio.git_init(&id),
            Req::Project { id } => self.project_view(&id),
            Req::Conversation { id } => self
                .studio
                .convs
                .get(&id)
                .map(|c| c.view())
                .ok_or_else(|| format!("there is no conversation {id}")),
            Req::ProjectConversation { project } => self
                .studio
                .project_conversation(&project)
                .map(|id| json!({ "conversation": id })),
            Req::Say { conv, text } => self.studio.say(&self.ctx, &conv, &text),
            Req::SetMode { conv, mode } => self.studio.set_mode(&conv, mode),
            Req::Approve {
                conv,
                id,
                answer,
                prefix,
            } => self.studio.approve(&conv, &id, answer, prefix),
            Req::NewFeature { project, title } => self.studio.new_feature(&project, &title),
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
            Req::ToolOpen { token, pid } => self.studio.tool_open(&token, pid),
        }
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
            plan: None,
            stop: Arc::new(AtomicBool::new(false)),
            waiters: Vec::new(),
            activity: 0,
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
        let needs_you: Vec<Value> = self
            .runs
            .values()
            .filter_map(|r| {
                needs(r).map(|why| json!({ "run": r.id(), "why": why, "task": r.state.task }))
            })
            .collect();
        let loops: Vec<Value> = self
            .runs
            .values()
            .rev()
            .filter(|r| !r.state.stage.is_final())
            .map(summary)
            .collect();
        let agents_running = self
            .runs
            .values()
            .filter(|r| matches!(r.busy, Some(Busy::Step(_))))
            .count();
        let cost_today: f64 = self
            .runs
            .values()
            .filter(|r| r.state.created.starts_with(today))
            .map(|r| r.state.cost_usd)
            .sum();
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
    let out = crate::platform::command("git")
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
    if s.feature_loop.is_some() {
        return (s.stage == Stage::Ready).then(|| "Ready to merge".into());
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
    // Before approval, the plan the engineer is asked to approve (its commands among it).
    let plan = r.plan.clone().or_else(|| {
        r.state
            .plan_commit
            .as_deref()
            .and_then(|c| crate::plan::at(&r.state.repo, c).ok().flatten())
    });
    v["plan"] = serde_json::to_value(&plan).unwrap_or(Value::Null);
    v
}
