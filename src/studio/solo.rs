//! 012 (US1): solo sessions in the studio: making one (research R-2), a turn (R-6, R-7, R-13),
//! its commit (R-3), the working copy settled before a turn (R-4), and the assistant's
//! permission prompts judged (R-6). A session is a conversation of kind `solo`; what only it
//! records is in `conversation::solo`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Kind, Owner, Studio, Tool, spawn_turn};
use crate::agent::rights;
use crate::agent::{self, Call, Session, prompts};
use crate::conversation::events::{Event as CEvent, InstructionRec};
use crate::conversation::solo::SoloEvent;
use crate::platform::job::Group;
use crate::project::Project;
use crate::run::engine::{Ctx, Reply};
use crate::studio::conv::Pending;
use crate::studio::judge;
use crate::verify::git;

/// The assistant new sessions start with when the project names none.
pub const ASSISTANT: &str = "assistant";

/// A branch name's slug from the session's first message (R-2): its first words, lowercased,
/// `[a-z0-9-]`.
pub fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|w| !w.is_empty())
        .take(5)
        .collect();
    let s = words.join("-");
    if s.is_empty() { "session".into() } else { s }
}

/// A session's name from its first message: its first line, cut.
fn name_of(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    agent::clip(line, 60)
}

/// The project's squad file as committed at the project folder's `HEAD` (R-6: the gates the
/// assistant is held to are never its own copy's).
pub fn squad_at_head(project: &Path) -> crate::config::Squad {
    squad_read(project).0
}

/// `squad_at_head`, and whether git really answered: the file read, or not in the commit
/// (issue #3 review: a failed read is never kept as "no squad file").
fn squad_read(project: &Path) -> (crate::config::Squad, bool) {
    let out = crate::platform::command("git")
        .current_dir(project)
        .args([
            "--no-optional-locks",
            "show",
            // `./`: relative to `project`, the spec root of a moved project (012 R-18).
            &format!("HEAD:./{}", crate::config::squad::SQUAD_FILE),
        ])
        .output()
        .ok();
    let (text, known) = match &out {
        Some(o) if o.status.success() => (String::from_utf8_lossy(&o.stdout).into_owned(), true),
        Some(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let absent =
                err.contains("does not exist") || err.contains("exists on disk, but not in");
            (String::new(), absent)
        }
        None => (String::new(), false),
    };
    let mut notices = Vec::new();
    let squad = crate::config::Squad::parse(crate::config::squad::SQUAD_FILE, &text, &mut notices);
    (squad, known)
}

/// 012 (FR-026): a session's turn that Fallow's stop or crash cut off: what it changed is
/// committed as cut short (never withdrawn, never undone).
pub(super) fn cut_on_load(c: &mut super::Conv) {
    let Some(s) = c.state.solo.clone() else {
        return;
    };
    let Some(t) = c.state.turns.last().cloned() else {
        return;
    };
    // First the agent, if it outlived Fallow (R-19): nothing writes after the snapshot.
    for f in pid_files(&c.dir) {
        crate::platform::job::kill_recorded(&f);
    }
    // A command waiting for the engineer is cancelled, said so (FR-026).
    if let Some(a) = c.state.pending_approval().cloned() {
        let _ = c.append(CEvent::ApprovalAnswered {
            id: a.id,
            answer: crate::conversation::events::ApprovalAnswer::Deny,
            prefix: None,
            auto: false,
            why: Some("Fallow restarted".into()),
        });
    }
    let msg = format!(
        "Fallow solo {}, turn {}: {} (cut short)",
        c.id(),
        t.n,
        agent::clip(t.said.lines().next().unwrap_or_default(), 60)
    );
    let before = c
        .state
        .cancelling()
        .and_then(|x| x.head.clone())
        .or_else(|| t.head.clone());
    let snap = git::snapshot(&s.worktree, &s.branch, &msg);
    let commit_failed = snap.err();
    let head = git::head(&s.worktree).ok();
    let commits = match (&before, &head) {
        (Some(b), Some(h)) if b != h => git::commits_between(&s.worktree, b, h).unwrap_or_default(),
        _ => Vec::new(),
    };
    let commit = head.filter(|_| !commits.is_empty());
    let _ = c.append(CEvent::TurnFinished {
        n: t.n,
        reply: "Fallow stopped during this turn: what it changed so far is committed, marked as cut short."
            .into(),
        result: None,
        session: None,
        documents_changed: Vec::new(),
        commit,
        discarded: Vec::new(),
        cost_usd: None,
        failure: Some("Fallow stopped during this turn".into()),
        withdrawn: false,
        commits,
        commit_failed,
        changed: Vec::new(),
        cut: true,
        folded_into: None,
    });
}

/// What the judge needs for a session.
pub(super) struct JudgeCx {
    pub(super) wt: PathBuf,
    pub(super) branch: String,
    pub(super) folder: PathBuf,
    pub(super) protect: Vec<String>,
    pub(super) gates: Vec<String>,
    /// The squad file's `[instructions] extra`, from the same read as `gates` (issue #2).
    pub(super) extra: Vec<String>,
}

impl Studio {
    /// `squad_at_head` of a project, read from git again only when its `HEAD` moved (issue #3).
    pub(super) fn squad_of(&self, p: &Project) -> crate::config::Squad {
        let root = p.root_dir();
        // The repository the squad file is read from: the spec root's own, when it is one
        // (issue #3 review).
        let top = if root.join(".git").exists() {
            root.clone()
        } else {
            p.path.clone()
        };
        let Some(head) = git::head_from_files(&top) else {
            return squad_at_head(&root);
        };
        if let Some((h, s)) = self.squads.borrow().get(&root) {
            if *h == head {
                return s.clone();
            }
        }
        let (squad, known) = squad_read(&root);
        // Kept only when git answered and `HEAD` did not move while it was read.
        if known && git::head_from_files(&top).as_deref() == Some(head.as_str()) {
            self.squads.borrow_mut().insert(root, (head, squad.clone()));
        }
        squad
    }

    /// The assistant a project's new sessions start with.
    fn default_assistant(&self, p: &Project) -> String {
        p.assistant.clone().unwrap_or_else(|| ASSISTANT.to_string())
    }

    /// Why `name` cannot be a session's assistant, if it cannot (FR-005).
    pub(super) fn assistant_problem(&self, path: &Path, name: &str) -> Option<String> {
        let lib = self.library(Some(path));
        let Some(def) = lib.agent(name) else {
            return Some(format!("there is no agent {name}"));
        };
        if def.kind != crate::defs::agent::Kind::Assistant {
            return Some(format!("{name} is not an assistant"));
        }
        agent::registry::runtime(&def.runtime, &self.settings)
            .err()
            .map(|e| format!("the assistant cannot start: {e}"))
    }

