//! 012 (US1): solo sessions in the studio: making one (research R-2), a turn (R-6, R-7, R-13),
//! its commit (R-3), the working copy settled before a turn (R-4), and the assistant's
//! permission prompts judged (R-6). A session is a conversation of kind `solo`; what only it
//! records is in `conversation::solo`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Kind, Owner, Studio, Tool, spawn_turn};
use crate::agent::rights;
use crate::agent::{self, Call, Session, prompts};
use crate::conversation::events::{Event as CEvent, InstructionRec};
use crate::conversation::solo::SoloEvent;
use crate::platform::job::Group;
use crate::project::{self, Project};
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
    let text = crate::platform::command("git")
        .current_dir(project)
        .args([
            "--no-optional-locks",
            "show",
            &format!("HEAD:{}", crate::config::squad::SQUAD_FILE),
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut notices = Vec::new();
    crate::config::Squad::parse(crate::config::squad::SQUAD_FILE, &text, &mut notices)
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
    let msg = format!(
        "Fallow solo {}, turn {}: {} (cut short)",
        c.id(),
        t.n,
        agent::clip(t.said.lines().next().unwrap_or_default(), 60)
    );
    let before = c.state.cancelling().and_then(|x| x.head.clone()).or_else(|| t.head.clone());
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
}

impl Studio {
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
    ) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("say something to the assistant".into());
        }
        let p = self.project(project)?.clone();
        if p.kind() != project::Kind::Git {
            return Err("this project is not a git repository yet".into());
        }
        let assistant = agent_name.unwrap_or_else(|| self.default_assistant(&p));
        if let Some(why) = self.assistant_problem(&p.path, &assistant) {
            return Err(why);
        }
        let start_commit = git::head(&p.path)
            .map_err(|_| "the repository has no commit yet".to_string())?;
        let start_branch = git::branch(&p.path);
        let dirty = !git::folder_status(&p.path).unwrap_or_default().is_empty();
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
        let wt = placeholder.join(&id);
        let first = format!("fallow/solo/{id}");
        {
            let c = self.conv_mut(&id)?;
            c.append(CEvent::Solo(SoloEvent::SessionCreating {
                branch: first.clone(),
                start_commit: start_commit.clone(),
            }))?;
        }
        let made = std::fs::create_dir_all(&placeholder)
            .map_err(|e| e.to_string())
            .and_then(|()| git::worktree_add_new(&p.path, &wt, &first, &start_commit));
        if let Err(e) = made {
            self.forget_half_made(&id, &p.path, &wt, &first);
            return Err(format!("the session's working copy could not be made: {e}"));
        }
        let lfs = std::fs::read_to_string(wt.join(".gitattributes"))
            .is_ok_and(|a| a.contains("filter=lfs"));
        // The branch after the first message, never overwriting one (FR-015).
        let base = format!("fallow/solo/{}", slug(text));
        let mut branch = base.clone();
        let mut k = 2;
        while git::rev_exists(&p.path, &format!("refs/heads/{branch}")) {
            branch = format!("{base}-{k}");
            k += 1;
        }
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
        let pm = self.project_mut(&p.id)?;
        pm.sessions.push(id.clone());
        pm.last_session = Some(id.clone());
        self.store.save()?;
        // The session exists now: a first turn that cannot start is said with it, never hidden
        // behind an error (review 8: the draft would make a second session).
        let mut v = match self.say_solo(ctx, &id, text, refs) {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        };
        v["session"] = json!(id);
        Ok(v)
    }

    /// Nothing is left of a session that could not be made (second review, low 3; third review,
    /// low 2): its record, its working copy, and the branch made for it.
    fn forget_half_made(&mut self, id: &str, repo: &Path, wt: &Path, branch: &str) {
        let dir = self.convs.remove(id).map(|c| c.dir.clone());
        if wt.exists() {
            let _ = git::worktree_remove(repo, wt);
        }
        git::worktree_prune(repo);
        if git::rev_exists(repo, &format!("refs/heads/{branch}")) {
            let _ = git::delete_branch(repo, branch);
        }
        if let Some(d) = dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// What the judge needs for `conv`'s session.
    pub(super) fn judge_cx(&self, conv: &str, runtime: &str) -> Result<JudgeCx, String> {
        let c = self.convs.get(conv).ok_or("the session is gone")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        let p = self.project(&c.state.project)?;
        let caps = agent::registry::capabilities(runtime, &self.settings);
        let mut protect: Vec<String> = rights::CONTROL_GLOBS.iter().map(|s| s.to_string()).collect();
        protect.extend(caps.control_files);
        let gates = squad_at_head(&p.path)
            .gates
            .iter()
            .map(|g| g.command.clone())
            .collect();
        Ok(JudgeCx {
            wt: solo.worktree.clone(),
            branch: solo.branch.clone(),
            folder: p.path.clone(),
            protect,
            gates,
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
        self.solo_turn(ctx, conv, text, refs, None)
    }

    /// `say_solo`, or with `queued` (its id) a waiting message being sent: it is not queued
    /// again when it still has to wait (`{"waiting": …}`), and it leaves the queue as its turn
    /// starts (third review, low 1: never sent twice).
    fn solo_turn(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        text: &str,
        refs: Vec<prompts::FileRef>,
        queued: Option<&str>,
    ) -> Result<Value, String> {
        let from_queue = queued.is_some();
        let text = text.trim();
        if text.is_empty() {
            return Err("say something to the assistant".into());
        }
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        if solo.closed.is_some() {
            return Err("the session is closed".into());
        }
        if let Some(n) = solo.pending_commit {
            if !solo.fold_next {
                return Err(format!(
                    "Fallow could not commit turn {n}: try again, fix it in the next turn, or undo those changes"
                ));
            }
        }
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
                attachments: Vec::new(),
            }))?;
            if !idle {
                return Ok(json!({ "queued": id }));
            }
            // Idle: the engineer's message is their go-ahead for those waiting (second review):
            // the oldest goes now, the rest after it.
            c.queue_hold = false;
            c.queue_go = true;
            let first = c
                .state
                .solo
                .as_ref()
                .and_then(|s| s.queue.first())
                .map(|w| (w.id.clone(), w.text.clone()));
            let mut v = json!({ "queued": id });
            if let Some((qid, qtext)) = first {
                match self.say_solo_queued(ctx, conv, &qid, &qtext) {
                    Ok(Some(n)) => v["turn"] = json!(n),
                    Ok(None) => {}
                    // Said as the waiting messages' state, never as an error: the new message
                    // is already queued (third review, medium).
                    Err(e) => {
                        let c = self.conv_mut(conv)?;
                        c.queue_hold = true;
                        c.slot_wait = Some(e.clone());
                        c.tick();
                        self.console.push(format!("session {conv}: {e}"));
                        v["waiting"] = json!(e);
                    }
                }
            }
            return Ok(v);
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
                attachments: Vec::new(),
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
        let mut engineer_edits = Vec::new();
        let dirty = git::changes_all(&wt).unwrap_or_default();
        if !dirty.is_empty() && solo.pending_commit.is_none() {
            let msg = format!(
                "Fallow solo {conv}: the engineer's edits before turn {n}"
            );
            match git::snapshot(&wt, &solo.branch, &msg) {
                Ok(Some(commit)) => {
                    let files: Vec<String> =
                        dirty.iter().map(|f| f.trim_matches('"').to_string()).collect();
                    engineer_edits = files.clone();
                    self.conv_mut(conv)?
                        .append(CEvent::Solo(SoloEvent::EngineerEdits {
                            before: format!("turn {n}"),
                            commit,
                            files,
                        }))?;
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(format!(
                        "Fallow could not commit your edits in the session's working copy: {e}"
                    ));
                }
            }
        }
        let lib = self.library(Some(&p.path));
        let def = lib
            .agent(&assistant)
            .cloned()
            .ok_or_else(|| format!("there is no agent {assistant}"))?;
        let runtime = agent::registry::runtime(&def.runtime, &ctx.settings)
            .map_err(|e| format!("the assistant cannot start: {e}"))?;
        let jc = self.judge_cx(conv, &def.runtime)?;
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
        let instr = agent::instructions::read(&wt, &agent::instructions::extra_at_head(&p.path));
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
            undone: &[],
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
            group: Some(group),
            attachments: Vec::new(),
            web: solo.web,
            user_settings: true,
        };
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
            mode: c.state.mode,
            session: session.id().to_string(),
            runtime: Some(def.runtime.clone()),
            model: Some(def.model.clone()).filter(|m| !m.is_empty()),
            new_session: !matches!(session, Session::Resume(_)),
            instructions: instr_recs,
            attachments: Vec::new(),
            slot_waited_ms: None,
            head: git::head(&wt).ok(),
        })?;
        c.live.clear();
        c.head_before = git::head(&wt).ok();
        c.activity.clear();
        c.token = Some(token);
        c.stop = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&c.stop);
        let turn_file = c.turns_dir().join(format!("{n}.jsonl"));
        let _ = std::fs::create_dir_all(c.turns_dir());
        if let Err(e) = spawn_turn(ctx.tx.clone(), conv.to_string(), n, runtime, call, stop, turn_file) {
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
        let cut = c.state.cancelling().is_some() || c.stop.load(std::sync::atomic::Ordering::Relaxed);
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
            Ok(_) => (head.clone().filter(|_| !commits.is_empty()), None, Vec::new()),
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
                (head.clone().filter(|_| !commits.is_empty()), Some(e.clone()), changed)
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
            c.append(CEvent::Solo(SoloEvent::FeatureSuggested { n, title, brief }))?;
        }
        c.tick();
        Ok(())
    }

    /// 012 (FR-008, FR-009, R-6): a permission prompt from a session's assistant, judged in the
    /// contract's terms. `Ok` when it was answered or is waiting for the engineer; `Err` with
    /// why it is denied.
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
        let ctxj = judge::Ctx {
            wt: &jc.wt,
            branch: &jc.branch,
            folder: &jc.folder,
            protect: &jc.protect,
        };
        let allow = |reply: &Sender<Reply>| {
            let _ = reply.send(Reply::Ok(agent::registry::permission_reply(
                runtime, true, "", raw,
            )));
        };
        match req.kind {
            agent::RequestKind::Web => {
                if solo.web {
                    allow(&reply);
                    Ok(())
                } else {
                    Err("Web access is off for this session.".into())
                }
            }
            agent::RequestKind::Edit => {
                let path = req.path.clone().unwrap_or_default();
                match judge::edit_problem(&path, &ctxj) {
                    None => {
                        allow(&reply);
                        Ok(())
                    }
                    Some(why) => Err(format!("The assistant may not change {path}: {why}")),
                }
            }
            agent::RequestKind::Other => Err(format!(
                "{} is not a tool Fallow can judge.",
                if req.tool.is_empty() { "That" } else { &req.tool }
            )),
            agent::RequestKind::Run => {
                let command = req.command.clone().unwrap_or_default();
                let p = self.project(&c.state.project)?;
                if judge::runs_unasked(&command, &jc.gates, &p.approved_commands, &ctxj) {
                    allow(&reply);
                    return Ok(());
                }
                let verdict = judge::assess(&command, &ctxj);
                let until = c.allow_until_switch;
                if until && verdict == judge::Verdict::Allow {
                    allow(&reply);
                    return Ok(());
                }
                if c.pending.is_some() {
                    return Err("Another command is waiting for the engineer.".into());
                }
                let reasons = match verdict {
                    judge::Verdict::Ask(why) => why,
                    judge::Verdict::Allow => Vec::new(),
                };
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
                    merge: None,
                })
                .map_err(|_| "Fallow could not record the question.".to_string())?;
                c.pending = Some(Pending {
                    id,
                    tool: req.tool.clone(),
                    command,
                    raw: raw.clone(),
                    runtime: runtime.to_string(),
                    reply,
                    deadline: std::time::Instant::now() + wait,
                });
                Ok(())
            }
        }
    }

    /// 012 (FR-008): "Ask for commands" or "until I switch back" (in memory only).
    pub fn set_solo_mode(&mut self, conv: &str, mode: &str) -> Result<Value, String> {
        let c = self.conv_mut(conv)?;
        if c.state.solo.is_none() {
            return Err("not a session".into());
        }
        c.allow_until_switch = match mode {
            "ask" => false,
            "until_switch" => true,
            other => return Err(format!("no mode {other:?}: ask or until_switch")),
        };
        c.tick();
        Ok(json!({}))
    }

    /// 012 (R-9, R-13): a waiting message is sent when the turn before it ended and a slot is
    /// free; after a restart it waits for the engineer.
    pub fn solo_tick(&mut self, ctx: &Ctx) {
        let ready: Vec<(String, String, String)> = self
            .convs
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
            .collect();
        for (conv, qid, text) in ready {
            match self.say_solo_queued(ctx, &conv, &qid, &text) {
                Ok(_) => {}
                Err(e) => {
                    // Held until the engineer says something: not tried again every second.
                    if let Some(c) = self.convs.get_mut(&conv) {
                        c.queue_hold = true;
                        c.slot_wait = Some(e.clone());
                        c.tick();
                    }
                    self.console.push(format!("session {conv}: {e}"));
                }
            }
        }
    }

    /// Sends a waiting message as the next turn: the turn's number, or `None` when it still
    /// waits for a slot.
    fn say_solo_queued(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        qid: &str,
        text: &str,
    ) -> Result<Option<u32>, String> {
        let v = self.solo_turn(ctx, conv, text, Vec::new(), Some(qid))?;
        Ok(v["turn"].as_u64().map(|n| n as u32))
    }

    /// The session's part of its view (contracts/web-api.md, "Session").
    pub(super) fn solo_view(&self, c: &super::Conv, v: &mut Value) {
        let Some(s) = c.state.solo.as_ref() else {
            return;
        };
        let head = git::head(&s.worktree).unwrap_or_default();
        let declared = self
            .project(&c.state.project)
            .map(|p| !squad_at_head(&p.path).gates.is_empty())
            .unwrap_or(false);
        let base = s.base();
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
        v["session"] = json!({
            "id": c.id(),
            "name": s.name,
            "agent": c.state.agent.clone().unwrap_or_else(|| ASSISTANT.to_string()),
            "branch": s.branch,
            "start": { "branch": s.start_branch, "commit": s.start_commit, "dirty": s.dirty_at_start },
            "worktree": s.worktree,
            "lfs": s.lfs,
            "web": { "on": s.web },
            "mode": if c.allow_until_switch { "until_switch" } else { "ask" },
            "base": base,
            "head": head,
            "pending_commit": s.pending_commit.map(|n| json!({
                "n": n,
                "error": s.commits.get(&n).and_then(|t| t.failed.clone()),
                "fold_next": s.fold_next,
            })),
            "queue": s.queue,
            "slot_wait": c.slot_wait,
            "gates": { "label": s.gates_label(&head, declared) },
            "suggestions": s.suggestions.iter().map(|(n, t, b)| json!({"n": n, "title": t, "brief": b})).collect::<Vec<_>>(),
            "features": s.features.iter().map(|(f, t, cv)| json!({"feature": f, "title": t, "conversation": cv})).collect::<Vec<_>>(),
            "closed": s.closed.as_ref().map(|(how, b, w)| json!({"how": how, "branch": b, "worktree": w})),
            "commits": s.commits,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_slug_comes_from_the_first_words() {
        assert_eq!(slug("Fix the Sunday bug, please now!"), "fix-the-sunday-bug-please");
        assert_eq!(slug("¿?"), "session");
        assert_eq!(name_of("first line\nsecond"), "first line");
    }
}
