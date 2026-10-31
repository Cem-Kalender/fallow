//! Fallow Studio's projects, conversations and features (007 US1; research S-5 to S-8, S-15,
//! S-16; contracts/web-api.md). The engine owns one `Studio`; turns of the lead run on threads
//! and report back as `Note`s.

pub mod conv;
pub mod defs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::rights::{self, Mode};
use crate::agent::{self, Call, Session, prompts};
use crate::conversation::events::{ApprovalAnswer, DocEdit, Event as CEvent, Kind, State};
use crate::defs::Library;
use crate::platform::job::Group;
use crate::project::{self, Feature, LockRec, Project, Store};
use crate::run::engine::{Ctx, Msg, Reply};
use crate::run::events::{Level, Log, PhaseSpec};
use crate::speckit::lock::{self, Finding};
use crate::verify::git;

use conv::{Conv, Pending};

/// The Spec Kit steps of a spec session, in order (FR-006). `done` follows analyze.
pub const STEPS: &[&str] = &[
    "constitution",
    "specify",
    "clarify",
    "plan",
    "tasks",
    "analyze",
    "done",
];

/// How long the engineer has to answer a command the lead asked to run (research S-7).
pub const APPROVAL_WAIT: Duration = Duration::from_secs(10 * 60);

/// What a conversation's MCP token belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Conv(String),
}

pub struct Tool {
    pub owner: Owner,
    pub group: Arc<Group>,
}

/// What a turn's thread reports.
#[derive(Debug)]
pub enum Note {
    Turn {
        conv: String,
        event: agent::Event,
        t: u64,
    },
    TurnDone {
        conv: String,
        n: u32,
        outcome: agent::Outcome,
    },
}

/// The page's and the bridge's requests about projects, conversations and features.
#[derive(Debug)]
pub enum Req {
    AddProject {
        path: PathBuf,
    },
    RemoveProject {
        id: String,
    },
    PatchProject {
        id: String,
        patch: Value,
    },
    SetCommands {
        id: String,
        commands: Vec<String>,
    },
    GitInit {
        id: String,
    },
    Project {
        id: String,
    },
    Conversation {
        id: String,
    },
    /// The project conversation, created on first use.
    ProjectConversation {
        project: String,
    },
    Say {
        conv: String,
        text: String,
    },
    SetMode {
        conv: String,
        mode: Mode,
    },
    Approve {
        conv: String,
        id: String,
        answer: ApprovalAnswer,
        prefix: Option<String>,
    },
    NewFeature {
        project: String,
        title: String,
    },
    Feature {
        project: String,
        feature: String,
    },
    Doc {
        project: String,
        feature: String,
        path: String,
    },
    PutDoc {
        project: String,
        feature: String,
        path: String,
        text: String,
        version: String,
    },
    SetStep {
        project: String,
        feature: String,
        to: String,
    },
    Dismiss {
        project: String,
        feature: String,
        finding: usize,
    },
    ToolOpen {
        token: String,
        pid: u32,
    },
    /// 007 US6: the agent library and the pipelines (contracts/web-api.md).
    Agents {
        project: Option<String>,
    },
    PutAgent {
        name: String,
        scope: String,
        text: String,
    },
    DeleteAgent {
        name: String,
        scope: String,
    },
    Pipelines {
        project: Option<String>,
    },
    CheckPipeline {
        project: Option<String>,
        toml: String,
    },
    PutPipeline {
        name: String,
        scope: String,
        toml: String,
    },
    DeletePipeline {
        name: String,
        scope: String,
    },
    Runtimes,
}

pub struct Studio {
    pub data: PathBuf,
    pub config: Option<PathBuf>,
    pub store: Store,
    pub convs: BTreeMap<String, Conv>,
    pub tools: BTreeMap<String, Tool>,
    /// The bridge's `--endpoint`, for an engine listening on its own (tests).
    pub endpoint: Option<String>,
    /// How long a command waits for the engineer (`APPROVAL_WAIT`; shorter in tests).
    pub approval_wait: Duration,
    pub console: Vec<String>,
}

fn now() -> String {
    crate::run::time::rfc3339(crate::run::time::unix_now())
}