    /// 012 (FR-001, FR-012, R-2): a session from its first message: its working copy and its
    /// branch from the engineer's `HEAD`, named after the message, then the message sent.
    pub fn new_session(
        &mut self,
        ctx: &Ctx,
        project: &str,
        agent_name: Option<String>,
        text: &str,
        refs: Vec<prompts::FileRef>,
        mode: Option<super::mode::ChatMode>,
    ) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("say something to the assistant".into());
        }
        // The engine refused a project that is not a git repository before this (its
        // `project_refusal`, which keeps the project's kind a few seconds: issue #2).
        let p = self.project(project)?.clone();
        if !p.path.is_dir() {
            return Err("the project's folder is gone".into());
        }
        let assistant = agent_name.unwrap_or_else(|| self.default_assistant(&p));
        if let Some(why) = self.assistant_problem(&p.path, &assistant) {
            return Err(why);
        }
        let (start_commit, start_branch) = git::head_and_branch(&p.path)
            .map_err(|_| "the repository has no commit yet".to_string())?;
        let placeholder = self.worktrees(&p.id).join("_solo");
        let id = self.new_conv_with(
            Kind::Solo,
            &p.id,
            None,
            None,
            placeholder.clone(),
            None,
            Some(assistant),
            None,
        )?;
        // 015 (FR-002): the mode chosen in the composer before the first message, with its note.
        if let Some(m) = mode {
            self.set_mode(&id, m)?;
        }
        let wt = placeholder.join(&id);
        let first = format!("fallow/solo/{id}");
        {
            let c = self.conv_mut(&id)?;
            c.append(CEvent::Solo(SoloEvent::SessionCreating {
                branch: first.clone(),
                start_commit: start_commit.clone(),
            }))?;
        }
        // What only reads the project runs while its files are written into the working copy
        // (issue #2: that checkout is most of a session's start on a big project).
        let repo = p.path.clone();
        let root = p.root_dir();
        let base = format!("fallow/solo/{}", slug(text));
        let (made, side) = std::thread::scope(|sc| {
            let side = sc.spawn(|| {
                let dirty = !git::folder_status(&repo).unwrap_or_default().is_empty();
                let squad = squad_at_head(&root);
                // The branch after the first message, never overwriting one (FR-015), nor the
                // one being made beside this.
                let mut branch = base.clone();
                let mut k = 2;
                while branch == first || git::rev_exists(&repo, &format!("refs/heads/{branch}")) {
                    branch = format!("{base}-{k}");
                    k += 1;
                }
                (dirty, squad, branch)
            });
            let made = std::fs::create_dir_all(&placeholder)
                .map_err(|e| e.to_string())
                .and_then(|()| git::worktree_add_new(&repo, &wt, &first, &start_commit));
            (made, side.join())
        });
        let side = side.map_err(|_| "the session's start failed".to_string());
        let (dirty, squad, branch) = match (made, side) {
            (Ok(()), Ok(s)) => s,
            (Err(e), _) | (_, Err(e)) => {
                self.forget_half_made(&id, &p.path, &wt, &first);
                return Err(format!("the session's working copy could not be made: {e}"));
            }
        };
        let lfs = std::fs::read_to_string(wt.join(".gitattributes"))
            .is_ok_and(|a| a.contains("filter=lfs"));
        if let Err(e) = git::rename_branch_nohooks(&wt, &first, &branch) {
            self.forget_half_made(&id, &p.path, &wt, &first);
            return Err(format!("the session's branch could not be named: {e}"));
        }
        let c = self.conv_mut(&id)?;
        c.append(CEvent::WorkdirSet {
            workdir: wt.clone(),
        })?;
        c.append(CEvent::Solo(SoloEvent::SessionCreated {
            branch: first.clone(),
            start_branch,
            start_commit,
            worktree: wt,
            dirty_at_start: dirty,
            web: true,
            lfs,
        }))?;
        c.append(CEvent::Solo(SoloEvent::BranchRenamed {
            from: first,
            to: branch,
        }))?;
        c.append(CEvent::Solo(SoloEvent::SessionRenamed {
            name: name_of(text),
            by: Some("first_message".into()),
        }))?;
        let declared = !squad.gates.is_empty();
        let pm = self.project_mut(&p.id)?;
        pm.sessions.push(id.clone());
        pm.last_session = Some(id.clone());
        self.store.save()?;
        // For the first turn only, which takes it (issue #2 review: never left on a session).
        self.conv_mut(&id)?.fresh = Some(super::conv::Fresh { squad });
        // The session exists now: a first turn that cannot start is said with it, never hidden
        // behind an error (review 8: the draft would make a second session).
        let mut v = match self.say_solo(ctx, &id, text, refs) {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        };
        if let Some(c) = self.convs.get_mut(&id) {
            c.fresh = None;
            // Its first view with less git (issue #2): nothing changed in its copy yet.
            c.primed = Some(super::conv::Primed {
                seq: c.state.seq,
                at: std::time::Instant::now(),
                declared,
            });
        }
        v["session"] = json!(id);
        Ok(v)
    }

    /// Nothing is left of a session that could not be made (second review, low 3; third review,
    /// low 2): its record, its working copy, and the branch made for it.
    fn forget_half_made(&mut self, id: &str, repo: &Path, wt: &Path, branch: &str) {
        let gone = self.convs.remove(id);
        let dir = gone.as_ref().map(|c| c.dir.clone());
        // The branch it is on now: the first message may have renamed it (fourth review, 3).
        // Read only from its own working copy, never a repository around the folder.
        let mut branches = vec![branch.to_string()];
        if wt.join(".git").is_file() {
            if let Some(now) = git::branch(wt).filter(|b| b.starts_with("fallow/solo/")) {
                branches.push(now);
            }
        }
        // Nor listed in its project.
        if let Some(p) = gone.map(|c| c.state.project.clone()) {
            if let Ok(pm) = self.project_mut(&p) {
                let listed = pm.sessions.len();
                pm.sessions.retain(|s| s != id);
                let was_last = pm.last_session.as_deref() == Some(id);
                if was_last {
                    pm.last_session = None;
                }
                if listed != pm.sessions.len() || was_last {
                    let _ = self.store.save();
                }
            }
        }
        if wt.exists() {
            let _ = git::worktree_remove(repo, wt);
        }
        git::worktree_prune(repo);
        for b in branches {
            if git::rev_exists(repo, &format!("refs/heads/{b}")) {
                let _ = git::delete_branch(repo, &b);
            }
        }

        if let Some(d) = dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// What the judge needs for `conv`'s session.
    pub(super) fn judge_cx(&self, conv: &str, runtime: &str) -> Result<JudgeCx, String> {
        self.judge_cx_with(conv, runtime, None)
    }

    /// `judge_cx`, with the squad file when it was read already (issue #2).
    fn judge_cx_with(
        &self,
        conv: &str,
        runtime: &str,
        squad: Option<crate::config::Squad>,
    ) -> Result<JudgeCx, String> {
        let c = self.convs.get(conv).ok_or("the session is gone")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        let p = self.project(&c.state.project)?;
        let caps = agent::registry::capabilities(runtime, &self.settings);
        let mut protect: Vec<String> = rights::CONTROL_GLOBS
            .iter()
            .map(|s| s.to_string())
            .collect();
        // 012 (R-18): a moved project's controls are under its spec root too.
        if !p.spec_root.is_empty() {
            protect.extend(rights::CONTROL_GLOBS.iter().map(|g| p.in_root(g)));
        }
        protect.extend(caps.control_files);
        // The copy's link to its repository: rewritten, it would point Fallow's own git at a
        // config the assistant wrote (fourth review, security).
        protect.extend([".git".to_string(), ".git/**".to_string()]);
        let squad = squad.unwrap_or_else(|| self.squad_of(p));
        let gates = squad.gates.iter().map(|g| g.command.clone()).collect();
        Ok(JudgeCx {
            wt: solo.worktree.clone(),
            branch: solo.branch.clone(),
            folder: p.path.clone(),
            protect,
            gates,
            extra: squad.instructions_extra,
        })
    }

    /// 012 (FR-006, FR-011, FR-013, R-4, R-9, R-13): a message in a session: waiting while a
    /// turn works or no agent slot is free; otherwise a turn of the assistant, after the
    /// engineer's own edits in the working copy are committed as theirs.
    pub fn say_solo(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
    ) -> Result<Value, String> {
        if text.trim() == "/close" {
            return self
                .close_session(conv, None, false, None)
                .map(|v| json!({ "close": v }));
        }
        self.solo_turn(ctx, conv, text, refs, None, Vec::new())
    }

    /// `say_solo`, or with `queued` (its id) a waiting message being sent: it is not queued
    /// again when it still has to wait (`{"waiting": …}`), and it leaves the queue as its turn
    /// starts (third review, low 1: never sent twice).
    pub(super) fn solo_turn(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
        queued: Option<&str>,
        attachments: Vec<crate::conversation::solo::AttachmentRec>,
    ) -> Result<Value, String> {
        let from_queue = queued.is_some();
        let text = text.trim();
        if text.is_empty() && attachments.is_empty() {
            return Err("say something to the assistant".into());
        }
        // A session made in this request: taken by its first turn, whatever happens to it.
        let fresh_squad = self
            .convs
            .get_mut(conv)
            .and_then(|c| c.fresh.take())
            .map(|f| f.squad);
        let fresh = fresh_squad.is_some();
        if !fresh {
            self.check_gone(conv);
        }
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        if solo.closed.is_some() {
            return Err("the session is closed".into());
        }
        if let Some(why) = &c.gone {
            return Err(format!("{why}: the session can only be closed"));
        }
        if solo.pending_commit.is_some() && !solo.fold_next && !c.state.busy {
            // Tried again first (R-4); it says why when it still cannot.
            self.settle(conv)?;
        }
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        // A turn at work: the message waits (FR-011).
        if c.state.busy && from_queue {
            return Ok(json!({ "waiting": "a turn is at work" }));
        }
        // Older messages still wait: this one goes after them, in the order typed (review 3).
        if c.state.busy || (!from_queue && !solo.queue.is_empty()) {
            let idle = !c.state.busy;
            let id = format!("q{}", c.state.seq + 1);
            let c = self.conv_mut(conv)?;
            c.append(CEvent::Solo(SoloEvent::MessageQueued {
                id: id.clone(),
                text: text.to_string(),
                attachments: attachments.clone(),
            }))?;
            if !idle {
                return Ok(json!({ "queued": id }));
            }
            // Idle: the engineer's message is their go-ahead for those waiting (second review):
            // the oldest goes now (the engine sends it, reading its `@` files then, T045), the
            // rest after it.
            c.queue_hold = false;
            c.queue_go = true;
            return Ok(json!({ "queued": id }));
        }
        // An agent slot, as a loop step takes one (FR-027).
        if let Err(why) = ctx.slots.take() {
            if from_queue {
                let c = self.conv_mut(conv)?;
                c.slot_wait = Some(why.clone());
                return Ok(json!({ "waiting": why }));
            }
            let id = format!("q{}", c.state.seq + 1);
            let c = self.conv_mut(conv)?;
            c.append(CEvent::Solo(SoloEvent::MessageQueued {
                id: id.clone(),
                text: text.to_string(),
                attachments: attachments.clone(),
            }))?;
            c.slot_wait = Some(why.clone());
            return Ok(json!({ "queued": id, "waiting": why }));
        }
        let p = self.project(&c.state.project)?.clone();
        let wt = solo.worktree.clone();
        let n = c.state.turns.iter().map(|t| t.n).max().unwrap_or(0) + 1;
        let assistant = c
            .state
            .agent
            .clone()
            .unwrap_or_else(|| ASSISTANT.to_string());
        if let Some(why) = self.assistant_problem(&p.path, &assistant) {
            return Err(why);
        }
        // The engineer's edits since the last turn are committed as theirs first (R-4).
        let _ = n;
        // A session made in this request has no edits of the engineer's yet.
        let engineer_edits = if fresh {
            Vec::new()
        } else {
            self.settle(conv)?
        };
        let lib = self.library(Some(&p.path));
        let def = lib
            .agent(&assistant)
            .cloned()
            .ok_or_else(|| format!("there is no agent {assistant}"))?;
        let runtime = agent::registry::runtime(&def.runtime, &ctx.settings)
            .map_err(|e| format!("the assistant cannot start: {e}"))?;
        let jc = self.judge_cx_with(conv, &def.runtime, fresh_squad)?;
        // Without asking: the gates, and the approved prefixes the judge finds safe (R-6).
        let ctxj = judge::Ctx {
            wt: &jc.wt,
            branch: &jc.branch,
            folder: &jc.folder,
            protect: &jc.protect,
        };
        let mut commands = jc.gates.clone();
        commands.extend(
            p.approved_commands
                .iter()
                .filter(|a| judge::prefix_safe(a, &ctxj))
                .cloned(),
        );
        // The project's instruction files: with a new agent session, and when they changed.
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let instr = agent::instructions::read(&wt, &jc.extra);
        let now: Vec<(String, String)> = instr
            .iter()
            .map(|i| (i.path.clone(), i.sha.clone()))
            .collect();
        let session = match &c.state.session {
            Some(s) => Session::Resume(s.clone()),
            None => Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?),
        };
        let hand = !matches!(session, Session::Resume(_)) || now != c.state.last_instructions();
        let instr_text = if hand {
            agent::instructions::block(&instr)
        } else {
            String::new()
        };
        let instr_recs: Vec<InstructionRec> = if hand {
            instr
                .iter()
                .map(|i| InstructionRec {
                    path: i.path.clone(),
                    sha: i.sha.clone(),
                    cut: i.cut,
                    skipped: i.skipped.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let failed = solo
            .pending_commit
            .and_then(|k| solo.commits.get(&k))
            .and_then(|t| t.failed.clone());
        let instruction = prompts::assistant_turn(&prompts::AssistantTurn {
            said: text,
            instructions: &instr_text,
            refs: &refs,
            undone: &Self::undone_to_tell(c),
            failed_commit: failed.as_deref(),
            engineer_edits: &engineer_edits,
            feature_request: text.starts_with("/feature"),
        });
        // The MCP bridge for the permission prompt, checked against the turn's own processes.
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
        let c = self.conv_mut(conv)?;
        let mut args = vec![
            "mcp-bridge".to_string(),
            "--fallow-pid".into(),
            ctx.pid.to_string(),
            "--step".into(),
            token.clone(),
        ];
        let mcp = c.dir.join("mcp.json");
        if let Some(e) = &self.endpoint {
            args.push("--endpoint".to_string());
            args.push(e.clone());
        }
        std::fs::write(
            &mcp,
            json!({ "mcpServers": { "fallow": { "command": ctx.exe, "args": args } } }).to_string(),
        )
        .map_err(|e| format!("{}: {e}", mcp.display()))?;
        let c = self.conv_mut(conv)?;
        let call = Call {
            instruction,
            schema: Some(agent::schemas::assistant_reply()),
            rights: rights::assistant(commands, jc.protect.clone()),
            mcp_config: Some(mcp),
            session: session.clone(),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            budget_usd: Some(def.limits.usd.unwrap_or(5.0)),
            cwd: wt.clone(),
            time_limit: Duration::from_secs(def.limits.minutes.unwrap_or(30) * 60),
            system: Some(prompts::assistant_system(&def.body)),
            partial: true,
            group: Some(Arc::clone(&group)),
            attachments: attachments
                .iter()
                .map(|a| agent::Attachment {
                    path: super::extras::attachment_path(&c.dir, a),
                    media_type: a.media_type.clone(),
                })
                .collect(),
            web: solo.web,
            user_settings: true,
        };
        let group_for_record = group;
        // A fresh session's working copy is its start commit (issue #2: one git process less).
        let head = if fresh {
            Some(solo.start_commit.clone())
        } else {
            git::head(&wt).ok()
        };
        let c = self.conv_mut(conv)?;
        c.slot_wait = None;
        if let Some(qid) = queued {
            c.append(CEvent::Solo(SoloEvent::QueueSent {
                id: qid.to_string(),
                n,
            }))?;
        }
        c.append(CEvent::TurnStarted {
            n,
            said: text.to_string(),
            edits: Vec::new(),
            mode: c.mode.name().to_string(),
            session: session.id().to_string(),
            runtime: Some(def.runtime.clone()),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            new_session: !matches!(session, Session::Resume(_)),
            instructions: instr_recs,
            attachments,
            slot_waited_ms: None,
            head: head.clone(),
        })?;
        c.live.clear();
        c.answered.clear();
        if let Ok(mut h) = c.hidden.lock() {
            h.clear();
        }
        c.head_before = head;
        // Read now, never assumed: the engineer may commit on it while the copy is made (issue
        // #2 review).
        c.start_before = solo
            .start_branch
            .as_ref()
            .and_then(|b| git::rev(&wt, &format!("refs/heads/{b}")).ok());
        // Kept beside the log too: a restart during the turn still knows it (R-5).
        match &c.start_before {
            Some(sb) => {
                let _ = std::fs::write(c.dir.join("start_before"), sb);
            }
            None => {
                let _ = std::fs::remove_file(c.dir.join("start_before"));
            }
        }
        c.activity.clear();
        c.token = Some(token);
        c.stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&c.stop);
        let turn_file = c.turns_dir().join(format!("{n}.jsonl"));
        let _ = std::fs::create_dir_all(c.turns_dir());
        let pid_file = c.dir.join(format!("turn-{n}.pid"));
        let recorded = Arc::clone(&group_for_record);
        c.turn_ended = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&c.turn_ended);
        let spawned = spawn_turn(
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
        );
        if spawned.is_ok() {
            // The agent's group, recorded once it is adopted, so a restart can stop it (R-19).
            let _ = std::thread::Builder::new()
                .name(format!("turn-pid-{conv}"))
                .spawn(move || {
                    for _ in 0..200 {
                        if ended.load(Ordering::SeqCst) {
                            return;
                        }
                        if recorded.record(&pid_file) {
                            // The turn ended while this was written: nothing is left to stop.
                            if ended.load(Ordering::SeqCst) {
                                let _ = std::fs::remove_file(&pid_file);
                            }
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                });
        }
        if let Err(e) = spawned {
            // The turn never ran: it ends now, so the session is not left busy (review 7).
            let outcome = agent::Outcome {
                result: None,
                failure: Some(e.clone()),
                cost_usd: None,
                session: None,
                denials: Vec::new(),
                duration: Duration::ZERO,
            };
            self.solo_turn_done(conv, n, outcome)?;
            return Err(e);
        }
        Ok(json!({ "turn": n }))
    }

    /// 012 (FR-013, R-3): a session's turn ended: Fallow's snapshot on the session's branch,
    /// its commits counted, a failure kept with what the turn left.
    pub(super) fn solo_turn_done(
        &mut self,
        conv: &str,
        n: u32,
        outcome: agent::Outcome,
    ) -> Result<(), String> {
        let c = self.conv_mut(conv)?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        let cut =
            c.state.cancelling().is_some() || c.stop.load(std::sync::atomic::Ordering::Relaxed);
        let said = c
            .state
            .turns
            .iter()
            .find(|t| t.n == n)
            .map(|t| t.said.clone())
            .unwrap_or_default();
        let before = c.head_before.take();
        let wt = solo.worktree.clone();
        let folding = solo.pending_commit.filter(|_| solo.fold_next);
        let msg = format!(
            "Fallow solo {conv}, turn{}: {}{}",
            match folding {
                Some(k) => format!("s {k}-{n}"),
                None => format!(" {n}"),
            },
            agent::clip(said.lines().next().unwrap_or_default(), 60),
            if cut { " (cut short)" } else { "" }
        );
        let snap = git::snapshot(&wt, &solo.branch, &msg);
        let head = git::head(&wt).ok();
        let commits = match (&before, &head) {
            (Some(b), Some(h)) if b != h => git::commits_between(&wt, b, h).unwrap_or_default(),
            _ => Vec::new(),
        };
        let (commit, commit_failed, changed) = match &snap {
            Ok(_) => (
                head.clone().filter(|_| !commits.is_empty()),
                None,
                Vec::new(),
            ),
            Err(e) => {
                let changed = git::changes_all(&wt)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| {
                        let path = f.trim_matches('"').to_string();
                        crate::conversation::solo::ChangedRec {
                            blob: git::blob_of(&wt, &path),
                            path,
                        }
                    })
                    .collect();
                (
                    head.clone().filter(|_| !commits.is_empty()),
                    Some(e.clone()),
                    changed,
                )
            }
        };
        let result = outcome.result.clone();
        let reply = match (&result, &outcome.failure) {
            (Some(r), _) => r["reply"].as_str().unwrap_or_default().to_string(),
            (None, _) if cut => {
                "Stopped. What the assistant changed so far is committed, marked as cut short."
                    .to_string()
            }
            (None, Some(f)) => format!("The assistant's turn failed: {f}"),
            (None, None) => String::new(),
        };
        let c = self.conv_mut(conv)?;
        c.turn_ended.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(c.dir.join(format!("turn-{n}.pid")));
        if let Some(t) = c.token.take() {
            self.tools.remove(&t);
        }
        let c = self.conv_mut(conv)?;
        if let Some(p) = c.pending.take() {
            let _ = p.reply.send(Reply::Ok(p.answer(false, "The turn ended.")));
        }
        if outcome.failure.is_some() || cut {
            c.queue_go = false;
        }
        c.append(CEvent::TurnFinished {
            n,
            reply,
            result: result.clone(),
            session: outcome.session.clone(),
            documents_changed: Vec::new(),
            commit,
            discarded: Vec::new(),
            cost_usd: outcome.cost_usd,
            failure: outcome.failure.clone(),
            withdrawn: false,
            commits,
            commit_failed,
            changed,
            cut,
            folded_into: folding,
        })?;
        if let Some((title, brief)) = result
            .as_ref()
            .and_then(crate::conversation::solo::suggestion_of)
        {
            c.append(CEvent::Solo(SoloEvent::FeatureSuggested {
                n,
                title,
                brief,
            }))?;
        }
        c.editor_during_turn = false;
        c.tick();
        // Whether the turn brought the branch up to date with the starting branch (R-5).
        if let (Some(b), Some(h)) = (before.as_deref(), head.as_deref()) {
            let after = git::head(&wt).unwrap_or_else(|_| h.to_string());
            self.base_after_turn(conv, n, b, &after);
        }
        Ok(())
    }

    /// 012 (FR-008, FR-009), 015 (research R-1, R-2): a permission prompt from a session's
    /// assistant, decided by `mode::assistant` in the contract's terms. `Ok` when it was
    /// answered, refused (visibly) or is waiting for the engineer; `Err` with why it is denied.
    pub(super) fn solo_tool_call(
        &mut self,
        conv: &str,
        runtime: &str,
        req: &agent::PermissionRequest,
        raw: &Value,
        reply: Sender<Reply>,
    ) -> Result<(), String> {
        let jc = self.judge_cx(conv, runtime)?;
        let c = self.convs.get(conv).ok_or("the session is gone")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        // Without web access the assistant has no web tools at all (012 R-8, 015 FR-003).
        if req.kind == agent::RequestKind::Web && !solo.web {
            return Err("Web access is off for this session.".into());
        }
        let p = self.project(&c.state.project)?;
        let ctxj = judge::Ctx {
            wt: &jc.wt,
            branch: &jc.branch,
            folder: &jc.folder,
            protect: &jc.protect,
        };
        let target = super::mode::Target::resolve(p.remote_seen.as_ref(), || git::branch(&p.path));
        let control_files = agent::registry::capabilities(runtime, &self.settings).control_files;
        let root = p.in_root("");
        // What a request that changes the engineer's folder shows (FR-016, T037).
        let facts_of = |command: &str| {
            Some(super::refwatch::folder_facts(
                &jc.folder, command, &jc.branch, &root, &jc.wt,
            ))
        };
        // 015 (FR-010): a read or search of a file that may hold secrets: its output is hidden.
        if req.kind == agent::RequestKind::Read {
            let path = req.path.as_deref();
            let why = match req.tool.as_str() {
                "Grep" => judge::search_problem(path, req.query.as_deref(), &ctxj),
                "Glob" => None,
                _ => judge::read_problem(path, &ctxj),
            };
            let id = raw["tool_use_id"].as_str().unwrap_or_default();
            if why.is_some_and(|w| w.contains("secrets")) && !id.is_empty() {
                if let Ok(mut h) = c.hidden.lock() {
                    h.push(id.to_string());
                }
            }
        }
        let decision = super::mode::assistant(
            c.mode,
            req,
            &super::mode::AssistantCx {
                judge: &ctxj,
                control_files: &control_files,
                gates: &jc.gates,
                approved: &p.approved_commands,
                target: target.as_ref(),
            },
            facts_of,
        );
        // A command asked in Manual that changes the engineer's folder shows 012's facts.
        let ask_facts = match (&decision, req.kind) {
            (super::mode::Decision::Ask(_), agent::RequestKind::Run) => {
                let command = req.command.clone().unwrap_or_default();
                super::refwatch::in_folder(&command, &jc.folder, &jc.wt)
                    .then(|| facts_of(&command))
                    .flatten()
            }
            _ => None,
        };
        self.answer_request(
            conv,
            runtime,
            super::mode::Agent::Assistant,
            req,
            raw,
            reply,
            decision,
            ask_facts,
        )
    }

    /// 012 (FR-043): how a read put to the engineer is named on its card.
    pub(super) fn read_words(req: &agent::PermissionRequest) -> String {
        let path = req.path.as_deref();
        match (req.tool.as_str(), path, req.query.as_deref()) {
            ("Grep", p, g) => format!(
                "search in {}{}",
                p.unwrap_or("the working copy"),
                g.map(|g| format!(" (files {g})")).unwrap_or_default()
            ),
            ("Glob", p, g) => format!(
                "list {} in {}",
                g.unwrap_or("files"),
                p.unwrap_or("the working copy")
            ),
            (_, p, _) => format!("read {}", p.unwrap_or_default()),
        }
    }

    /// 012 (FR-008, FR-043): a command or a read put to the engineer: recorded as asked, and
    /// answered when they answer (or the wait runs out).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ask_engineer(
        &mut self,
        conv: &str,
        runtime: &str,
        tool: &str,
        read: bool,
        command: String,
        reasons: Vec<String>,
        raw: &Value,
        reply: Sender<Reply>,
        facts: Option<Value>,
    ) -> Result<(), String> {
        let c = self.convs.get(conv).ok_or("the session is gone")?;
        let id = format!(
            "a{}",
            c.state
                .turns
                .iter()
                .map(|t| t.approvals.len())
                .sum::<usize>()
                + 1
        );
        let wait = self.approval_wait;
        let c = self.conv_mut(conv)?;
        c.append(CEvent::ApprovalAsked {
            id: id.clone(),
            command: command.clone(),
            reasons,
            merge: facts.clone(),
        })
        .map_err(|_| "Fallow could not record the question.".to_string())?;
        c.pending = Some(Pending {
            id,
            tool: tool.to_string(),
            read,
            facts,
            command,
            raw: raw.clone(),
            runtime: runtime.to_string(),
            reply,
            deadline: std::time::Instant::now() + wait,
        });
        Ok(())
    }

    /// 012 (FR-002, FR-025, contracts/web-api.md): the project's sessions for the dropdown:
    /// open ones (the one open last first), closed ones, earlier conversations with the lead,
    /// and how much the sessions' records hold.
    pub fn list_sessions(&mut self, project: &str) -> Result<Value, String> {
        let ids = self.project(project)?.sessions.clone();
        for id in &ids {
            self.check_gone(id);
        }
        let p = self.project(project)?;
        let mut open = Vec::new();
        let mut closed = Vec::new();
        let mut bytes = 0u64;
        for id in p.sessions.iter().rev() {
            let Some(c) = self.convs.get(id) else {
                continue;
            };
            let Some(s) = c.state.solo.as_ref() else {
                continue;
            };
            bytes += dir_size(&c.dir);
            let changes = if s.closed.is_some() || c.gone.is_some() {
                0
            } else {
                // HEAD unknown here: its commits are read (issue #3).
                changed_count(s, "")
            };
            let item = json!({
                "id": id,
                "name": s.name,
                "busy": c.state.busy,
                "changes": changes,
                "gone": c.gone,
                "branch": s.branch,
                "closed": s.closed.as_ref().map(|(how, b, w)| json!({ "how": how, "branch": b, "worktree": w, "copy_removed": s.copy_removed })),
            });
            if s.closed.is_some() {
                closed.push(item);
            } else if p.last_session.as_deref() == Some(id.as_str()) {
                open.insert(0, item);
            } else {
                open.push(item);
            }
        }
        let earlier: Vec<Value> = p
            .conversation
            .iter()
            .filter_map(|id| self.convs.get(id))
            .filter(|c| !c.state.turns.is_empty())
            .map(|c| {
                json!({
                    "conversation": c.id(),
                    "title": "Project conversation with the lead",
                    "at": c.state.turns.last().map(|t| t.started.clone()),
                })
            })
            .collect();
        Ok(json!({ "open": open, "closed": closed, "earlier": earlier, "records_bytes": bytes }))
    }

    /// 012 (FR-004): renames a session; its branch keeps its name.
    pub fn rename_session(&mut self, conv: &str, name: &str) -> Result<Value, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("give the session a name".into());
        }
        let c = self.conv_mut(conv)?;
        if c.state.solo.is_none() {
            return Err("not a session".into());
        }
        c.append(CEvent::Solo(SoloEvent::SessionRenamed {
            name: agent::clip(name, 80),
            by: None,
        }))?;
        Ok(json!({}))
    }

    /// 012 (FR-001): the session the engineer opened, shown first next time.
    pub fn open_session(&mut self, conv: &str) -> Result<Value, String> {
        self.check_gone(conv);
        let c = self.convs.get(conv).ok_or("there is no session")?;
        if c.state.solo.as_ref().is_none_or(|s| s.closed.is_some()) {
            return Err("that session is closed".into());
        }
        let project = c.state.project.clone();
        let pm = self.project_mut(&project)?;
        pm.last_session = Some(conv.to_string());
        self.store.save()?;
        Ok(json!({}))
    }

    /// 012 (R-2, R-19): after a restart: a session made only halfway is forgotten (the draft's
    /// message was never sent), one whose working copy or branch is gone is marked so, and one
    /// that was in "until I switch back" says it is back to asking.
    pub(super) fn solo_on_load(&mut self) {
        self.briefs_on_load();
        let ids: Vec<String> = self
            .convs
            .iter()
            .filter(|(_, c)| c.state.kind == Kind::Solo)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            // An undo stopped in the middle is finished from git first (R-5); gate copies a
            // restart left go.
            self.finish_undo_on_load(&id);
            self.forget_gate_copies(&id);
            self.finish_handover_on_load(&id);
            self.finish_close_on_load(&id);
            let Some(c) = self.convs.get_mut(&id) else {
                continue;
            };
            let Some(s) = c.state.solo.clone() else {
                continue;
            };
            let repo = self.store.get(&c.state.project).map(|p| p.path.clone());
            if let Some((branch, _)) = s.creating.clone() {
                if let Some(repo) = repo {
                    let wt = c.state.workdir.join(&id);
                    self.forget_half_made(&id, &repo, &wt, &branch);
                }
                continue;
            }
            if s.closed.is_some() {
                continue;
            }
            let project = c.state.project.clone();
            self.check_gone(&id);
            // Made, but Fallow stopped before the project listed it (fourth review, 4).
            if let Ok(pm) = self.project_mut(&project) {
                if !pm.sessions.contains(&id) {
                    pm.sessions.push(id.clone());
                    let _ = self.store.save();
                }
            }
        }
    }

    /// 012 (R-19): marks a session whose working copy or branch is gone (outside Fallow, at
    /// any time: fourth review, 7); it can only be closed then.
    /// `check_gone`, for the engine's ref watcher.
    pub(crate) fn check_gone_pub(&mut self, id: &str) {
        self.check_gone(id);
    }

    /// 012 (R-10): the opening snapshot of a session's command Fallow just allowed.
    /// Only a command that can move a ref opens a window (a long `npm test` holds nothing up:
    /// Phase 7 review, 5); a second one allowed before the first's result joins it.
    pub(super) fn open_call(&mut self, conv: &str, raw: &Value, command: &str) {
        let Some(c) = self.convs.get(conv) else {
            return;
        };
        let Some(solo) = c.state.solo.as_ref() else {
            return;
        };
        let Some(folder) = self.store.get(&c.state.project).map(|p| p.path.clone()) else {
            return;
        };
        let id = raw["tool_use_id"].as_str().unwrap_or_default().to_string();
        let turn = c.state.turns.last().map_or(0, |t| t.n);
        let wt = solo.worktree.clone();
        let branch = solo.branch.clone();
        let joined = c.open_call.clone();
        let opening = match &joined {
            Some(o) => o.opening.clone(),
            None => super::refwatch::take(&folder),
        };
        let head = opening.as_ref().and_then(|o| o.head.clone());
        let targets = super::refwatch::targets(command, &folder, head.as_deref(), &branch, &wt);
        let touches = super::refwatch::in_folder(command, &folder, &wt);
        if targets.is_empty() && !touches {
            return;
        }
        let mut call = joined.unwrap_or(super::refwatch::OpenCall {
            ids: Vec::new(),
            commands: Vec::new(),
            opening,
            turn,
            targets: Vec::new(),
            folder: false,
            opened_at: std::time::Instant::now(),
        });
        call.ids.push(id);
        call.commands.push(command.to_string());
        call.targets.extend(targets);
        call.folder |= touches;
        if let Some(c) = self.convs.get_mut(conv) {
            c.open_call = Some(call);
        }
    }

    pub(super) fn check_gone(&mut self, id: &str) {
        let Some(c) = self.convs.get(id) else {
            return;
        };
        let Some(s) = c.state.solo.as_ref() else {
            return;
        };
        if s.closed.is_some() || s.creating.is_some() || c.gone.is_some() {
            return;
        }
        let repo = self.store.get(&c.state.project).map(|p| p.path.clone());
        let why = match repo {
            None => Some("the project is gone".to_string()),
            Some(_) if !s.worktree.join(".git").is_file() => {
                Some("the session's working copy is gone".to_string())
            }
            Some(repo) if !git::rev_exists(&repo, &format!("refs/heads/{}", s.branch)) => {
                Some(format!("the session's branch {} is gone", s.branch))
            }
            Some(_) => None,
        };
        if let (Some(why), Some(c)) = (why, self.convs.get_mut(id)) {
            c.gone = Some(why);
            c.ticks += 1;
        }
    }

    /// 012, 015 (FR-001): a session's mode, `manual` or `auto`, as `set_mode` sets it.
    pub fn set_solo_mode(&mut self, conv: &str, mode: &str) -> Result<Value, String> {
        if self.conv_mut(conv)?.state.solo.is_none() {
            return Err("not a session".into());
        }
        let mode = super::mode::ChatMode::parse(mode)?;
        self.set_mode(conv, mode)
    }

    /// 012 (R-9, R-13): a waiting message is sent when the turn before it ended and a slot is
    /// free; after a restart it waits for the engineer.
    pub fn solo_tick(&mut self, ctx: &Ctx) {
        // 012 (R-15): spec sessions whose constitution step is done run specify on the brief.
        self.brief_tick(ctx);
        // 015 (R-5): Fallow's notes to a lead whose turn ended.
        self.lead_notes_tick(ctx);
    }

    /// 012 (R-9): the sessions whose oldest waiting message may go now: (session, id, text).
    /// Not after a failure or a refused approval until the engineer says something, nor after a
    /// restart (held), nor while a commit is pending.
    pub fn waiting_ready(&self) -> Vec<(String, String, String)> {
        self.convs
            .values()
            .filter(|c| !c.state.busy && !c.queue_hold)
            .filter_map(|c| {
                let s = c.state.solo.as_ref()?;
                if s.closed.is_some() || (s.pending_commit.is_some() && !s.fold_next) {
                    return None;
                }
                // Not after a failure or a refused approval (R-9), until the engineer says
                // something again.
                let last = c.state.turns.last();
                if last.is_some_and(|t| t.failure.is_some()) && !c.queue_go {
                    return None;
                }
                let w = s.queue.first()?;
                Some((c.id().to_string(), w.id.clone(), w.text.clone()))
            })
            .collect()
    }

    /// Sends a waiting message as the next turn, with its images and the files its text names
    /// (`refs`, read now): the turn's number, or `None` when it still waits for a slot. A
    /// failure holds the queue until the engineer says something: not tried every second.
    pub fn send_waiting(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        qid: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
    ) -> Option<u32> {
        let attachments = self
            .convs
            .get(conv)
            .and_then(|c| c.state.solo.as_ref())
            .and_then(|s| s.queue.iter().find(|w| w.id == qid))
            .map(|w| w.attachments.clone())
            .unwrap_or_default();
        // Checked again as they go: the kept bytes must still be the images attached (review).
        let attachments = attachments
            .iter()
            .map(|a| self.attachment_checked(conv, &a.sha))
            .collect::<Result<Vec<_>, _>>();
        match attachments.and_then(|a| self.solo_turn(ctx, conv, text, refs, Some(qid), a)) {
            Ok(v) => v["turn"].as_u64().map(|n| n as u32),
            Err(e) => {
                if let Some(c) = self.convs.get_mut(conv) {
                    c.queue_hold = true;
                    c.slot_wait = Some(e.clone());
                    c.tick();
                }
                self.console.push(format!("session {conv}: {e}"));
                None
            }
        }
    }

    /// 012 (R-10, T051): the commits the sessions on `repo` made, with each session's name: its
    /// turns' commits, its squashes, and the one commit a command of it made on another branch
    /// (a merge commit). A move that only pointed a branch at commits, or brought in several
    /// (a fast-forward, a pull), claims none of them: those may be the engineer's (review).
    pub fn session_commits(&self, repo: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for c in self.convs.values() {
            let Some(s) = c.state.solo.as_ref() else {
                continue;
            };
            if self
                .project(&c.state.project)
                .map(|p| p.path != repo)
                .unwrap_or(true)
            {
                continue;
            }
            let name = s.name.clone().unwrap_or_else(|| "session".into());
            for t in s.commits.values() {
                out.extend(
                    t.commits
                        .iter()
                        .chain(t.commit.iter())
                        .map(|k| (k.clone(), name.clone())),
                );
            }
            for o in &s.observed {
                if o.how == crate::conversation::solo::How::Commits
                    && o.n == Some(1)
                    && o.commits.len() == 1
                {
                    out.push((o.commits[0].clone(), name.clone()));
                }
            }
            out.extend(s.squashes.iter().map(|(_, k, _)| (k.clone(), name.clone())));
        }
        out
    }

    /// The session's part of its view (contracts/web-api.md, "Session").
    pub(super) fn solo_view(&self, c: &super::Conv, v: &mut Value) {
        let Some(s) = c.state.solo.as_ref() else {
            return;
        };
        // A session just made, nothing happened in it since: what its creation knew (issue #2).
        let primed = c
            .primed
            .as_ref()
            .filter(|p| p.seq == c.state.seq && p.at.elapsed() < std::time::Duration::from_secs(1));
        let base = s.base();
        let start_ref = s.start_branch.as_ref().map(|b| format!("refs/heads/{b}"));
        // The copy's HEAD and the starting branch's tip in one git process (issue #3), read
        // even for a session just made: the engineer may commit while its copy is made.
        let (head, start_tip) = {
            let mut names = vec!["HEAD"];
            names.extend(start_ref.as_deref());
            match git::revs(&s.worktree, &names) {
                Some(r) => (r[0].clone(), r.get(1).cloned()),
                None => (
                    git::head(&s.worktree).unwrap_or_default(),
                    start_ref
                        .as_ref()
                        .and_then(|r| git::rev(&s.worktree, r).ok()),
                ),
            }
        };
        let declared = match primed {
            Some(p) => p.declared,
            None => self
                .project(&c.state.project)
                .map(|p| !self.squad_of(p).gates.is_empty())
                .unwrap_or(false),
        };
        // "Always in this project" only for a prefix the server would accept (review 6).
        if let Some(p) = c.pending.as_ref() {
            let always = crate::project::commands::always_prefix(&p.command).ok();
            let safe = always.as_deref().is_some_and(|pre| {
                self.judge_cx(c.id(), &p.runtime).is_ok_and(|jx| {
                    judge::prefix_safe(
                        pre,
                        &judge::Ctx {
                            wt: &jx.wt,
                            branch: &jx.branch,
                            folder: &jx.folder,
                            protect: &jx.protect,
                        },
                    )
                })
            });
            if !safe {
                v["pending_approval"]["always"] = Value::Null;
                v["pending_approval"]["always_not"] =
                    json!("it could also allow commands Fallow asks about one by one");
            }
        }
        // 012 US5: what the assistant's runtime can do, for the composer and the web switch; the
        // instruction files the assistant was last handed (R-7), without reading any file here.
        let caps = self.solo_caps(c);
        let instructions: Vec<Value> = c
            .state
            .turns
            .iter()
            .rev()
            .find(|t| !t.instructions.is_empty() && !t.withdrawn)
            .map(|t| t.instructions.iter().map(|i| json!({ "path": i.path, "skipped": i.skipped, "cut": i.cut, "turn": t.n })).collect())
            .unwrap_or_default();
        v["session"] = json!({
            "id": c.id(),
            "name": s.name,
            "images": caps.images,
            "instructions": instructions,
            "agent": c.state.agent.clone().unwrap_or_else(|| ASSISTANT.to_string()),
            "branch": s.branch,
            "start": { "branch": s.start_branch, "commit": s.start_commit, "dirty": s.dirty_at_start },
            "worktree": s.worktree,
            "lfs": s.lfs,
            "web": { "on": s.web, "can_switch": caps.web_off },
            "mode": c.mode.name(),
            "mode_note": c.mode_note,
            "default_mode": self.settings.defaults.chat_mode.name(),
            "base": base,
            "head": head,
            "pending_commit": s.pending_commit.map(|n| json!({
                "n": n,
                "error": s.commits.get(&n).and_then(|t| t.failed.clone()),
                "fold_next": s.fold_next,
            })),
            "queue": s.queue,
            "slot_wait": c.slot_wait,
            "gone": c.gone,
            "gates": {
                "label": s.gates_label(&head, declared),
                "running": c.gates_running,
                "error": c.gates_error,
                "commit": s.gates.as_ref().map(|g| g.0.clone()),
                "results": s.gates.as_ref().map(|g| g.2.clone()).unwrap_or_default(),
            },
            "undone": s.undone,
            "engineer_edits": s.engineer_edits.iter().map(|(b, c, f)| json!({"before": b, "commit": c, "files": f})).collect::<Vec<_>>(),
            "editor_during_turn": c.editor_during_turn,
            // The starting branch's commits past the base (R-5; the ref watcher, T035, refreshes).
            // Counted only when the tip moved from the base (issue #3).
            "start_moved": start_tip.as_ref().filter(|t| **t != base.gates).zip(start_ref.as_ref()).and_then(|(_, r)| {
                git::count_between(&s.worktree, &base.gates, r)
            }).filter(|n| *n > 0).map(|n| json!({ "commits": n })),
            "changes_notice": super::solo_check::ONLY_THE_COPY,
            "changes_count": if primed.is_some() { 0 } else { changed_count(s, &head) },
            // 012 (R-10, R-11): what the session's commands changed elsewhere, its squashes, and
            // the engineer's folder after a command that touched it.
            "observed": s.observed,
            "squashes": s.squashes.iter().map(|(b, c, j)| json!({"branch": b, "commit": c, "upto_turn": j})).collect::<Vec<_>>(),
            "folder": s.folder.last().map(|(t, st)| json!({"turn": t, "state": st})),
            "suggestions": s.suggestions.iter().map(|(n, t, b)| json!({"n": n, "title": t, "brief": b})).collect::<Vec<_>>(),
            "features": s.features.iter().map(|(f, t, cv)| json!({"feature": f, "title": t, "conversation": cv})).collect::<Vec<_>>(),
            "closed": s.closed.as_ref().map(|(how, b, w)| json!({"how": how, "branch": b, "worktree": w})),
            "commits": s.commits,
        });
    }
}

