//! Fallow Studio's projects, conversations and features (007 US1; research S-5 to S-8, S-15,
//! S-16; contracts/web-api.md). The engine owns one `Studio`; turns of the lead run on threads
//! and report back as `Note`s.

pub mod close;
pub mod commands;
pub mod control;
pub mod conv;
pub mod defs;
pub mod extras;
pub mod files;
pub mod handover;
pub mod judge;
pub mod mode;
pub mod refwatch;
pub mod repo;
pub mod solo;
pub mod solo_check;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::rights;
use crate::agent::{self, Call, Session, prompts};
use crate::conversation::events::{ApprovalAnswer, DocEdit, Event as CEvent, KeptDoc, Kind, State};
use crate::defs::Library;
use crate::platform::job::Group;
use crate::project::{self, Feature, LockRec, Project, Store};
use crate::run::engine::{Ctx, Msg, Reply};
use crate::run::events::{Level, Log, PhaseSpec};
use crate::speckit::lock::{self, Finding};
use crate::verify::git;

use conv::Conv;

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
    /// 012: the runtime of the turn, whose adapter maps its permission prompts (IX).
    pub runtime: String,
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
    /// 012 (R-12): a session's gate run ended: its results, and a baseline run if one was made.
    GatesDone {
        conv: String,
        commit: String,
        base: String,
        #[allow(clippy::type_complexity)]
        outcome: Result<
            (
                Vec<(crate::verify::gates::GateResult, String)>,
                Option<Vec<crate::verify::gates::GateResult>>,
            ),
            String,
        >,
        baseline: Option<Vec<crate::verify::gates::GateResult>>,
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
    /// 012 (FR-039): `git init` where needed and the first commit, with the ignore file the
    /// engineer accepted (or none).
    GitInit {
        id: String,
        gitignore: Option<String>,
    },
    /// 012 (FR-039): what "Make it a git repository" would commit (read only).
    GitScan {
        id: String,
    },
    /// 012 (FR-039, R-18): a project inside a bigger repository moved to its top folder;
    /// `confirmed: false` only says what would move.
    Move {
        id: String,
        confirmed: bool,
    },
    Project {
        id: String,
    },
    /// 010: read the project's remote again now (the engineer opened the project).
    CheckRemote {
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
    /// 012: a solo session from its first message (FR-001, FR-012).
    NewSession {
        project: String,
        agent: Option<String>,
        text: String,
        /// 015: the mode chosen in the composer before the first message, if any.
        mode: Option<crate::config::settings::ChatMode>,
    },
    /// 012, 015: a session's mode, `manual` or `auto`.
    SessionMode {
        session: String,
        mode: String,
    },
    /// 012: the project's sessions for the dropdown (FR-002).
    Sessions {
        project: String,
    },
    RenameSession {
        session: String,
        name: String,
    },
    OpenSession {
        session: String,
    },
    /// 012 (R-4): the way on from a failed turn commit: `fold_next` or `discard`.
    SoloPending {
        session: String,
        choice: String,
        confirmed: bool,
    },
    /// 012 (R-5): what undoing turn `turn` takes back (read only).
    UndoPreview {
        session: String,
        turn: u32,
    },
    /// 012 (FR-014, R-5): undo turn `turn` and every turn after it.
    Undo {
        session: String,
        turn: u32,
        confirmed: bool,
    },
    /// 012 (FR-019): the session's changes against its base.
    SoloChanges {
        session: String,
    },
    /// 012 (FR-017): the project's branch changes with their sources.
    GitChanges {
        project: String,
    },
    /// 012 (R-15): the feature dialog prefilled from a session's suggestion.
    FeatureDraft {
        session: String,
        suggestion: Option<u32>,
    },
    /// 012 (R-15): a feature from a session's discussion.
    FeatureFromSession {
        session: String,
        title: String,
        brief: String,
        lead: Option<String>,
    },
    /// 012 (FR-037): a feature from an earlier project conversation's proposal.
    FeatureFromConversation {
        conv: String,
        title: String,
        brief: String,
        lead: Option<String>,
    },
    /// 012 (R-12): run the project's gates on the session's head.
    SoloGates {
        session: String,
    },
    /// 012 US5 (R-8): an image for the session's next message, checked by its magic bytes.
    SoloAttach {
        session: String,
        name: String,
        bytes: Vec<u8>,
    },
    /// 012 US5 (R-8): where an attached image is kept, and its type (to serve it).
    SoloAttachment {
        session: String,
        sha: String,
    },
    /// 012 US5 (R-8): a session's message with attached images (their ids from `SoloAttach`).
    SoloSay {
        session: String,
        text: String,
        attachments: Vec<String>,
    },
    /// 012 US5 (R-9): a waiting message edited, removed, or sent now.
    SoloQueue {
        session: String,
        id: String,
        text: Option<String>,
        remove: bool,
        send: bool,
    },
    /// 012 US5 (FR-009): the session's web access on or off.
    SoloWeb {
        session: String,
        on: bool,
    },
    /// 012 US6 (R-14): close a session: without `choice`, what closing would lose (or the close
    /// of a merged and clean one); with `keep` or `discard`, the engineer's choice.
    CloseSession {
        session: String,
        choice: Option<String>,
        confirmed: bool,
        /// The digest of the question the choice answers.
        digest: Option<String>,
    },
    /// 012 US6 (R-14): a session closed with its copy kept loses the copy, confirmed.
    RemoveCopy {
        session: String,
        confirmed: bool,
    },
    /// 012 US6 (FR-025): a closed session's record deleted, confirmed.
    DeleteSession {
        session: String,
        confirmed: bool,
    },
    /// Cancels the lead's running turn; the message is withdrawn (008 T047).
    Cancel {
        conv: String,
    },
    /// The `/` menu's commands of a conversation (009 US1).
    Commands {
        conv: String,
    },
    /// 015: a spec session's mode, `manual` or `auto`.
    SetMode {
        conv: String,
        mode: mode::ChatMode,
    },
    Approve {
        conv: String,
        id: String,
        answer: ApprovalAnswer,
        prefix: Option<String>,
    },
    /// 015 (FR-007): "Switch to Auto" on a request card: allowed once, then the chat in Auto.
    ApproveAuto {
        conv: String,
        id: String,
    },
    /// 015 (R-5): the engineer's answer to a refusal: `allow_once` or `dismiss`.
    Refusal {
        conv: String,
        id: String,
        answer: String,
    },
    NewFeature {
        project: String,
        title: String,
        /// The lead the spec session starts with (009); none is the project's.
        lead: Option<String>,
    },
    /// Picks the session's lead before its first message (009 US2).
    SetLead {
        conv: String,
        lead: String,
    },
    /// 009 US5: the project's files, read-only (contracts/web-api.md "Files").
    FileVersions {
        project: String,
    },
    Files {
        project: String,
        version: String,
    },
    File {
        project: String,
        version: String,
        path: String,
        changes: bool,
    },
    OpenFile {
        project: String,
        version: String,
        path: String,
        editor: Option<files::Editor>,
        command: Option<String>,
    },
    GetEditor,
    /// 013: the engineer's theme (contracts/web-api.md).
    GetTheme,
    SetTheme {
        theme: String,
    },
    /// 016 (FR-033): the chat panels' width per screen.
    GetChatWide,
    SetChatWide {
        screens: serde_json::Map<String, Value>,
    },
    SetEditor {
        editor: Option<files::Editor>,
        command: Option<String>,
        clear: bool,
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
    /// 008: only these frontmatter fields (contracts/agents-api.md).
    PutAgentFields {
        name: String,
        scope: String,
        fields: serde_json::Map<String, Value>,
        body: Option<String>,
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
    /// The settings in force: which runtimes a lead can run on (009).
    pub settings: crate::config::Settings,
    /// Squad files as committed, per folder, with the commit read (issue #3: a session's view
    /// reads it from git again only when the project's `HEAD` moved).
    pub squads: std::cell::RefCell<BTreeMap<PathBuf, (String, crate::config::Squad)>>,
}

/// Whether a session's lead now runs on another runtime or model than the session began on
/// (FR-019a). A session from before 009 recorded neither: it is taken as unchanged.
fn lead_changed(s: &State, runtime: &str, model: &Option<String>) -> bool {
    s.session.is_some()
        && s.lead_runtime.is_some()
        && (s.lead_runtime.as_deref() != Some(runtime) || s.lead_model != *model)
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

/// Runs one turn's call on a thread of its own: each event is kept in the turn's activity file
/// and reported as a `Note::Turn`, its end as a `Note::TurnDone`. For the lead and for a solo
/// session alike.
#[allow(clippy::too_many_arguments)]
fn spawn_turn(
    tx: Sender<Msg>,
    id: String,
    n: u32,
    runtime: Box<dyn agent::Runtime>,
    call: Call,
    stop: Arc<AtomicBool>,
    turn_file: PathBuf,
    hidden: Arc<std::sync::Mutex<Vec<String>>>,
) -> Result<(), String> {
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
            let cwd = call.cwd.clone();
            let hidden_ids = hidden.clone();
            // 015 (R-10, FR-010): the calls whose output may hold secret values, by id.
            let mut hide: Vec<String> = Vec::new();
            let mut sink = |mut event: agent::Event| {
                match &mut event {
                    agent::Event::ToolCall {
                        id,
                        kind,
                        summary,
                        detail,
                    } if judge::output_may_hold_secrets(kind.as_str(), summary, detail, &cwd) => {
                        hide.push(id.clone());
                    }
                    // A refusal's reason is Fallow's own text, shown as it is.
                    agent::Event::ToolResult {
                        id,
                        output,
                        refused: false,
                        ..
                    } if hide.contains(id) || hidden_ids.lock().is_ok_and(|h| h.contains(id)) => {
                        *output = mode::HIDDEN_OUTPUT.to_string();
                    }
                    _ => {}
                }
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
        .map(|_| ())
        .map_err(|e| format!("cannot start a thread: {e}"))
}

fn conv_dir(data: &Path, id: &str) -> PathBuf {
    data.join("conversations").join(id)
}

/// Undoes what the lead did in a cancelled turn (008 T047): its working copy goes back to where
/// it stood when the turn began (the lead's own commits too, and every file it wrote), and the
/// engineer's edited documents, not committed yet, are put back. Best effort: answers what could
/// not be undone, for the chat and the lead's next turn. In a plain folder (or a repository with
/// no commit yet) the lead works in the engineer's own files, so nothing is undone.
/// 012 (Phase 5 review, 1): a working copy of Fallow's own is a linked worktree, whose `.git`
/// is a file; the engineer's checkout (its `.git` a folder, or none) never is, wherever the
/// project's folder is now. Only in one does Fallow reset, discard or commit for the lead.
pub(crate) fn own_copy(workdir: &Path) -> bool {
    workdir.join(".git").is_file()
}

fn undo(
    p: &Project,
    workdir: &Path,
    cancel: &crate::conversation::events::Cancel,
) -> Option<String> {
    let _ = p;
    // 012 (FR-041): every project is a git repository; the lead always works in a worktree.
    if !own_copy(workdir) {
        return Some(
            "Fallow cannot undo the lead's changes here: what it changed before you cancelled stays".into(),
        );
    }
    let mut failed = Vec::new();
    if let Some(b) = &cancel.head {
        if git::head(workdir).ok().as_deref() != Some(b.as_str()) {
            if let Err(e) = git::reset_mixed(workdir, b) {
                failed.push(e);
            }
        }
    }
    let feature_json = workdir.join(p.in_root(".specify/feature.json"));
    let keep = std::fs::read(&feature_json).ok();
    if let Err(e) = git::discard_changes(workdir) {
        failed.push(e);
    }
    if let Some(k) = keep {
        let _ = std::fs::write(&feature_json, k);
    }
    let _ = project::install::scaffold(workdir);
    for d in &cancel.kept {
        let path = workdir.join(&d.path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, &d.text) {
            failed.push(format!("{}: {e}", d.path));
        }
    }
    (!failed.is_empty()).then(|| {
        format!(
            "Fallow could not undo everything the lead changed: {}",
            failed.join("; ")
        )
    })
}

impl Studio {
    /// Loads the projects and every conversation. A conversation whose turn was cut off by a
    /// stop or a crash gets that turn closed, so it can go on (constitution VII).
    pub fn load(
        data: &Path,
        config: Option<PathBuf>,
        settings: crate::config::Settings,
    ) -> Result<Studio, String> {
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
            settings,
            squads: std::cell::RefCell::default(),
        };
        // 012 (R-2, R-3): the empty folder Fallow's own git commands on a session use as their
        // hooks path, so no hook runs.
        git::set_nohooks(data.join("nohooks"))?;
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
                    // 015 (R-6): every chat starts in the default mode after a restart.
                    mode::at_load(&mut c, s.settings.defaults.chat_mode);
                    // 012 (R-9): after a restart, waiting messages wait for the engineer.
                    c.queue_hold = c.state.solo.as_ref().is_some_and(|s| !s.queue.is_empty());
                    if c.state.kind == Kind::Solo && c.state.busy {
                        // 012 (FR-026, review 1): what the turn changed is committed as cut
                        // short; a session's turn is never withdrawn.
                        solo::cut_on_load(&mut c);
                    } else if let Some(cancel) = c.state.cancelling().cloned() {
                        let n = c.state.turns.last().map_or(0, |t| t.n);
                        let failure = match s.store.get(&c.state.project) {
                            Some(p) => undo(p, &c.state.workdir, &cancel),
                            None => Some("the project is gone: nothing was undone".into()),
                        };
                        let _ = c.append(CEvent::TurnFinished {
                            n,
                            reply: String::new(),
                            result: None,
                            session: None,
                            documents_changed: Vec::new(),
                            commit: None,
                            discarded: Vec::new(),
                            cost_usd: None,
                            failure,
                            withdrawn: true,
                            commits: Vec::new(),
                            commit_failed: None,
                            changed: Vec::new(),
                            cut: false,
                            folded_into: None,
                        });
                    } else if c.state.busy {
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
                                withdrawn: false,
                                commits: Vec::new(),
                                commit_failed: None,
                                changed: Vec::new(),
                                cut: false,
                                folded_into: None,
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
        // 012: sessions made halfway, gone, or whose mode was reset (R-2, R-19).
        s.solo_on_load();
        Ok(s)
    }

    pub fn library(&self, project: Option<&Path>) -> Library {
        // 012 (R-18): a moved project's `.fallow/` stays at its spec root.
        let root = project.and_then(|path| {
            self.store
                .projects
                .iter()
                .find(|p| p.path == path && !p.spec_root.is_empty())
                .map(|p| p.root_dir())
        });
        Library::load(self.config.as_deref(), root.as_deref().or(project))
    }

    /// Why `name` cannot be a lead in the project at `path`, if it cannot (009, FR-019, FR-020):
    /// it is gone, it is not of kind Lead, or its runtime is not available.
    pub fn lead_problem(&self, path: &Path, name: &str) -> Option<String> {
        let lib = self.library(Some(path));
        let Some(def) = lib.agent(name) else {
            return Some(format!(
                "the lead {name} is gone: there is no agent of that name"
            ));
        };
        if def.kind != crate::defs::agent::Kind::Lead {
            return Some(format!("{name} is not a lead"));
        }
        agent::registry::runtime(&def.runtime, &self.settings)
            .err()
            .map(|e| format!("{name}'s runtime is not available: {e}"))
    }

    /// The lead a new session of project `p` starts with: its chosen lead while that can lead,
    /// else `lead` (`None`).
    pub fn default_lead(&self, p: &Project) -> Option<String> {
        p.lead
            .clone()
            .filter(|l| self.lead_problem(&p.path, l).is_none())
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
        lead: Option<String>,
    ) -> Result<String, String> {
        self.new_conv_with(
            kind,
            project,
            feature,
            forked_from,
            workdir,
            lead,
            None,
            None,
        )
    }

    /// `new_conv` with 012's `agent` (a solo session's assistant) and `origin` (the session a
    /// spec session was started from).
    #[allow(clippy::too_many_arguments)]
    fn new_conv_with(
        &mut self,
        kind: Kind,
        project: &str,
        feature: Option<String>,
        forked_from: Option<String>,
        workdir: PathBuf,
        lead: Option<String>,
        agent: Option<String>,
        origin: Option<String>,
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
            mode_changes: Vec::new(),
            step: None,
            turns: Vec::new(),
            busy: false,
            cost_usd: 0.0,
            seq: 0,
            pending_edits: Vec::new(),
            lead: lead.clone(),
            lead_runtime: None,
            lead_model: None,
            lead_pending: None,
            agent: agent.clone(),
            origin: origin.clone(),
            solo: (kind == Kind::Solo).then(crate::conversation::solo::Solo::default),
        };
        let mut c = Conv::new(dir, log, state);
        // 015 (FR-005): a new chat starts in the default mode.
        c.mode = self.settings.defaults.chat_mode;
        let _ = std::fs::write(c.dir.join("mode"), c.mode.name());
        c.append(CEvent::ConversationCreated {
            kind,
            project: project.to_string(),
            feature,
            forked_from,
            workdir,
            lead,
            agent,
            origin,
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
        // Every key is checked on a copy before any is applied (009 T005): a refused key leaves
        // the project as it was.
        let mut p = self.project(id)?.clone();
        if let Some(v) = patch.get("path").and_then(Value::as_str) {
            let path = PathBuf::from(v);
            if !path.is_dir() {
                return Err(format!("{v} is not a folder"));
            }
            if !p.spec_root.is_empty() && path != p.path {
                return Err("this project was moved to its repository's top folder: its folder cannot be changed here".into());
            }
            p.path = path;
        }
        if let Some(v) = patch.get("pipeline").and_then(Value::as_str) {
            // Only a pipeline in force for the project, saved and checked (FR-027, FR-028).
            if !self.library(Some(&p.root_dir())).pipelines.contains_key(v) {
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
        if let Some(v) = patch.get("lead").and_then(Value::as_str) {
            // An agent of kind Lead of the project's library, on a runtime that is there (009,
            // FR-013, FR-020).
            if self.library(Some(&p.root_dir())).agent(v).is_none() {
                return Err(format!("there is no lead {v} for this project"));
            }
            if let Some(why) = self.lead_problem(&p.path, v) {
                return Err(why);
            }
            p.lead = Some(v.to_string()).filter(|l| l != "lead");
        }
        // 012 (FR-005): the assistant new sessions start with.
        if let Some(v) = patch.get("assistant").and_then(Value::as_str) {
            if let Some(why) = self.assistant_problem(&p.path, v) {
                return Err(why);
            }
            p.assistant = Some(v.to_string()).filter(|a| a != solo::ASSISTANT);
        }
        // 015 (2026-11-24 amendment): the engineer removes what they allowed always past a
        // limit; entries are only added from a card ("Always allow"), never here.
        if let Some(v) = patch.get("allowed_past_limits") {
            let keep: Vec<project::PastLimit> = serde_json::from_value(v.clone())
                .map_err(|e| format!("allowed_past_limits: {e}"))?;
            if let Some(e) = keep.iter().find(|e| !p.allowed_past_limits.contains(e)) {
                return Err(format!(
                    "{:?} is not allowed past a limit: it is only added from a card",
                    e.prefix
                ));
            }
            p.allowed_past_limits.retain(|e| keep.contains(e));
        }
        *self.project_mut(id)? = p;
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
        // 012 (FR-041): always a worktree of the project's commit; never the folder itself.
        let head = git::head(&p.path).map_err(|_| format!("{} has no commit yet", p.name))?;
        let workdir = {
            let wd = self.worktrees(&p.id).join("_project");
            std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
            git::worktree_add(&p.path, &wd, None, &head)?;
            wd
        };
        let lead = self.default_lead(&p);
        let id = self.new_conv(Kind::Project, &p.id, None, None, workdir, lead)?;
        self.project_mut(project)?.conversation = Some(id.clone());
        self.store.save()?;
        Ok(id)
    }

    /// A conversation as the page shows it, with its lead and the leads it could pick (009 US2,
    /// data-model.md "Derived in the view").
    pub fn conv_view(&self, id: &str) -> Option<Value> {
        let c = self.convs.get(id)?;
        let mut v = c.view();
        v["default_mode"] = json!(self.settings.defaults.chat_mode.name());
        if c.state.kind == Kind::Solo {
            self.solo_view(c, &mut v);
            return Some(v);
        }
        let Ok(p) = self.project(&c.state.project) else {
            return Some(v);
        };
        let lib = self.library(Some(&p.path));
        let name = c.state.lead_name();
        let def = lib.agent(name);
        let runtime = def.map(|d| d.runtime.clone()).unwrap_or_default();
        let model = def.map(|d| d.model.clone()).filter(|m| !m.is_empty());
        let changed = def.is_some() && lead_changed(&c.state, &runtime, &model);
        v["lead"] = json!({
            "name": name,
            "runtime": runtime,
            "model": model,
            "fixed": c.state.lead_fixed(),
            "changed": changed,
            "problem": self.lead_problem(&p.path, name),
        });
        v["leads"] = Value::Array(
            lib.agents
                .values()
                .filter(|a| a.kind == crate::defs::agent::Kind::Lead)
                .map(|a| {
                    let reason = agent::registry::runtime(&a.runtime, &self.settings)
                        .err()
                        .map(|e| format!("its runtime is not available: {e}"));
                    json!({
                        "name": a.name,
                        "runtime": a.runtime,
                        "model": Some(a.model.clone()).filter(|m| !m.is_empty()),
                        "available": reason.is_none(),
                        "reason": reason,
                    })
                })
                .collect(),
        );
        Some(v)
    }

    /// Picks the lead of a session that has not started (009 US2, FR-014, FR-015).
    pub fn set_lead(&mut self, conv: &str, lead: &str) -> Result<Value, String> {
        let c = self
            .convs
            .get(conv)
            .ok_or_else(|| format!("there is no conversation {conv}"))?;
        if c.state.busy {
            return Err("Wait until the lead has answered".into());
        }
        if c.state.lead_fixed() {
            return Err(
                "A session keeps its lead; start a new feature or session to use another".into(),
            );
        }
        let path = self.project(&c.state.project)?.path.clone();
        if let Some(why) = self.lead_problem(&path, lead) {
            return Err(why);
        }
        let c = self.conv_mut(conv)?;
        c.append(CEvent::LeadSet {
            lead: lead.to_string(),
        })?;
        Ok(json!({}))
    }

    /// The `/` menu's commands of a conversation (009 US1, contracts/web-api.md "Commands").
    pub fn commands(&self, conv: &str) -> Result<Value, String> {
        let c = self
            .convs
            .get(conv)
            .ok_or_else(|| format!("there is no conversation {conv}"))?;
        let p = self.project(&c.state.project)?;
        let workdir = &c.state.workdir;
        // A working copy of its own is Fallow's worktree of a git project (no git call here: the
        // menu opens as the engineer types, SC-002). As at every turn, Spec Kit's skills are in it.
        if own_copy(workdir)
            && !workdir
                .join(".claude/skills/speckit-specify/SKILL.md")
                .is_file()
        {
            project::install::scaffold(workdir)?;
        }
        let mut list = commands::catalogue(
            &p.root_dir(),
            &workdir.join(&p.spec_root),
            c.state.kind == Kind::Project,
        );
        // 012 (R-15): a session offers Fallow's own commands and the project's, not Spec Kit's
        // steps (those are a spec session's).
        if c.state.kind == Kind::Solo {
            list.retain(|x| x.group == commands::Group::Project);
            list.insert(
                0,
                commands::fallow(
                    "close",
                    "Close this session: merge it, keep its branch, or discard it",
                    true,
                    None,
                ),
            );
            list.insert(
                0,
                commands::fallow(
                    "feature",
                    "Ask the assistant to suggest a feature from this discussion",
                    true,
                    None,
                ),
            );
        }
        Ok(json!({ "commands": list }))
    }

    /// 015 (FR-001, FR-006, research R-6, R-8): a chat's mode, for the lead's spec sessions and
    /// the assistant's solo sessions alike: recorded for the chat's note, kept in memory and in
    /// the mark that words the note after a restart; a request waiting for the engineer is
    /// answered as Auto would answer it.
    pub fn set_mode(&mut self, conv: &str, mode: mode::ChatMode) -> Result<Value, String> {
        let c = self.conv_mut(conv)?;
        if c.state.solo.as_ref().is_some_and(|s| s.closed.is_some()) {
            return Err("this session is closed".into());
        }
        let changed = c.mode != mode;
        c.mode = mode;
        c.mode_note = None;
        let _ = std::fs::write(c.dir.join("mode"), mode.name());
        if changed {
            c.append(CEvent::ChatMode { mode })?;
        } else {
            c.tick();
        }
        if mode == mode::ChatMode::Auto {
            self.pending_under_auto(conv);
        }
        Ok(json!({ "mode": mode.name(), "label": mode.label() }))
    }

    /// Starts a turn of the lead (research S-5): one resumable call, its reply streamed.
    pub fn say(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
    ) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("say something to the lead".into());
        }
        let c = self
            .convs
            .get(conv)
            .ok_or_else(|| format!("there is no conversation {conv}"))?;
        // 012: a solo session's turn is its own (studio/solo.rs).
        if c.state.kind == Kind::Solo {
            return self.say_solo(ctx, conv, text, refs);
        }
        if c.state.kind == Kind::Project {
            return Err("This conversation is read-only: the project conversation with the lead is retired. Start a session, or a feature".into());
        }
        if c.state.busy {
            return Err("the lead is still answering".into());
        }
        // 009: `/specify <title>` in the project conversation starts the feature, as "+ New
        // feature" does, and runs specify in its spec session (clarify answer 1).
        if c.state.kind == Kind::Project {
            if let Some(rest) = text
                .strip_prefix("/specify")
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
            {
                let title = rest.trim().to_string();
                if title.is_empty() {
                    return Err("Give the feature a name: /specify <name>".into());
                }
                let project = c.state.project.clone();
                let p = self.project(&project)?;
                let lead = self.default_lead(p).unwrap_or_else(|| "lead".into());
                if let Some(why) = self.lead_problem(&p.path, &lead) {
                    return Err(why);
                }
                let mut f = self.new_feature(&project, &title, None)?;
                let spec = f["conversation"].as_str().unwrap_or_default().to_string();
                // The feature exists now: if specify cannot start, the engineer opens it anyway.
                if let Err(e) = self.say(ctx, &spec, &format!("/specify {title}"), Vec::new()) {
                    f["warning"] = json!(format!(
                        "The feature was made, but specify did not start: {e}"
                    ));
                }
                return Ok(f);
            }
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
        // The lead's working copy (012 FR-041: every project is a git repository).
        if own_copy(&workdir) {
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
                Kind::Spec | Kind::Solo => {}
            }
        }
        if own_copy(&workdir) {
            // Spec Kit, installed into the engineer's folder but not committed, is not in a
            // worktree: the lead gets its skills and templates there too, never committed.
            project::install::scaffold(&workdir.join(&p.spec_root))?;
        }
        if let Some(f) = &feature {
            // Spec Kit's skills find the feature through this file (research S-8), relative to
            // its own root (012 R-18).
            let fj = workdir.join(p.in_root(".specify/feature.json"));
            if let Some(d) = fj.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let rel = f
                .dir
                .strip_prefix(project::feature_root(&f.dir))
                .unwrap_or(&f.dir);
            let _ = std::fs::write(&fj, json!({ "feature_directory": rel }).to_string());
            let _ = std::fs::create_dir_all(workdir.join(&f.dir));
        }
        // 009 US1: a message whose first word names a command of the `/` menu runs it; any other
        // message is plain text (FR-005).
        let catalogue = commands::catalogue(
            &p.root_dir(),
            &workdir.join(&p.spec_root),
            kind == Kind::Project,
        );
        let command = match commands::resolve(&catalogue, text) {
            Some((cmd, _)) if !cmd.available => {
                return Err(format!(
                    "/{} cannot be sent: {}",
                    cmd.name,
                    cmd.reason.as_deref().unwrap_or("it is not available")
                ));
            }
            Some((cmd, input)) => Some(match cmd.group {
                commands::Group::SpecKit => prompts::CommandUse::Step {
                    step: cmd.name.clone(),
                    path: cmd.source.clone(),
                    input,
                },
                commands::Group::Project => prompts::CommandUse::Project {
                    name: cmd.name.clone(),
                    source: cmd.source.clone(),
                    instructions: commands::instructions(cmd, &input)?,
                },
                // A session's own commands never reach a spec session's lead.
                commands::Group::Fallow => {
                    return Err(format!("/{} is a session's command", cmd.name));
                }
            }),
            None => None,
        };
        // 009 US2: the session's own lead, refused with the reason when it cannot lead (FR-019).
        let lead_name = self
            .convs
            .get(conv)
            .map(|c| c.state.lead_name().to_string())
            .unwrap_or_else(|| "lead".into());
        if let Some(why) = self.lead_problem(&p.path, &lead_name) {
            return Err(why);
        }
        let lib = self.library(Some(&p.path));
        let def = lib
            .agent(&lead_name)
            .cloned()
            .ok_or_else(|| format!("there is no lead {lead_name}"))?;
        let runtime = agent::registry::runtime(&def.runtime, &ctx.settings)
            .map_err(|e| format!("the lead cannot answer: {e}"))?;
        let def_model = Some(def.model.clone()).filter(|m| !m.is_empty());
        // The project's approved commands, with its gates (FR-007a).
        let mut approved = p.approved_commands.clone();
        let squad_text =
            std::fs::read_to_string(workdir.join(p.in_root(crate::config::squad::SQUAD_FILE)))
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
        // Spec Kit's own scripts run without asking in a spec session (008 T040); Fallow makes
        // features itself, so the script that creates one still asks.
        if feature.is_some() {
            for s in project::commands::speckit_scripts(&workdir) {
                if !s.contains("create-new-feature") && !approved.contains(&s) {
                    approved.push(s);
                }
            }
        }
        let c = self.convs.get(conv).expect("checked");
        let first = c.state.turns.iter().all(|t| t.withdrawn) && c.state.session.is_none();
        // The messages the engineer withdrew since the lead last answered (008 T047): a resumed
        // session may still hold them.
        let mut withdrawn: Vec<(String, bool)> = c
            .state
            .turns
            .iter()
            .rev()
            .take_while(|t| t.withdrawn)
            .map(|t| (t.said.clone(), t.failure.is_none()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        // A new session never saw them: nothing to withdraw there.
        if c.state.session.is_none() {
            withdrawn.clear();
        }
        let n = c.state.turns.len() as u32 + 1;
        let edits: Vec<DocEdit> = c.state.pending_edits.clone();
        // A session cannot move to another runtime or model (FR-019a): when the lead's changed
        // since the session began, this turn starts a new one with it.
        let changed = lead_changed(&c.state, &def.runtime, &def_model);
        let new = || {
            crate::platform::random::uuid_v4()
                .map(Session::New)
                .map_err(|e| e.to_string())
        };
        let session = match &c.state.session {
            Some(s) if !changed => Session::Resume(s.clone()),
            Some(_) => new()?,
            None => {
                // A spec session forks the project conversation's session only when both talk
                // to the same lead on the same runtime and model.
                let fork = c
                    .state
                    .forked_from
                    .as_ref()
                    .and_then(|f| self.convs.get(f))
                    .filter(|f| {
                        f.state.lead_name() == lead_name
                            && !lead_changed(&f.state, &def.runtime, &def_model)
                    })
                    .and_then(|f| f.state.session.clone());
                match fork {
                    Some(s) => Session::Fork(s),
                    None => new()?,
                }
            }
        };
        if changed {
            withdrawn.clear();
        }
        let prompt_edits: Vec<prompts::Edit> = edits
            .iter()
            .map(|e| prompts::Edit {
                path: &e.path,
                diff: &e.diff,
            })
            .collect();
        // 012 (FR-042, research R-7): the project's instruction files, read from the working
        // copy, handed with a session's first turn and again when one of them changed.
        let instr =
            agent::instructions::read(&workdir, &agent::instructions::extra_at_head(&p.root_dir()));
        let now: Vec<(String, String)> = instr
            .iter()
            .map(|i| (i.path.clone(), i.sha.clone()))
            .collect();
        let hand = !matches!(session, Session::Resume(_)) || now != c.state.last_instructions();
        let instr_text = if hand {
            agent::instructions::block(&instr)
        } else {
            String::new()
        };
        let instr_recs: Vec<crate::conversation::events::InstructionRec> = if hand {
            instr
                .iter()
                .map(|i| crate::conversation::events::InstructionRec {
                    path: i.path.clone(),
                    sha: i.sha.clone(),
                    cut: i.cut,
                    skipped: i.skipped.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let instruction = prompts::lead_turn(&prompts::LeadTurn {
            project: &p.name,
            feature: feature
                .as_ref()
                .map(|f| (f.dir.as_str(), f.title.as_str(), f.step.as_str())),
            first,
            said: text,
            edits: &prompt_edits,
            withdrawn: &withdrawn,
            command,
            refs: &refs,
            instructions: &instr_text,
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
                runtime: def.runtime.clone(),
            },
        );
        let c = self.convs.get_mut(conv).expect("checked");
        // 015 (hardening): Fallow, not a runtime rule, runs these without asking.
        c.lead_runs = approved;
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
            rights: rights::lead(),
            mcp_config: Some(mcp),
            session: session.clone(),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            budget_usd: Some(def.limits.usd.unwrap_or(2.0)),
            // 012 (R-18): Spec Kit runs from its own root, a subfolder for a moved project.
            cwd: workdir.join(&p.spec_root),
            time_limit: Duration::from_secs(def.limits.minutes.unwrap_or(20) * 60),
            system: Some(prompts::lead_system(&def.body)),
            partial: true,
            group: Some(group),
            attachments: Vec::new(),
            web: false,
            user_settings: false,
        };
        c.append(CEvent::TurnStarted {
            n,
            said: text.to_string(),
            edits,
            mode: c.mode.name().to_string(),
            session: session.id().to_string(),
            runtime: Some(def.runtime.clone()),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            // A new session or a fork: a session of its own, on this runtime and model.
            new_session: !matches!(session, Session::Resume(_)),
            instructions: instr_recs,
            attachments: Vec::new(),
            slot_waited_ms: None,
            head: None,
        })?;
        c.live.clear();
        c.answered.clear();
        c.allowed_paths.clear();
        if let Ok(mut h) = c.hidden.lock() {
            h.clear();
        }
        c.head_before = if own_copy(&c.state.workdir) {
            git::head(&c.state.workdir).ok()
        } else {
            None
        };
        c.activity.clear();
        c.token = Some(token);
        c.kept = c
            .state
            .turns
            .last()
            .map(|t| t.edits.clone())
            .unwrap_or_default()
            .iter()
            .filter_map(|e| {
                std::fs::read_to_string(c.state.workdir.join(&e.path))
                    .ok()
                    .map(|text| KeptDoc {
                        path: e.path.clone(),
                        text,
                    })
            })
            .collect();
        c.stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&c.stop);
        let turn_file = c.turns_dir().join(format!("{n}.jsonl"));
        let _ = std::fs::create_dir_all(c.turns_dir());
        spawn_turn(
            ctx.tx.clone(),
            conv.to_string(),
            n,
            runtime,
            call,
            stop,
            turn_file,
            self.convs
                .get(conv)
                .map(|c| c.hidden.clone())
                .unwrap_or_default(),
        )?;
        Ok(json!({ "turn": n }))
    }

    /// Cancels the running turn (008 T047): the lead's call is stopped, and when it has ended
    /// the turn is withdrawn and its changes undone. Answers with the message, for the page to
    /// put back in the box.
    pub fn cancel(&mut self, conv: &str) -> Result<Value, String> {
        let c = self.conv_mut(conv)?;
        if !c.state.busy {
            return Err("the lead is not answering".into());
        }
        if c.state.cancelling().is_none() {
            let n = c.state.turns.last().map_or(0, |t| t.n);
            let (head, kept) = (c.head_before.clone(), c.kept.clone());
            c.append(CEvent::CancelAsked { n, head, kept })?;
        }
        c.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(p) = c.pending.take() {
            let _ = p.reply.send(Reply::Ok(
                p.answer(false, "The engineer cancelled this turn."),
            ));
        }
        let said = c
            .state
            .turns
            .last()
            .map(|t| t.said.clone())
            .unwrap_or_default();
        c.tick();
        Ok(json!({ "said": said }))
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
                    // 015 (R-3): an edit the runtime refused by its own rule (its settings
                    // files), which Fallow never answered: shown as a refusal, never allowed.
                    if let agent::Event::ToolResult {
                        id, refused: true, ..
                    } = &event
                    {
                        mode::runtime_refused(c, id);
                    }
                    c.tick();
                }
            }
            Note::TurnDone { conv, n, outcome } => {
                if let Err(e) = self.turn_done(&conv, n, outcome) {
                    self.console.push(format!("conversation {conv}: {e}"));
                }
            }
            Note::GatesDone {
                conv,
                commit,
                base,
                outcome,
                baseline,
            } => {
                if let Err(e) = self.solo_gates_done(&conv, commit, base, outcome, baseline) {
                    self.console.push(format!("session {conv}: {e}"));
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
            let _ = p.reply.send(Reply::Ok(p.answer(false, "The turn ended.")));
        }
        let kind = c.state.kind;
        if kind == Kind::Solo {
            return self.solo_turn_done(conv, n, outcome);
        }
        let workdir = c.state.workdir.clone();
        let project_id = c.state.project.clone();
        let feature_name = c.state.feature.clone();
        let cancel = c.state.cancelling().cloned();
        let p = self.project(&project_id)?.clone();
        if let Some(cancel) = cancel {
            return self.withdraw(conv, n, &p, &workdir, &cancel, outcome);
        }
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
        if own_copy(&workdir) {
            // Commits the lead made itself are undone; their changes are judged like any other
            // (Phase 3 review: Fallow alone commits, and only spec documents).
            let before = self.convs.get(conv).and_then(|c| c.head_before.clone());
            if let Some(b) = before {
                if git::head(&workdir).ok().as_deref() != Some(b.as_str()) {
                    git::reset_mixed(&workdir, &b)?;
                }
            }
            let all = git::changes_all(&workdir).unwrap_or_default();
            let dirs: Vec<String> = rights::SPEC_DIRS.iter().map(|d| p.in_root(d)).collect();
            let is_spec = |path: &str| {
                dirs.iter()
                    .any(|d| path == d || path.starts_with(&format!("{d}/")))
            };
            let root = p.in_root("");
            // 015 (review of US2, 3): what the engineer allowed the lead once this turn stays.
            let allowed = self
                .convs
                .get(conv)
                .map(|c| c.allowed_paths.clone())
                .unwrap_or_default();
            for path in all {
                let path = path.trim_matches('"').to_string();
                let in_root = path.strip_prefix(root.as_str()).unwrap_or(&path);
                if in_root == ".specify/feature.json"
                    || in_root == ".specify/"
                    || project::install::is_scaffold(in_root)
                {
                    continue;
                }
                if kind == Kind::Spec && (is_spec(&path) || allowed.contains(&path)) {
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
                let fj = workdir.join(p.in_root(".specify/feature.json"));
                let keep = std::fs::read(&fj).ok();
                git::discard_changes(&workdir)?;
                if let Some(k) = keep {
                    let _ = std::fs::write(&fj, k);
                }
                let _ = project::install::scaffold(&workdir.join(&p.spec_root));
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
                    feat.analyze_outdated = false;
                    feat.findings = fs;
                }
            }
            self.store.save()?;
        }
        // A document changed in a turn that was not analyze: analyze must run again before the
        // lock (T086: edits after analyze were locked unchecked).
        if let Some(f) = feature_name.as_deref() {
            if !changed.is_empty() && step_ran.as_deref() != Some("analyze") {
                self.analyze_again(&project_id, f)?;
            }
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
            withdrawn: false,
            commits: Vec::new(),
            commit_failed: None,
            changed: Vec::new(),
            cut: false,
            folded_into: None,
        })?;
        c.live.clear();
        c.answered.clear();
        c.allowed_paths.clear();
        if let Ok(mut h) = c.hidden.lock() {
            h.clear();
        }
        Ok(())
    }

    /// The end of a cancelled turn: what the lead did for it is undone (see `undo`) and the turn
    /// is recorded as withdrawn. Its session is not recorded: a call stopped early may not have
    /// saved one, and the session the conversation had before stays.
    fn withdraw(
        &mut self,
        conv: &str,
        n: u32,
        p: &Project,
        workdir: &Path,
        cancel: &crate::conversation::events::Cancel,
        outcome: agent::Outcome,
    ) -> Result<(), String> {
        let failure = undo(p, workdir, cancel);
        let c = self.conv_mut(conv)?;
        c.kept.clear();
        c.append(CEvent::TurnFinished {
            n,
            reply: String::new(),
            result: None,
            session: None,
            documents_changed: Vec::new(),
            commit: None,
            discarded: Vec::new(),
            cost_usd: outcome.cost_usd,
            failure,
            withdrawn: true,
            commits: Vec::new(),
            commit_failed: None,
            changed: Vec::new(),
            cut: false,
            folded_into: None,
        })?;
        c.live.clear();
        c.answered.clear();
        c.allowed_paths.clear();
        if let Ok(mut h) = c.hidden.lock() {
            h.clear();
        }
        Ok(())
    }

    // ---- Features and documents -------------------------------------------------------------

    /// Starts a feature from the project conversation (FR-034): its folder and branch, its
    /// worktree, and its spec session forked from the project conversation.
    pub fn new_feature(
        &mut self,
        project_id: &str,
        title: &str,
        lead: Option<String>,
    ) -> Result<Value, String> {
        self.new_feature_with(project_id, title, lead, None)
    }

    /// `new_feature`, with where it was started from (012 FR-034: `solo:<s>` or `conv:<c>`).
    /// Its spec session starts an agent session of its own: it forks from no project
    /// conversation (FR-035).
    pub fn new_feature_with(
        &mut self,
        project_id: &str,
        title: &str,
        lead: Option<String>,
        origin: Option<String>,
    ) -> Result<Value, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("name the feature".into());
        }
        // One feature per name: a proposal started twice, or a double click, opens no second
        // one (009, FR-042). Names compare without case and with spaces collapsed.
        let same = |t: &str| {
            t.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        if let Some(f) = self
            .project(project_id)?
            .features
            .iter()
            .find(|f| same(&f.title) == same(title))
        {
            let n = f.dir.rsplit('/').next().unwrap_or(&f.dir);
            return Err(format!(
                "a feature named \"{}\" exists already ({n})",
                f.title
            ));
        }
        // The engineer's pick, or the project's lead (009, FR-014).
        let lead = match lead {
            Some(l) => {
                let p = self.project(project_id)?;
                if let Some(why) = self.lead_problem(&p.path, &l) {
                    return Err(why);
                }
                Some(l).filter(|l| l != "lead")
            }
            None => self.default_lead(self.project(project_id)?),
        };
        let p = self.project(project_id)?.clone();
        let dir = p.next_dir(title);
        let name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
        // 012 (FR-041): a feature always has its branch and worktree.
        let (base_branch, branch, workdir) = {
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
        };
        std::fs::create_dir_all(workdir.join(&dir)).map_err(|e| e.to_string())?;
        let conv = self.new_conv_with(
            Kind::Spec,
            &p.id,
            Some(name.clone()),
            None,
            workdir,
            lead,
            None,
            origin.clone(),
        )?;
        // A project whose constitution is still the template starts with the constitution.
        let constitution =
            std::fs::read_to_string(p.root_dir().join(".specify/memory/constitution.md"))
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
            analyze_outdated: false,
            findings: Vec::new(),
            origin,
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
            // Without a chat: the feature's own worktree, when it has one (012 R-18).
            .or_else(|| {
                let w = self.worktrees(&p.id).join(f.name());
                w.is_dir().then_some(w)
            })
            .unwrap_or_else(|| p.path.clone());
        Ok((p, f, wd))
    }

    /// 007: the lead of a locked feature, for an agent's question during its loop (research
    /// S-10): the spec session to fork, the feature's worktree, and the approved commands.
    pub fn lead_of(
        &self,
        project: &str,
        feature: &str,
    ) -> Result<crate::run::questions::Lead, String> {
        let (p, f, wd) = self.feature_ctx(project, feature)?;
        if !own_copy(&wd) {
            return Err("the feature has no worktree of its own".into());
        }
        let conv = f.conversation.as_ref().and_then(|c| self.convs.get(c));
        // The feature's spec session's lead (009 US2, FR-017); `lead` for a session that never
        // named one.
        let agent = conv.map_or("lead", |c| c.state.lead_name()).to_string();
        // Its session is forked only on the runtime and model it began on (research R-12).
        let def = self.library(Some(&p.path)).agent(&agent).cloned();
        let session = conv
            .filter(|c| {
                def.as_ref().is_some_and(|d| {
                    !lead_changed(
                        &c.state,
                        &d.runtime,
                        &Some(d.model.clone()).filter(|m| !m.is_empty()),
                    )
                })
            })
            .and_then(|c| c.state.session.clone());
        Ok(crate::run::questions::Lead {
            session,
            workdir: wd,
            approved: p.approved_commands.clone(),
            agent,
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
        v.push(format!(
            "{}.specify/memory/constitution.md",
            project::feature_root(&f.dir)
        ));
        v
    }

    fn doc_allowed(f: &Feature, path: &str) -> bool {
        !path.contains("..")
            && !path.starts_with('/')
            && !path.contains('\\')
            && (path
                == format!(
                    "{}.specify/memory/constitution.md",
                    project::feature_root(&f.dir)
                )
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
        // Who changed it last (contracts/web-api.md): the engineer, whose edit waits for the
        // lead's next turn, or the lead, whose last turn changed it.
        let conv = f.conversation.as_ref().and_then(|c| self.convs.get(c));
        let last_edit_by = conv.and_then(|c| {
            if c.state.pending_edits.iter().any(|e| e.path == path) {
                return Some("you");
            }
            // The latest turn that touched it: the lead's if the lead reported the change, or if
            // Fallow committed it without an edit of the engineer's; the engineer's otherwise.
            c.state.turns.iter().rev().find_map(|t| {
                let mine = t.edits.iter().any(|e| e.path == path);
                let committed = t.documents_changed.iter().any(|d| d == path);
                let reported = t.result.as_ref().is_some_and(|r| {
                    r["documents_changed"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|d| d == path))
                });
                if reported || (committed && !mine) {
                    Some("lead")
                } else if mine {
                    Some("you")
                } else {
                    None
                }
            })
        });
        Ok(
            json!({ "path": path, "text": text, "version": version_of(&text), "last_edit_by": last_edit_by }),
        )
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
        // Analyze judged the documents before this edit: it runs again before the lock (T086).
        if old != text {
            self.analyze_again(project, feature)?;
        }
        Ok(json!({ "version": v }))
    }

    /// The documents changed after analyze judged them: analyze must run again before the lock,
    /// and the stepper goes back to it, so that the lead's next turn runs it (T086).
    fn analyze_again(&mut self, project: &str, feature: &str) -> Result<(), String> {
        let Some(feat) = self
            .project_mut(project)?
            .feature_mut(feature)
            .filter(|f| f.analyzed)
        else {
            return Ok(());
        };
        feat.analyzed = false;
        feat.analyze_outdated = true;
        let at = |s: &str| STEPS.iter().position(|x| *x == s).unwrap_or(0);
        let moved = at(&feat.step) > at("analyze");
        if moved {
            feat.step = "analyze".into();
        }
        let conv = feat.conversation.clone();
        self.store.save()?;
        if let Some(c) = conv.filter(|_| moved).and_then(|c| self.convs.get_mut(&c)) {
            c.append(CEvent::StepSet {
                step: "analyze".into(),
            })?;
        }
        Ok(())
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
            f.analyze_outdated = false;
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
        let (_, f, wd) = self.feature_ctx(project, feature).map_err(|e| vec![e])?;
        let read = |name: &str| std::fs::read_to_string(wd.join(&f.dir).join(name)).ok();
        let mut problems = Vec::new();
        let analyzed = f.analyzed;
        let outdated = f.analyze_outdated;
        // Say why analyze must run: never ran, or the documents changed after it (008 T042).
        let reword = |mut ps: Vec<String>| {
            if outdated {
                for p in &mut ps {
                    if p.starts_with("the analyze step has not run") {
                        *p = "the documents changed after analyze: ask the lead to run analyze again".into();
                    }
                }
            }
            ps
        };
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
            Err(e) => {
                problems.append(&mut reword(e));
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
        if f.base_branch.is_none() || !own_copy(&wd) {
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
        let rooted: Vec<String> = rights::SPEC_DIRS.iter().map(|d| p.in_root(d)).collect();
        let dirs: Vec<&str> = rooted.iter().map(String::as_str).collect();
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
        let Some((Owner::Conv(conv), runtime)) = self
            .tools
            .get(token)
            .map(|t| (t.owner.clone(), t.runtime.clone()))
        else {
            let _ = reply.send(Reply::Err("unknown or finished step".into()));
            return;
        };
        let deny = |msg: &str| {
            Reply::Ok(agent::registry::permission_reply(
                &runtime, false, msg, &args,
            ))
        };
        if name != "approve" {
            let _ = reply.send(Reply::Err(format!("no tool {name:?}")));
            return;
        }
        // 012 (IX): judged in the contract's terms, never by a runtime's tool names.
        let req = agent::registry::permission_request(&runtime, &args);
        let Some(c) = self.convs.get_mut(&conv) else {
            let _ = reply.send(deny("the conversation is gone"));
            return;
        };
        // 015 (R-3): Fallow answers this call, whatever the answer: a refusal the runtime then
        // reports is Fallow's, not the runtime's own.
        if let Some(id) = args["tool_use_id"].as_str().filter(|i| !i.is_empty()) {
            c.answered.push(id.to_string());
        }
        if c.state.cancelling().is_some() {
            // Stopping: nothing more runs for a withdrawn message (008 T047 review).
            let _ = reply.send(deny("The engineer cancelled this turn."));
            return;
        }
        if c.state.kind == Kind::Solo {
            let reply2 = reply.clone();
            match self.solo_tool_call(&conv, &runtime, &req, &args, reply) {
                Ok(()) => {}
                Err(why) => {
                    let _ = reply2.send(deny(&why));
                }
            }
            return;
        }
        let reply2 = reply.clone();
        if let Err(why) = self.lead_tool_call(&conv, &runtime, &req, &args, reply) {
            let _ = reply2.send(deny(&why));
        }
    }

    /// 015 (research R-1, R-2): a permission prompt from a spec session's lead, decided by
    /// `mode::lead`: its commands under the chat's mode, its edits outside the spec documents
    /// refused visibly.
    fn lead_tool_call(
        &mut self,
        conv: &str,
        runtime: &str,
        req: &agent::PermissionRequest,
        raw: &Value,
        reply: Sender<Reply>,
    ) -> Result<(), String> {
        let c = self.convs.get(conv).ok_or("the conversation is gone")?;
        let p = self.project(&c.state.project)?;
        let target = mode::Target::resolve(p.remote_seen.as_ref(), || git::branch(&p.path));
        let control_files = agent::registry::capabilities(runtime, &self.settings).control_files;
        // Spec Kit runs from the project's spec root (012 R-18), where the lead's paths start.
        let cwd = c.state.workdir.join(&p.spec_root);
        // The approved commands as they are now (one allowed always this turn included), with
        // the turn's gates and Spec Kit scripts.
        let mut approved = p.approved_commands.clone();
        approved.extend(c.lead_runs.iter().cloned());
        let decision = mode::lead(
            c.mode,
            req,
            &mode::LeadCx {
                workdir: &cwd,
                control_files: &control_files,
                approved: &approved,
                target: target.as_ref(),
                folder: &p.path,
            },
        );
        self.answer_request(
            conv,
            runtime,
            mode::Agent::Lead,
            req,
            raw,
            reply,
            decision,
            None,
        )
    }

    /// 015 (FR-007): "Switch to Auto" on Manual's request card, in either chat: the waiting
    /// request is allowed once, as the engineer's own answer, and the chat goes to Auto.
    /// When the request no longer waits (answered, timed out), nothing changes: the mode stays,
    /// and the page says why.
    pub fn approve_and_auto(&mut self, conv: &str, id: &str) -> Result<Value, String> {
        self.approve(conv, id, ApprovalAnswer::Once, None)?;
        self.set_mode(conv, mode::ChatMode::Auto)
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
        let solo = c.state.solo.clone();
        if answer == ApprovalAnswer::Always
            && c.pending
                .as_ref()
                .is_some_and(|p| p.is_read() && p.held.is_none())
        {
            return Err("a read is allowed once at a time".into());
        }
        let command = c
            .pending
            .as_ref()
            .map(|p| p.command.clone())
            .unwrap_or_default();
        let project = c.state.project.clone();
        let held = c.pending.as_ref().and_then(|p| p.held.clone());
        // 015 (2026-11-24 amendment): "Always allow" past a limit: an entry of the project's
        // `allowed_past_limits`, not an approved command.
        let mut past: Option<project::PastLimit> = None;
        if let (ApprovalAnswer::Always, Some(h)) = (answer, held.as_ref()) {
            if !h.always() {
                return Err("this is allowed once at a time".into());
            }
            let pre = prefix
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .ok_or("name what to allow always")?
                .to_string();
            let fits = if h.kind == "edit" {
                command == pre || (pre.ends_with('/') && command.starts_with(&pre))
            } else {
                command
                    .strip_prefix(pre.as_str())
                    .is_some_and(|r| r.is_empty() || r.starts_with(' '))
            };
            if !fits {
                return Err(if h.kind == "edit" {
                    format!("{pre:?} is not this path, nor a folder ending in / that holds it")
                } else {
                    format!("{pre:?} is not the start of {command:?}")
                });
            }
            past = Some(project::PastLimit {
                limit: h.limit.name().to_string(),
                kind: h.kind.to_string(),
                prefix: pre,
            });
        }
        let prefix = match (answer, prefix) {
            (ApprovalAnswer::Always, Some(pre)) if past.is_some() => Some(pre.trim().to_string()),
            (ApprovalAnswer::Always, Some(pre)) => {
                let pre = pre.trim().to_string();
                if let Some(why) = project::commands::command_problem(&pre) {
                    return Err(format!("{pre:?}: {why}"));
                }
                let rest = command.strip_prefix(pre.as_str());
                if !rest
                    .is_some_and(|r| r.is_empty() || r.starts_with([' ', '&', ';', '|', '>', '<']))
                {
                    return Err(format!("{pre:?} is not the start of {command:?}"));
                }
                // 012 (R-6): in a session, never a prefix that could cover a command asked
                // per command.
                if solo.is_some() {
                    let runtime = self
                        .convs
                        .get(conv)
                        .and_then(|c| c.pending.as_ref())
                        .map(|p| p.runtime.clone())
                        .unwrap_or_default();
                    let jx = self.judge_cx(conv, &runtime)?;
                    let jc = judge::Ctx {
                        wt: &jx.wt,
                        branch: &jx.branch,
                        folder: &jx.folder,
                        protect: &jx.protect,
                    };
                    if !judge::prefix_safe(&pre, &jc) {
                        return Err(format!(
                            "{pre:?} could also allow commands Fallow asks about one by one: allow it once instead"
                        ));
                    }
                }
                Some(pre)
            }
            (ApprovalAnswer::Always, None) => {
                return Err("name the command prefix to allow always".into());
            }
            _ => None,
        };
        if let Some(e) = past {
            let p = self.project_mut(&project)?;
            if !p.allowed_past_limits.contains(&e) {
                p.allowed_past_limits.push(e);
            }
            self.store.save()?;
        } else if let Some(pre) = &prefix {
            let p = self.project_mut(&project)?;
            if !p.approved_commands.contains(pre) {
                p.approved_commands.push(pre.clone());
            }
            self.store.save()?;
        }
        let lead_cwd = self.convs.get(conv).and_then(|c| {
            let p = self.project(&c.state.project).ok()?;
            Some((c.state.workdir.clone(), c.state.workdir.join(&p.spec_root)))
        });
        let c = self.conv_mut(conv)?;
        let pending = c.pending.take().expect("checked");
        let msg = match (answer, &pending.held) {
            (ApprovalAnswer::Deny, Some(h)) => pending.answer(false, &h.denied()),
            (ApprovalAnswer::Deny, None) if pending.is_read() => {
                pending.answer(false, "The engineer refused this read.")
            }
            (ApprovalAnswer::Deny, None) => {
                pending.answer(false, "The engineer refused this command.")
            }
            _ => pending.answer(true, ""),
        };
        // 015: a lead's edit allowed past its limit is kept at the turn's end (review of US2, 3).
        if solo.is_none() && answer != ApprovalAnswer::Deny {
            if let (Some(h), Some((wd, cwd))) = (&pending.held, &lead_cwd) {
                if h.kind == "edit" {
                    let abs = cwd.join(&pending.command);
                    if let Some(rel) = judge::rel_inside(wd, &abs.display().to_string()) {
                        c.allowed_paths.push(rel);
                    }
                }
            }
        }
        // 012 (R-10): a session's command allowed now opens its window (never earlier: what
        // the engineer did while the card waited is not the session's).
        let opens = solo.is_some()
            && answer != ApprovalAnswer::Deny
            && !pending.is_read()
            && pending.held.as_ref().is_none_or(|h| h.kind == "run");
        let (raw, command) = (pending.raw.clone(), pending.command.clone());
        // The window opens before the answer goes: a fast push is in it (Phase 7 review, 4).
        if opens {
            self.open_call(conv, &raw, &command);
        }
        let _ = pending.reply.send(Reply::Ok(msg));
        let c = self.conv_mut(conv)?;
        c.append(CEvent::ApprovalAnswered {
            id: id.to_string(),
            answer,
            prefix,
            auto: false,
            why: None,
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
                let _ = p.reply.send(Reply::Ok(
                    p.answer(false, "The engineer did not answer within 10 minutes."),
                ));
                let _ = c.append(CEvent::ApprovalAnswered {
                    id: p.id,
                    answer: ApprovalAnswer::Deny,
                    prefix: None,
                    auto: false,
                    why: None,
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