/// A 64-bit FNV-1a hash: a document's version.
pub fn version_of(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn conv_dir(data: &Path, id: &str) -> PathBuf {
    data.join("conversations").join(id)
}

impl Studio {
    /// Loads the projects and every conversation. A conversation whose turn was cut off by a
    /// stop or a crash gets that turn closed, so it can go on (constitution VII).
    pub fn load(data: &Path, config: Option<PathBuf>) -> Result<Studio, String> {
        let store = Store::load(data)?;
        let mut s = Studio {
            data: data.to_path_buf(),
            config,
            store,
            convs: BTreeMap::new(),
            tools: BTreeMap::new(),
            endpoint: None,
            approval_wait: APPROVAL_WAIT,
            console: Vec::new(),
        };
        let dir = data.join("conversations");
        crate::platform::private_dir(&dir)?;
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Some(id) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            match Log::<CEvent>::open(&e.path()).and_then(|(log, events, notes)| {
                State::fold(&id, &events).map(|st| (log, st, notes))
            }) {
                Ok((log, state, notes)) => {
                    s.console.extend(notes);
                    let mut c = Conv::new(e.path(), log, state);
                    if c.state.busy {
                        if let Some(t) = c.state.turns.last() {
                            let n = t.n;
                            let _ = c.append(CEvent::TurnFinished {
                                n,
                                reply: String::new(),
                                result: None,
                                session: None,
                                documents_changed: Vec::new(),
                                commit: None,
                                discarded: Vec::new(),
                                cost_usd: None,
                                failure: Some(
                                    "Fallow stopped during this turn; say it again to go on".into(),
                                ),
                            });
                        }
                    }
                    s.convs.insert(id, c);
                }
                Err(err) => s
                    .console
                    .push(format!("conversation {id} could not be loaded: {err}")),
            }
        }
        Ok(s)
    }

    pub fn library(&self, project: Option<&Path>) -> Library {
        Library::load(self.config.as_deref(), project)
    }

    fn project(&self, id: &str) -> Result<&Project, String> {
        self.store
            .get(id)
            .ok_or_else(|| format!("there is no project {id}"))
    }

    fn project_mut(&mut self, id: &str) -> Result<&mut Project, String> {
        self.store
            .get_mut(id)
            .ok_or_else(|| format!("there is no project {id}"))
    }

    fn conv_mut(&mut self, id: &str) -> Result<&mut Conv, String> {
        self.convs
            .get_mut(id)
            .ok_or_else(|| format!("there is no conversation {id}"))
    }

    fn worktrees(&self, project: &str) -> PathBuf {
        self.data.join("worktrees").join(project)
    }

    fn new_conv(
        &mut self,
        kind: Kind,
        project: &str,
        feature: Option<String>,
        forked_from: Option<String>,
        workdir: PathBuf,
    ) -> Result<String, String> {
        let id = format!(
            "c{}",
            crate::platform::random::hex(4).map_err(|e| e.to_string())?
        );
        let dir = conv_dir(&self.data, &id);
        let (log, _, _) = Log::<CEvent>::open(&dir)?;
        let state = State {
            id: id.clone(),
            kind,
            project: project.to_string(),
            feature: feature.clone(),
            forked_from: forked_from.clone(),
            workdir: workdir.clone(),
            session: None,
            mode: Mode::Ask,
            step: None,
            turns: Vec::new(),
            busy: false,
            cost_usd: 0.0,
            seq: 0,
            pending_edits: Vec::new(),
        };
        let mut c = Conv::new(dir, log, state);
        c.append(CEvent::ConversationCreated {
            kind,
            project: project.to_string(),
            feature,
            forked_from,
            workdir,
        })?;
        self.convs.insert(id.clone(), c);
        Ok(id)
    }

    // ---- Projects ---------------------------------------------------------------------------

    pub fn add_project(&mut self, path: &Path) -> Result<Value, String> {
        let (p, report) = self.store.add(path, &now())?;
        Ok(json!({ "project": p.id, "spec_kit": report }))
    }

    pub fn remove_project(&mut self, id: &str) -> Result<Value, String> {
        let p = self.store.remove(id)?;
        // Fallow's own data for it: conversations and worktrees. The folder is not touched.
        let convs: Vec<String> = self
            .convs
            .iter()
            .filter(|(_, c)| c.state.project == p.id)
            .map(|(k, _)| k.clone())
            .collect();
        for c in &convs {
            if let Some(c) = self.convs.remove(c) {
                // A running turn ends; its bridge's token stops working.
                c.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                if let Some(t) = &c.token {
                    self.tools.remove(t);
                }
                let _ = std::fs::remove_dir_all(&c.dir);
            }
        }
        let wt = self.worktrees(&p.id);
        if wt.exists() && project::kind_of(&p.path) == project::Kind::Git {
            for e in std::fs::read_dir(&wt).into_iter().flatten().flatten() {
                let _ = git::worktree_remove(&p.path, &e.path());
            }
        }
        let _ = std::fs::remove_dir_all(&wt);
        Ok(json!({ "removed": p.id, "conversations": convs.len() }))
    }

    pub fn patch_project(&mut self, id: &str, patch: &Value) -> Result<Value, String> {
        let config = self.config.clone();
        let p = self.project_mut(id)?;
        if let Some(v) = patch.get("pipeline").and_then(Value::as_str) {
            // Only a pipeline in force for the project, saved and checked (FR-027, FR-028).
            if !Library::load(config.as_deref(), Some(&p.path))
                .pipelines
                .contains_key(v)
            {
                return Err(format!("there is no pipeline {v:?} for this project"));
            }
            p.pipeline = v.to_string();
        }
        if let Some(v) = patch.get("autonomy").and_then(Value::as_str) {
            p.level = match v {
                "strict" => Level::Strict,
                "normal" => Level::Normal,
                "free" => Level::Free,
                _ => return Err(format!("autonomy {v:?}: strict, normal or free")),
            };
        }
        if let Some(b) = patch.get("budget_per_task") {
            if let Some(u) = b.get("usd").and_then(Value::as_f64).filter(|u| *u > 0.0) {
                p.budget_per_task.usd = u;
            }
            if let Some(m) = b.get("minutes").and_then(Value::as_u64).filter(|m| *m > 0) {
                p.budget_per_task.minutes = m;
            }
        }
        if let Some(v) = patch.get("notifications").and_then(Value::as_bool) {
            p.notifications = v;
        }
        if let Some(v) = patch.get("path").and_then(Value::as_str) {
            let path = PathBuf::from(v);
            if !path.is_dir() {
                return Err(format!("{v} is not a folder"));
            }
            p.path = path;
        }
        self.store.save()?;
        Ok(json!({}))
    }

    pub fn set_commands(&mut self, id: &str, commands: Vec<String>) -> Result<Value, String> {
        let mut clean = Vec::new();
        for c in commands {
            let c = c.trim().to_string();
            if let Some(why) = project::commands::command_problem(&c) {
                return Err(format!("{c:?}: {why}"));
            }
            if !clean.contains(&c) {
                clean.push(c);
            }
        }
        self.project_mut(id)?.approved_commands = clean.clone();
        self.store.save()?;
        Ok(json!(clean))
    }

    /// `git init` and an initial commit of what is there, at the engineer's request (edge cases).
    pub fn git_init(&mut self, id: &str) -> Result<Value, String> {
        let p = self.project(id)?.clone();
        if p.kind() == project::Kind::Git {
            return Err(format!("{} is already a git repository", p.name));
        }
        if let Some(top) = git::inside_repo(&p.path) {
            return Err(format!(
                "{} is inside the repository {}: add that folder as the project instead",
                p.path.display(),
                top.display()
            ));
        }
        let path = p.path.clone();
        let run = |args: &[&str]| -> Result<(), String> {
            let out = crate::platform::command("git")
                .current_dir(&path)
                .args(args)
                .output()
                .map_err(|e| format!("git: {e}"))?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        };
        run(&["init", "-q"])?;
        git::commit_all(&path, "Initial commit (Fallow)", None)?;
        // The features and conversations move onto branches and worktrees, as in any git
        // project (research S-6), so nothing more is written in the engineer's own checkout.
        let head = git::head(&path)?;
        let base = git::branch(&path).ok_or("git init left no branch checked out")?;
        std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
        for f in &p.features {
            let name = f.name().to_string();
            git::create_branch(&path, &name, &head)?;
            let wd = self.worktrees(&p.id).join(&name);
            git::worktree_add(&path, &wd, Some(&name), &head)?;
            if let Some(c) = f.conversation.as_ref().and_then(|c| self.convs.get_mut(c)) {
                c.append(CEvent::WorkdirSet { workdir: wd })?;
            }
            if let Some(fm) = self.project_mut(id)?.feature_mut(&name) {
                fm.base_branch = Some(base.clone());
                fm.branch = Some(name.clone());
            }
        }
        let wd = self.worktrees(&p.id).join("_project");
        if let Some(c) = p.conversation.as_ref().and_then(|c| self.convs.get_mut(c)) {
            git::worktree_add(&path, &wd, None, &head)?;
            c.append(CEvent::WorkdirSet { workdir: wd })?;
        }
        self.store.save()?;
        Ok(json!({ "kind": "git", "branch": base }))
    }

    // ---- Conversations ----------------------------------------------------------------------

    /// The project conversation, created on first use (FR-034). In a git project the lead works
    /// in a worktree of the project's current commit, so the engineer's folder stays untouched.
    pub fn project_conversation(&mut self, project: &str) -> Result<String, String> {
        let p = self.project(project)?.clone();
        if let Some(c) = p
            .conversation
            .as_ref()
            .filter(|c| self.convs.contains_key(*c))
        {
            return Ok(c.clone());
        }
        let workdir = match (p.kind(), git::head(&p.path)) {
            (project::Kind::Git, Ok(head)) => {
                let wd = self.worktrees(&p.id).join("_project");
                std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
                git::worktree_add(&p.path, &wd, None, &head)?;
                wd
            }
            _ => p.path.clone(),
        };
        let id = self.new_conv(Kind::Project, &p.id, None, None, workdir)?;
        self.project_mut(project)?.conversation = Some(id.clone());
        self.store.save()?;
        Ok(id)
    }

    pub fn set_mode(&mut self, conv: &str, mode: Mode) -> Result<Value, String> {
        let c = self.conv_mut(conv)?;
        c.append(CEvent::ModeSet { mode })?;
        Ok(json!({ "mode": mode, "label": mode.label() }))
    }

    /// Starts a turn of the lead (research S-5): one resumable call, its reply streamed.
    pub fn say(&mut self, ctx: &Ctx, conv: &str, text: &str) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("say something to the lead".into());
        }
        let c = self
            .convs
            .get(conv)
            .ok_or_else(|| format!("there is no conversation {conv}"))?;
        if c.state.busy {
            return Err("the lead is still answering".into());
        }
        let p = self.project(&c.state.project)?.clone();
        let kind = c.state.kind;
        let workdir = c.state.workdir.clone();
        let feature = c
            .state
            .feature
            .as_deref()
            .and_then(|f| p.feature(f))
            .cloned();
        if let Some(f) = &feature {
            if !f.locks.is_empty() && !f.unlocked {
                return Err("the feature is locked: unlock it to change its spec".into());
            }
        }
        let git_project = p.kind() == project::Kind::Git;
        // The lead's working copy.
        if git_project && workdir != p.path {
            match kind {
                Kind::Project => {
                    if let Ok(head) = git::head(&p.path) {
                        let _ = git::discard_changes(&workdir);
                        let _ = crate::platform::command("git")
                            .current_dir(&workdir)
                            .args(["checkout", "-q", "--detach", &head])
                            .output();
                    }
                }
                Kind::Spec => {}
            }
        }
        if let Some(f) = &feature {
            // Spec Kit's skills find the feature through this file (research S-8).
            let fj = workdir.join(".specify").join("feature.json");
            if let Some(d) = fj.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&fj, json!({ "feature_directory": f.dir }).to_string());
            let _ = std::fs::create_dir_all(workdir.join(&f.dir));
        }
        let lib = self.library(Some(&p.path));
        let def = lib
            .agent("lead")
            .cloned()
            .ok_or("there is no lead agent definition")?;
        let runtime = agent::registry::runtime(&def.runtime, &ctx.settings)
            .map_err(|e| format!("the lead cannot answer: {e}"))?;
        // The project's approved commands, with its gates (FR-007a).
        let mut approved = p.approved_commands.clone();
        let squad_text = std::fs::read_to_string(workdir.join(crate::config::squad::SQUAD_FILE))
            .unwrap_or_default();
        let mut notices = Vec::new();
        let squad = crate::config::Squad::parse(
            crate::config::squad::SQUAD_FILE,
            &squad_text,
            &mut notices,
        );
        for g in &squad.gates {
            if !approved.contains(&g.command) {
                approved.push(g.command.clone());
            }
        }
        let c = self.convs.get(conv).expect("checked");
        let mode = c.state.mode;
        let first = c.state.turns.is_empty();
        let n = c.state.turns.len() as u32 + 1;
        let edits: Vec<DocEdit> = c.state.pending_edits.clone();
        let session = match &c.state.session {
            Some(s) => Session::Resume(s.clone()),
            None => {
                let fork = c
                    .state
                    .forked_from
                    .as_ref()
                    .and_then(|f| self.convs.get(f))
                    .and_then(|f| f.state.session.clone());
                match fork {
                    Some(s) => Session::Fork(s),
                    None => {
                        Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?)
                    }
                }
            }
        };
        let prompt_edits: Vec<prompts::Edit> = edits
            .iter()
            .map(|e| prompts::Edit {
                path: &e.path,
                diff: &e.diff,
            })
            .collect();
        let instruction = prompts::lead_turn(&prompts::LeadTurn {
            project: &p.name,
            feature: feature
                .as_ref()
                .map(|f| (f.dir.as_str(), f.title.as_str(), f.step.as_str())),
            first,
            said: text,
            edits: &prompt_edits,
            plain_folder: !git_project,
        });
        // The MCP bridge for the approval tool, checked against the turn's own processes.
        let token = crate::mcp::identity::Token::new()
            .map_err(|e| e.to_string())?
            .hex();
        let group =
            Arc::new(Group::new().map_err(|e| format!("cannot create a process group: {e}"))?);
        self.tools.insert(
            token.clone(),
            Tool {
                owner: Owner::Conv(conv.to_string()),
                group: Arc::clone(&group),
            },
        );
        let c = self.convs.get_mut(conv).expect("checked");
        let mut args = vec![
            "mcp-bridge".to_string(),
            "--fallow-pid".into(),
            ctx.pid.to_string(),
            "--step".into(),
            token.clone(),
        ];
        if let Some(e) = &self.endpoint {
            args.push("--endpoint".into());
            args.push(e.clone());
        }
        let mcp = c.dir.join("mcp.json");
        std::fs::write(
            &mcp,
            json!({ "mcpServers": { "fallow": { "command": ctx.exe, "args": args } } }).to_string(),
        )
        .map_err(|e| format!("{}: {e}", mcp.display()))?;
        let call = Call {
            instruction,
            schema: Some(agent::schemas::reply()),
            rights: rights::lead(mode, approved),
            mcp_config: Some(mcp),
            session: session.clone(),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            budget_usd: Some(def.limits.usd.unwrap_or(2.0)),
            cwd: workdir,
            time_limit: Duration::from_secs(def.limits.minutes.unwrap_or(20) * 60),
            system: Some(prompts::lead_system(&def.body)),
            partial: true,
            group: Some(group),
        };
        c.append(CEvent::TurnStarted {
            n,
            said: text.to_string(),
            edits,
            mode,
            session: session.id().to_string(),
        })?;
        c.live.clear();
        c.head_before = if git_project && c.state.workdir != p.path {
            git::head(&c.state.workdir).ok()
        } else {
            None
        };
        c.activity.clear();
        c.token = Some(token);
        c.stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&c.stop);
        let tx = ctx.tx.clone();
        let id = conv.to_string();
        let turn_file = c.turns_dir().join(format!("{n}.jsonl"));
        let _ = std::fs::create_dir_all(c.turns_dir());
        std::thread::Builder::new()
            .name(format!("turn-{id}-{n}"))
            .spawn(move || {
                let started = Instant::now();
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&turn_file)
                    .ok();
                let tx2 = tx.clone();
                let id2 = id.clone();
                let mut sink = |event: agent::Event| {
                    let t = started.elapsed().as_millis() as u64;
                    if let (Some(f), Some(a)) = (file.as_mut(), agent::activity_of(&event, t)) {
                        use std::io::Write;
                        if let Ok(line) = serde_json::to_string(&a) {
                            let _ = writeln!(f, "{line}");
                        }
                    }
                    let _ = tx2.send(Msg::Studio(Note::Turn {
                        conv: id2.clone(),
                        event,
                        t,
                    }));
                };
                let outcome = runtime.start(&call, &mut sink, &stop);
                let _ = tx.send(Msg::Studio(Note::TurnDone {
                    conv: id,
                    n,
                    outcome,
                }));
            })
            .map_err(|e| format!("cannot start a thread: {e}"))?;
        Ok(json!({ "turn": n }))
    }

    /// A turn's event or its end.
    pub fn note(&mut self, note: Note) {
        match note {
            Note::Turn { conv, event, t } => {
                if let Some(c) = self.convs.get_mut(&conv) {
                    match &event {
                        agent::Event::MessageDelta { text } => c.live.push_str(text),
                        e => {
                            if let Some(a) = agent::activity_of(e, t) {
                                c.activity.push(a);
                            }
                        }
                    }
                    c.tick();
                }
            }
            Note::TurnDone { conv, n, outcome } => {
                if let Err(e) = self.turn_done(&conv, n, outcome) {
                    self.console.push(format!("conversation {conv}: {e}"));
                }
            }
        }
    }

    fn turn_done(&mut self, conv: &str, n: u32, outcome: agent::Outcome) -> Result<(), String> {
        let c = self.conv_mut(conv)?;
        if let Some(t) = c.token.take() {
            self.tools.remove(&t);
        }
        let c = self.conv_mut(conv)?;
        if let Some(p) = c.pending.take() {
            let _ = p.reply.send(Reply::Ok(
                json!({ "behavior": "deny", "message": "The turn ended." }),
            ));
        }
        let kind = c.state.kind;
        let workdir = c.state.workdir.clone();
        let project_id = c.state.project.clone();
        let feature_name = c.state.feature.clone();
        let p = self.project(&project_id)?.clone();
        let result = outcome.result.clone();
        let step_ran = result
            .as_ref()
            .and_then(|r| r["step"].as_str())
            .filter(|s| STEPS.contains(s))
            .map(str::to_string);
        // Commit the spec documents; throw away anything else (research S-6).
        let mut commit = None;
        let mut discarded = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        if p.kind() == project::Kind::Git && workdir != p.path {
            // Commits the lead made itself are undone; their changes are judged like any other
            // (Phase 3 review: Fallow alone commits, and only spec documents).
            let before = self.convs.get(conv).and_then(|c| c.head_before.clone());
            if let Some(b) = before {
                if git::head(&workdir).ok().as_deref() != Some(b.as_str()) {
                    git::reset_mixed(&workdir, &b)?;
                }
            }
            let all = git::changes_all(&workdir).unwrap_or_default();
            let is_spec = |path: &str| {
                rights::SPEC_DIRS
                    .iter()
                    .any(|d| path == *d || path.starts_with(&format!("{d}/")))
            };
            for path in all {
                let path = path.trim_matches('"').to_string();
                if path == ".specify/feature.json" || path == ".specify/" {
                    continue;
                }
                if kind == Kind::Spec && is_spec(&path) {
                    changed.push(path);
                } else {
                    discarded.push(path);
                }
            }
            if !changed.is_empty() {
                let dirs: Vec<&str> = changed.iter().map(String::as_str).collect();
                let msg = match &step_ran {
                    Some(s) => format!("spec: {s} (turn {n})"),
                    None => format!("spec: turn {n}"),
                };
                commit = git::commit_all(&workdir, &msg, Some(&dirs))?;
            }
            if !discarded.is_empty() {
                // Keep the feature file Fallow wrote; everything else not committed goes.
                let keep = std::fs::read(workdir.join(".specify/feature.json")).ok();
                git::discard_changes(&workdir)?;
                if let Some(k) = keep {
                    let _ = std::fs::write(workdir.join(".specify/feature.json"), k);
                }
            }
        } else if let Some(r) = &result {
            changed = r["documents_changed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }
        let reply = match (&result, &outcome.failure) {
            (Some(r), _) => r["reply"].as_str().unwrap_or_default().to_string(),
            (None, Some(f)) => format!("The lead's turn failed: {f}"),
            (None, None) => String::new(),
        };
        // The step the session is at moves past the one that ran; analyze records its findings.
        if let (Some(f), Some(step)) = (feature_name.as_deref(), step_ran.as_deref()) {
            let findings: Option<Vec<Finding>> = (step == "analyze").then(|| {
                result
                    .as_ref()
                    .and_then(|r| r["findings"].as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .map(|x| Finding {
                        severity: x["severity"].as_str().unwrap_or("LOW").to_string(),
                        text: x["text"].as_str().unwrap_or_default().to_string(),
                        dismissed: false,
                    })
                    .collect()
            });
            let pm = self.project_mut(&project_id)?;
            if let Some(feat) = pm.feature_mut(f) {
                let i = STEPS.iter().position(|s| *s == step).unwrap_or(0);
                let next = STEPS[(i + 1).min(STEPS.len() - 1)];
                let cur = STEPS.iter().position(|s| *s == feat.step).unwrap_or(0);
                if i + 1 > cur {
                    feat.step = next.to_string();
                }
                if let Some(fs) = findings {
                    feat.analyzed = true;
                    feat.findings = fs;
                }
            }
            self.store.save()?;
        }
        let c = self.conv_mut(conv)?;
        c.append(CEvent::TurnFinished {
            n,
            reply,
            result,
            session: outcome.session.clone(),
            documents_changed: changed,
            commit,
            discarded,
            cost_usd: outcome.cost_usd,
            failure: outcome.failure.clone(),
        })?;
        c.live.clear();
        Ok(())
    }

    // ---- Features and documents -------------------------------------------------------------

    /// Starts a feature from the project conversation (FR-034): its folder and branch, its
    /// worktree, and its spec session forked from the project conversation.
    pub fn new_feature(&mut self, project_id: &str, title: &str) -> Result<Value, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("name the feature".into());
        }
        let parent = self.project_conversation(project_id)?;
        let p = self.project(project_id)?.clone();
        let dir = p.next_dir(title);
        let name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
        let (base_branch, branch, workdir) = if p.kind() == project::Kind::Git {
            let head = git::head(&p.path).map_err(|_| {
                "the repository has no commit yet: commit something first".to_string()
            })?;
            let base = git::branch(&p.path)
                .ok_or("the project is on a detached HEAD: check out the branch to build on")?;
            git::create_branch(&p.path, &name, &head)?;
            let wd = self.worktrees(&p.id).join(&name);
            std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
            git::worktree_add(&p.path, &wd, Some(&name), &head)?;
            (Some(base), Some(name.clone()), wd)
        } else {
            (None, None, p.path.clone())
        };
        std::fs::create_dir_all(workdir.join(&dir)).map_err(|e| e.to_string())?;
        let conv = self.new_conv(Kind::Spec, &p.id, Some(name.clone()), Some(parent), workdir)?;
        // A project whose constitution is still the template starts with the constitution.
        let constitution = std::fs::read_to_string(p.path.join(".specify/memory/constitution.md"))
            .unwrap_or_default();
        let step = if constitution.is_empty() || constitution.contains("[PROJECT_NAME]") {
            "constitution"
        } else {
            "specify"
        };
        let feature = Feature {
            dir: dir.clone(),
            title: title.to_string(),
            created: now(),
            base_branch,
            branch,
            conversation: Some(conv.clone()),
            step: step.into(),
            pipeline: None,
            level: None,
            locks: Vec::new(),
            run: None,
            unlocked: false,
            ended: None,
            analyzed: false,
            findings: Vec::new(),
        };
        self.project_mut(project_id)?.features.push(feature);
        self.store.save()?;
        if let Some(c) = self.convs.get_mut(&conv) {
            let _ = c.append(CEvent::StepSet { step: step.into() });
        }
        Ok(json!({ "feature": name, "conversation": conv }))
    }

    pub(crate) fn feature_ctx(
        &self,
        project: &str,
        feature: &str,
    ) -> Result<(Project, Feature, PathBuf), String> {
        let p = self.project(project)?.clone();
        let f = p
            .feature(feature)
            .cloned()
            .ok_or_else(|| format!("there is no feature {feature} in {}", p.name))?;
        let wd = f
            .conversation
            .as_ref()
            .and_then(|c| self.convs.get(c))
            .map(|c| c.state.workdir.clone())
            .unwrap_or_else(|| p.path.clone());
        Ok((p, f, wd))
    }

    /// 007: the lead of a locked feature, for an agent's question during its loop (research
    /// S-10): the spec session to fork, the feature's worktree, and the approved commands.
    pub fn lead_of(&self, project: &str, feature: &str) -> Result<crate::run::questions::Lead, String> {
        let (p, f, wd) = self.feature_ctx(project, feature)?;
        if wd == p.path {
            return Err("the feature has no worktree of its own".into());
        }
        let session = f
            .conversation
            .as_ref()
            .and_then(|c| self.convs.get(c))
            .and_then(|c| c.state.session.clone());
        Ok(crate::run::questions::Lead {
            session,
            workdir: wd,
            approved: p.approved_commands.clone(),
        })
    }

    /// The documents of a feature the page shows and the engineer may edit.
    pub fn docs(f: &Feature) -> Vec<String> {
        let mut v: Vec<String> = [
            "spec.md",
            "plan.md",
            "tasks.md",
            "research.md",
            "data-model.md",
            "quickstart.md",
        ]
        .iter()
        .map(|d| format!("{}/{d}", f.dir))
        .collect();
        v.push(".specify/memory/constitution.md".into());
        v
    }

    fn doc_allowed(f: &Feature, path: &str) -> bool {
        !path.contains("..")
            && !path.starts_with('/')
            && !path.contains('\\')
            && (path == ".specify/memory/constitution.md"
                || path.starts_with(&format!("{}/", f.dir)))
            && (path.ends_with(".md")
                || path.ends_with(".json")
                || path.ends_with(".toml")
                || path.ends_with(".yaml"))
    }

    pub fn doc(&self, project: &str, feature: &str, path: &str) -> Result<Value, String> {
        let (_, f, wd) = self.feature_ctx(project, feature)?;
        if !Self::doc_allowed(&f, path) {
            return Err(format!("{path} is not one of the feature's documents"));
        }
        let text = std::fs::read_to_string(wd.join(path)).unwrap_or_default();
        Ok(json!({ "path": path, "text": text, "version": version_of(&text) }))
    }

    /// The engineer's edit (FR-007): refused while the lead's turn runs, or when the document
    /// changed since the page read it (research S-21); the lead receives it on its next turn.
    pub fn put_doc(
        &mut self,
        project: &str,
        feature: &str,
        path: &str,
        text: &str,
        version: &str,
    ) -> Result<Value, String> {
        let (p, f, wd) = self.feature_ctx(project, feature)?;
        if !Self::doc_allowed(&f, path) {
            return Err(format!("{path} is not one of the feature's documents"));
        }
        if f.locks.last().is_some() && !f.unlocked {
            return Err("the feature is locked: unlock it to change its documents".into());
        }
        let conv = f
            .conversation
            .clone()
            .ok_or("the feature has no spec session")?;
        let c = self.convs.get(&conv).ok_or("the spec session is missing")?;
        if c.state.busy {
            return Err(
                "the lead's turn is running: your edit is kept as a draft until it ends".into(),
            );
        }
        let file = wd.join(path);
        let old = std::fs::read_to_string(&file).unwrap_or_default();
        if version_of(&old) != version {
            return Err("the document changed since you opened it: both versions are shown".into());
        }
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        let _ = p;
        let c = self.conv_mut(&conv)?;
        // One pending edit per document since the lead last saw it, kept in the log.
        let d = match c.state.pending_edits.iter().find(|e| e.path == path) {
            Some(prev) => format!("{}{}", prev.diff, conv::diff(&old, text)),
            None => conv::diff(&old, text),
        };
        let v = version_of(text);
        c.append(CEvent::DocEdited {
            path: path.to_string(),
            version: u64::from_str_radix(&v[..8], 16).unwrap_or(0),
            diff: d,
        })?;
        Ok(json!({ "version": v }))
    }

    /// Moves the stepper (FR-006): the lead's next turn runs that step.
    pub fn set_step(&mut self, project: &str, feature: &str, to: &str) -> Result<Value, String> {
        // `done` follows analyze running; the engineer cannot set it (Phase 3 review).
        if !STEPS.contains(&to) || to == "done" {
            return Err(format!("{to:?} is not a Spec Kit step to move to"));
        }
        let p = self.project_mut(project)?;
        let f = p
            .feature_mut(feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?;
        if !f.locks.is_empty() && !f.unlocked {
            return Err("the feature is locked: unlock it to change its spec".into());
        }
        // Going back before analyze means analyze must run again.
        let at = |s: &str| STEPS.iter().position(|x| *x == s).unwrap_or(0);
        if at(to) <= at("analyze") {
            f.analyzed = false;
        }
        f.step = to.to_string();
        let conv = f.conversation.clone();
        self.store.save()?;
        if let Some(c) = conv.and_then(|c| self.convs.get_mut(&c)) {
            c.append(CEvent::StepSet { step: to.into() })?;
        }
        Ok(json!({ "step": to }))
    }

    pub fn dismiss(&mut self, project: &str, feature: &str, i: usize) -> Result<Value, String> {
        let p = self.project_mut(project)?;
        let f = p
            .feature_mut(feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?;
        let x = f.findings.get_mut(i).ok_or("there is no such finding")?;
        x.dismissed = true;
        self.store.save()?;
        Ok(json!({}))
    }

    /// The lock's checks over the documents in the feature's working copy.
    pub fn lock_check(&self, project: &str, feature: &str) -> Result<lock::Locked, Vec<String>> {
        let (p, f, wd) = self.feature_ctx(project, feature).map_err(|e| vec![e])?;
        let read = |name: &str| std::fs::read_to_string(wd.join(&f.dir).join(name)).ok();
        let mut problems = Vec::new();
        if p.kind() != project::Kind::Git {
            problems.push(
                "the project is a plain folder: a loop needs git (Fallow can set it up)".into(),
            );
        }
        let analyzed = f.analyzed;
        match lock::check(
            &f.dir,
            read("spec.md").as_deref(),
            read("plan.md").as_deref(),
            read("tasks.md").as_deref(),
            analyzed,
            &f.findings,
        ) {
            Ok(l) if problems.is_empty() => Ok(l),
            Ok(_) => Err(problems),
            Err(mut e) => {
                problems.append(&mut e);
                Err(problems)
            }
        }
    }

    /// What locking needs and does in the feature's working copy: the checks, then a commit of
    /// whatever the engineer changed since the last turn (FR-008). The engine creates the loop.
    pub fn lock_prepare(&mut self, project: &str, feature: &str) -> Result<LockPrep, Vec<String>> {
        let locked = self.lock_check(project, feature)?;
        let (p, f, wd) = self.feature_ctx(project, feature).map_err(|e| vec![e])?;
        // Every check before anything is committed (Phase 3 review).
        if !f.locks.is_empty() && !f.unlocked {
            return Err(vec!["the feature is already locked".into()]);
        }
        if f.base_branch.is_none() || wd == p.path {
            return Err(vec![
                "the feature has no branch of its own: set up git for the project first".into(),
            ]);
        }
        if let Some(c) = f.conversation.as_ref().and_then(|c| self.convs.get(c)) {
            if c.state.busy {
                return Err(vec!["the lead's turn is running".into()]);
            }
        }
        let n = f.locks.len() as u32 + 1;
        let dirs: Vec<&str> = rights::SPEC_DIRS.to_vec();
        git::commit_all(&wd, &format!("lock {n}: {}", f.title), Some(&dirs))
            .map_err(|e| vec![e])?;
        let commit = git::head(&wd).map_err(|e| vec![e])?;
        let lib = self.library(Some(&p.path));
        Ok(LockPrep {
            project: p,
            feature: f,
            lock: n,
            commit,
            phases: locked.phases,
            commands: locked.commands,
            library: lib,
        })
    }

    /// The engineer unlocked the feature (FR-009): the spec session opens again.
    pub fn mark_unlocked(&mut self, project: &str, feature: &str) -> Result<(), String> {
        let p = self.project_mut(project)?;
        let f = p
            .feature_mut(feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?;
        f.unlocked = true;
        self.store.save()
    }

    /// Records a lock and its loop (data-model.md, Lock).
    pub fn lock_record(
        &mut self,
        project: &str,
        feature: &str,
        rec: LockRec,
        run: u64,
    ) -> Result<(), String> {
        let p = self.project_mut(project)?;
        let f = p
            .feature_mut(feature)
            .ok_or_else(|| format!("there is no feature {feature}"))?;
        f.pipeline = Some(rec.pipeline.clone());
        f.level = Some(rec.level);
        f.locks.push(rec);
        f.unlocked = false;
        f.run = Some(run);
        self.store.save()
    }

    // ---- Approvals (research S-7) -----------------------------------------------------------

    /// A bridge opening: the token must be live and the peer one of the turn's own processes.
    pub fn tool_open(&self, token: &str, pid: u32) -> Result<Value, String> {
        let t = self.tools.get(token).ok_or("unknown or finished step")?;
        if !t.group.contains(pid) {
            return Err("the connecting process is not part of the step".into());
        }
        match &t.owner {
            Owner::Conv(_) => Ok(json!([{
                "name": "approve",
                "description": "Asks the engineer whether a command may run (Fallow's permission prompt).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tool_name": { "type": "string" },
                        "input": { "type": "object" },
                        "tool_use_id": { "type": "string" }
                    },
                    "required": ["tool_name", "input"]
                }
            }])),
        }
    }

    /// A tool call through the bridge. The reply goes to `reply`, now or when the engineer
    /// answers.
    pub fn tool_call(&mut self, token: &str, name: &str, args: Value, reply: Sender<Reply>) {
        let deny = |msg: &str| Reply::Ok(json!({ "behavior": "deny", "message": msg }));
        let Some(Owner::Conv(conv)) = self.tools.get(token).map(|t| t.owner.clone()) else {
            let _ = reply.send(Reply::Err("unknown or finished step".into()));
            return;
        };
        if name != "approve" {
            let _ = reply.send(Reply::Err(format!("no tool {name:?}")));
            return;
        }
        let tool = args["tool_name"].as_str().unwrap_or_default().to_string();
        let input = args["input"].clone();
        let Some(c) = self.convs.get_mut(&conv) else {
            let _ = reply.send(deny("the conversation is gone"));
            return;
        };
        if c.state.mode != Mode::Ask {
            let _ = reply.send(deny("Only Ask mode asks the engineer."));
            return;
        }
        if tool != "Bash" && tool != "PowerShell" {
            // Edits outside the spec documents and other tools are never asked about.
            let _ = reply.send(deny(&format!(
                "{tool} is not allowed here: the lead changes only spec documents."
            )));
            return;
        }
        if c.pending.is_some() {
            let _ = reply.send(deny("Another command is waiting for the engineer."));
            return;
        }
        let command = input["command"].as_str().unwrap_or_default().to_string();
        let id = format!(
            "a{}",
            c.state
                .turns
                .iter()
                .map(|t| t.approvals.len())
                .sum::<usize>()
                + 1
        );
        if c.append(CEvent::ApprovalAsked {
            id: id.clone(),
            command,
        })
        .is_err()
        {
            let _ = reply.send(deny("Fallow could not record the question."));
            return;
        }
        c.pending = Some(Pending {
            id,
            tool,
            input,
            reply,
            deadline: Instant::now() + self.approval_wait,
        });
    }

    /// The engineer's answer to a command the lead asked to run.
    pub fn approve(
        &mut self,
        conv: &str,
        id: &str,
        answer: ApprovalAnswer,
        prefix: Option<String>,
    ) -> Result<Value, String> {
        let c = self.conv_mut(conv)?;
        if c.pending.as_ref().map(|p| p.id.as_str()) != Some(id) {
            return Err("that command is no longer waiting".into());
        }
        let command = c
            .pending
            .as_ref()
            .and_then(|p| p.input["command"].as_str().map(str::to_string))
            .unwrap_or_default();
        let project = c.state.project.clone();
        let prefix = match (answer, prefix) {
            (ApprovalAnswer::Always, Some(pre)) => {
                let pre = pre.trim().to_string();
                if let Some(why) = project::commands::command_problem(&pre) {
                    return Err(format!("{pre:?}: {why}"));
                }
                if command != pre && !command.starts_with(&format!("{pre} ")) {
                    return Err(format!("{pre:?} is not the start of {command:?}"));
                }
                Some(pre)
            }
            (ApprovalAnswer::Always, None) => {
                return Err("name the command prefix to allow always".into());
            }
            _ => None,
        };
        if let Some(pre) = &prefix {
            let p = self.project_mut(&project)?;
            if !p.approved_commands.contains(pre) {
                p.approved_commands.push(pre.clone());
            }
            self.store.save()?;
        }
        let c = self.conv_mut(conv)?;
        let pending = c.pending.take().expect("checked");
        let msg = match answer {
            ApprovalAnswer::Deny => {
                json!({ "behavior": "deny", "message": "The engineer refused this command." })
            }
            _ => json!({ "behavior": "allow", "updatedInput": pending.input }),
        };
        let _ = pending.reply.send(Reply::Ok(msg));
        c.append(CEvent::ApprovalAnswered {
            id: id.to_string(),
            answer,
            prefix,
        })?;
        Ok(json!({}))
    }

    /// Denies commands nobody answered in time, and lets go of pages that stopped waiting.
    pub fn tick(&mut self) {
        for c in self.convs.values_mut() {
            c.prune();
            if c.pending
                .as_ref()
                .is_some_and(|p| Instant::now() >= p.deadline)
            {
                let p = c.pending.take().expect("checked");
                let _ = p.reply.send(Reply::Ok(json!({
                    "behavior": "deny",
                    "message": "The engineer did not answer within 10 minutes."
                })));
                let _ = c.append(CEvent::ApprovalAnswered {
                    id: p.id,
                    answer: ApprovalAnswer::Deny,
                    prefix: None,
                });
            }
        }
    }
}

/// What the engine needs to create a feature's loop at a lock.
pub struct LockPrep {
    pub project: Project,
    pub feature: Feature,
    pub lock: u32,
    pub commit: String,
    pub phases: Vec<PhaseSpec>,
    pub commands: Vec<String>,
    pub library: Library,
}