/// The files a session changed since its base, its uncommitted ones included (FR-002). `head`
/// is the copy's: no commits to read when it is the base still (issue #3).
fn changed_count(s: &crate::conversation::solo::Solo, head: &str) -> usize {
    // Without its link git would read a repository around the folder (fifth review, 4).
    if !s.worktree.join(".git").is_file() {
        return 0;
    }
    let base = s.base().mark;
    let mut files: std::collections::BTreeSet<String> = if head == base {
        std::collections::BTreeSet::new()
    } else {
        git::files_between_z(&s.worktree, &base, "HEAD")
            .unwrap_or_default()
            .into_iter()
            .collect()
    };
    for f in git::session_changes(&s.worktree).unwrap_or_default() {
        files.insert(f);
    }
    files.len()
}

/// The turns' process records in a session's folder (`turn-<n>.pid`, R-19).
fn pid_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("turn") && n.ends_with(".pid"))
        })
        .collect()
}

/// How many bytes a folder holds, for "how much its sessions' records hold" (FR-025).
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(m) = e.metadata() else {
            continue;
        };
        total += if m.is_dir() {
            dir_size(&e.path())
        } else {
            m.len()
        };
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_slug_comes_from_the_first_words() {
        assert_eq!(
            slug("Fix the Sunday bug, please now!"),
            "fix-the-sunday-bug-please"
        );
        assert_eq!(slug("¿?"), "session");
        assert_eq!(name_of("first line\nsecond"), "first line");
    }
}
