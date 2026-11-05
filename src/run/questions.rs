//! Questions through the lead (007 research S-10, contracts/mcp-tools.md): an agent's `ask`,
//! the lead's answer forked from the spec session, relaying to the engineer by the autonomy
//! level, committed answers, and overruling.
//!
//! Agents never reach the engineer or each other: a question goes to Fallow, which asks the
//! lead, applies its own rules to what the lead says, commits every answer to the spec's
//! Clarifications, and records each change of the question in the loop's log (constitution III).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::engine::{Ctx, Msg, Reply, Run};
use super::events::*;
use super::state::{LoopState, QuestionEntry};
use crate::agent::{self, Call, Session, prompts, rights};
use crate::platform::job::Group;
use crate::speckit;
use crate::verify::git;

/// A loop step's tools: its token, accepted only from the step's own processes while it runs.
pub struct StepTool {
    pub token: String,
    pub n: u32,
    pub group: Arc<Group>,
}

/// What the lead's call needs from the studio: the spec session to fork, the feature's
/// worktree (on the feature branch, where answers are committed), and the project's approved
/// commands.
#[derive(Debug, Clone)]
pub struct Lead {
    pub session: Option<String>,
    pub workdir: PathBuf,
    pub approved: Vec<String>,
    /// The lead agent of the feature's spec session (009 US2).
    pub agent: String,
}

/// An agent's `ask` waiting for the lead.
pub struct Asking {
    pub q: String,
    pub reply: Sender<Reply>,
    pub stop: Arc<AtomicBool>,
}

pub const ONE_OPEN: &str = "you already have a question open: wait for its answer, or end your step with status blocked and its id";

/// The tools of a loop step (contracts/mcp-tools.md).
pub fn tools() -> Value {
    json!([
        {
            "name": "ask",
            "description": "Asks a question about the feature when the locked documents do not settle it. Fallow asks the lead; the answer comes back here, committed to spec.md, or the question waits for the engineer: then end your step with status blocked and the question id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["question"]
            }
        },
        {
            "name": "progress",
            "description": "A short line for the engineer's activity feed: what you are doing now.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        }
    ])
}

/// The MCP configuration of step `n`: a fresh token for Fallow's bridge, written next to the
/// step's instruction.
pub fn bridge(
    run: &Run,
    ctx: &Ctx,
    n: u32,
    group: Arc<Group>,
) -> Result<(StepTool, PathBuf), String> {
    let token = crate::mcp::identity::Token::new()
        .map_err(|e| e.to_string())?
        .hex();
    let mut args = vec![
        "mcp-bridge".to_string(),
        "--fallow-pid".into(),
        ctx.pid.to_string(),
        "--step".into(),
        token.clone(),
    ];
    if let Some(e) = &ctx.endpoint {
        args.push("--endpoint".into());
        args.push(e.clone());
    }
    let path = run.steps_dir().join(format!("{n}.mcp.json"));
    std::fs::write(
        &path,
        json!({ "mcpServers": { "fallow": { "command": ctx.exe, "args": args } } }).to_string(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((StepTool { token, n, group }, path))
}

/// The running step that holds this tool token (several may run at once, 008 R-7).
fn step_of_token(run: &Run, token: &str) -> Option<u32> {
    run.running
        .iter()
        .find(|(_, x)| x.tool.token == token)
        .map(|(n, _)| *n)
}

/// A bridge opening for a loop step.
pub fn open(run: &Run, token: &str, pid: u32) -> Result<Value, String> {
    let n = step_of_token(run, token).ok_or("unknown or finished step")?;
    let t = &run.running[&n].tool;
    if !t.group.contains(pid) {
        return Err("the connecting process is not part of the step".into());
    }
    Ok(tools())
}

/// A tool call from a loop step. `ask` is answered later, when the lead has answered.
pub fn call(
    run: &mut Run,
    ctx: &Ctx,
    lead: Result<Lead, String>,
    token: &str,
    name: &str,
    args: Value,
    reply: Sender<Reply>,
) {
    let Some(n) = step_of_token(run, token) else {
        let _ = reply.send(Reply::Err("unknown or finished step".into()));
        return;
    };
    match name {
        // The line itself is in the step's activity, from the tool call.
        "progress" => {
            let _ = reply.send(Reply::Ok(json!({})));
        }
        "ask" => {
            if let Err(e) = ask(run, ctx, lead, n, &args, reply.clone()) {
                let _ = reply.send(Reply::Err(e));
            }
        }
        _ => {
            let _ = reply.send(Reply::Err(format!("no tool {name:?}")));
        }
    }
}

fn lp(run: &Run) -> Result<LoopState, String> {
    run.state
        .feature_loop
        .clone()
        .ok_or_else(|| "not a feature loop".to_string())
}

/// The story a step works on: its phase's story, else the story of its task.
fn story_of(l: &LoopState, phase: Option<u32>, task: Option<&str>) -> Option<String> {
    let p = l.phases.iter().find(|p| Some(p.spec.n) == phase)?;
    p.spec.story.clone().or_else(|| {
        p.spec
            .tasks
            .iter()
            .find(|t| Some(t.id.as_str()) == task)
            .and_then(|t| t.story.clone())
    })
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Strict => "strict",
        Level::Normal => "normal",
        Level::Free => "free",
    }
}

fn open_of(l: &LoopState, n: u32) -> Option<&QuestionEntry> {
    l.questions.values().find(|q| {
        q.step == n
            && matches!(
                q.state,
                QuestionState::WithLead | QuestionState::WaitingEngineer
            )
    })
}

/// A question that waits for the engineer: the loop waits for it (FR-021).
pub fn waiting(l: &LoopState) -> Option<&QuestionEntry> {
    l.questions
        .values()
        .find(|q| q.state == QuestionState::WaitingEngineer)
}

#[allow(clippy::too_many_arguments)]
fn record(
    run: &mut Run,
    q: &QuestionEntry,
    state: QuestionState,
    by: Option<&str>,
    answer: Option<String>,
    commit: Option<String>,
    options: Vec<String>,
    recommended: Option<String>,
    cost_usd: f64,
    reopened: Vec<String>,
    why: Option<String>,
) -> Result<(), String> {
    run.append(Event::Question {
        id: q.id.clone(),
        step: q.step,
        task: q.task.clone(),
        text: q.text.clone(),
        options,
        state,
        by: by.map(str::to_string),
        answer,
        commit,
        affects: q.affects.clone(),
        recommended,
        cost_usd,
        reopened,
        why,
    })
}

fn ask(
    run: &mut Run,
    ctx: &Ctx,
    lead: Result<Lead, String>,
    n: u32,
    args: &Value,
    reply: Sender<Reply>,
) -> Result<(), String> {
    let l = lp(run)?;
    if run.state.stage.is_final() {
        return Err("the loop is over".into());
    }
    if open_of(&l, n).is_some() || run.running.get(&n).is_some_and(|x| x.asking.is_some()) {
        return Err(ONE_OPEN.into());
    }
    let text = args["question"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("the question is empty".into());
    }
    let options: Vec<String> = args["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o.as_str().map(str::to_string))
        .filter(|o| !o.trim().is_empty())
        .collect();
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    let id = format!("q{}", l.questions.len() + 1);
    let task = step.task.clone().or_else(|| step.part.clone());
    let entry = QuestionEntry {
        id: id.clone(),
        step: n,
        task: task.clone(),
        text: text.clone(),
        options: options.clone(),
        state: QuestionState::WithLead,
        by: None,
        answer: None,
        commit: None,
        affects: story_of(&l, step.phase, step.task.as_deref()),
        recommended: None,
        reopened: Vec::new(),
        why: None,
        seq: 0,
        asked_seq: 0,
        history: Vec::new(),
    };
    record(
        run,
        &entry,
        QuestionState::WithLead,
        None,
        None,
        None,
        options.clone(),
        None,
        0.0,
        Vec::new(),
        None,
    )?;
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(x) = run.running.get_mut(&n) {
        x.asking = Some(Asking {
            q: id.clone(),
            reply,
            stop: Arc::clone(&stop),
        });
    }
    // Fallow's own refusals go straight to the engineer, without the lead.
    let lead = match lead {
        Ok(x) => x,
        Err(e) => {
            return lead_done(
                run,
                None,
                &id,
                Err(format!("the lead cannot be asked: {e}")),
            );
        }
    };
    match start_lead(run, ctx, &l, &lead, &entry, stop) {
        Ok(()) => Ok(()),
        Err(e) => lead_done(
            run,
            Some(&lead),
            &id,
            Err(format!("the lead cannot be asked: {e}")),
        ),
    }
}

/// The lead's call for a question, on a thread of its own: a fork of the spec session, in a
/// worktree of the feature branch that Fallow throws away (it changes nothing; Fallow commits).
fn start_lead(
    run: &Run,
    ctx: &Ctx,
    l: &LoopState,
    lead: &Lead,
    q: &QuestionEntry,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let lib = crate::defs::Library::load(ctx.config.as_deref(), Some(&run.state.repo));
    let def = lib
        .agent(&lead.agent)
        .filter(|d| d.kind == crate::defs::agent::Kind::Lead)
        .cloned()
        .ok_or_else(|| format!("the lead {} is gone", lead.agent))?;
    let runtime = agent::registry::runtime(&def.runtime, &ctx.settings)
        .map_err(|e| format!("the lead cannot answer: {e}"))?;
    let at = git::head(&lead.workdir)?;
    let spec_text = git::show(
        &run.state.repo,
        &at,
        &format!("specs/{}/spec.md", l.feature),
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let reserved = speckit::spec::read("spec.md", &spec_text)
        .map(|s| s.all_reserved())
        .unwrap_or_default();
    let wt = run.worktrees().join(format!("lead-{}", q.id));
    let _ = git::worktree_remove(&run.state.repo, &wt);
    git::worktree_add(&run.state.repo, &wt, None, &at)?;
    let asked_by = run
        .state
        .steps
        .get(&q.step)
        .and_then(|s| s.def.clone())
        .unwrap_or_else(|| "builder".into());
    let feature_dir = format!("specs/{}", l.feature);
    let instruction = prompts::loop_answer(&prompts::AskCx {
        id: &q.id,
        feature_dir: &feature_dir,
        asked_by: &asked_by,
        task: q.task.as_deref(),
        question: &q.text,
        options: &q.options,
        level: level_name(l.level),
        reserved: &reserved,
    });
    let _ = std::fs::write(
        run.steps_dir().join(format!("{}.prompt", q.id)),
        &instruction,
    );
    let session = match &lead.session {
        Some(s) => Session::Fork(s.clone()),
        None => Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?),
    };
    let remaining = (run.state.limits.usd - run.state.cost_usd).max(0.01);
    let call = Call {
        instruction,
        schema: Some(agent::schemas::answer()),
        rights: rights::lead_in_loop(lead.approved.clone()),
        mcp_config: None,
        session,
        model: Some(def.model.clone()).filter(|m| !m.is_empty()),
        budget_usd: Some(def.limits.usd.map_or(remaining, |u| u.min(remaining))),
        cwd: wt,
        time_limit: Duration::from_secs(ctx.settings.defaults.lead_answer_sec.max(1)),
        system: Some(prompts::lead_system(&def.body)),
        partial: false,
        group: Some(Arc::new(
            Group::new().map_err(|e| format!("cannot create a process group: {e}"))?,
        )),
        attachments: Vec::new(),
        web: false,
        user_settings: false,
    };
    let tx = ctx.tx.clone();
    let id = run.id();
    let qid = q.id.clone();
    std::thread::Builder::new()
        .name(format!("lead-{id}-{qid}"))
        .spawn(move || {
            let outcome = runtime.start(&call, &mut |_| {}, &stop);
            let _ = tx.send(Msg::LeadAnswer {
                run: id,
                q: qid,
                outcome,
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// Why Fallow relays what the lead said, whatever the lead said (FR-020a): `None` when the
/// lead's answer stands.
pub fn relay_reason(
    level: Level,
    v: &Value,
    question: &str,
    reserved: &[String],
) -> Option<String> {
    let outcome = v["outcome"].as_str().unwrap_or_default();
    let answer = v["answer"].as_str().unwrap_or_default();
    let basis = v["basis"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if level == Level::Strict {
        return Some("the autonomy level is strict".into());
    }
    if v["reserved"].as_bool() != Some(false) {
        return Some("it touches an item reserved for the engineer".into());
    }
    // Fallow checks the reserved items itself too: the lead's word is a claim.
    let text = format!("{question}\n{answer}").to_lowercase();
    if let Some(r) = reserved
        .iter()
        .find(|r| !r.trim().is_empty() && text.contains(&r.trim().to_lowercase()))
    {
        return Some(format!("it touches {r:?}, reserved for the engineer"));
    }
    if v["changes_criterion"].as_bool() != Some(false) {
        return Some("it would change an acceptance scenario".into());
    }
    match outcome {
        "answer" | "decided" if answer.trim().is_empty() => Some("the lead gave no answer".into()),
        "answer" if basis.is_empty() || basis == "none" => {
            Some("the lead's answer has no basis in the documents or the conversations".into())
        }
        "answer" => None,
        // Small, as FR-020a defines it: no public interface, no dependency (reserved items
        // and acceptance scenarios are checked above). A missing flag counts as set.
        "decided" if level == Level::Free && v["touches_interface"].as_bool() != Some(false) => {
            Some("it touches a public interface, which the lead may not decide".into())
        }
        "decided" if level == Level::Free && v["adds_dependency"].as_bool() != Some(false) => {
            Some("it adds a dependency, which the lead may not decide".into())
        }
        "decided" if level == Level::Free => None,
        "decided" => Some("only the free level lets the lead decide an open question".into()),
        "relay" => Some("the lead relayed it".into()),
        _ => Some("the lead gave no usable answer".into()),
    }
}

/// The lead's call ended: Fallow's rules decide whether its answer stands, then the answer is
/// committed and the asking agent gets it, or the question waits for the engineer.
pub fn lead_done(
    run: &mut Run,
    lead: Option<&Lead>,
    q: &str,
    outcome: Result<agent::Outcome, String>,
) -> Result<(), String> {
    let l = lp(run)?;
    let Some(entry) = l.questions.get(q).cloned() else {
        return Ok(());
    };
    if entry.state != QuestionState::WithLead {
        return Ok(());
    }
    let _ = git::worktree_remove(&run.state.repo, &run.worktrees().join(format!("lead-{q}")));
    let cost = outcome
        .as_ref()
        .ok()
        .and_then(|o| o.cost_usd)
        .unwrap_or(0.0);
    let value = match &outcome {
        Ok(o) => match (&o.result, &o.failure) {
            (Some(v), None) => Ok(v.clone()),
            (_, Some(f)) => Err(format!("the lead's call failed: {f}")),
            (None, None) => Err("the lead gave no result".to_string()),
        },
        Err(e) => Err(e.clone()),
    };
    let reserved = lead
        .and_then(|ld| git::head(&ld.workdir).ok())
        .and_then(|h| {
            git::show(&run.state.repo, &h, &format!("specs/{}/spec.md", l.feature))
                .ok()
                .flatten()
        })
        .and_then(|t| speckit::spec::read("spec.md", &t).ok())
        .map(|s| s.all_reserved())
        .unwrap_or_default();
    let why = match (&value, lead) {
        (Err(e), _) => Some(e.clone()),
        _ if run.state.stage.is_final() => Some("the loop is over".into()),
        // Answers are not committed while the documents are open again (research S-17).
        _ if l.unlocked => Some("the feature is unlocked".into()),
        (_, None) => Some("Fallow could not find the feature's spec session to commit to".into()),
        (Ok(v), Some(_)) => relay_reason(l.level, v, &entry.text, &reserved),
    };
    // The step that asked, if it still runs, gets the answer within its step.
    let reply = run
        .running
        .values_mut()
        .find_map(|x| x.asking.take_if(|a| a.q == q))
        .map(|a| a.reply);
    let send = |v: Value| {
        if let Some(r) = &reply {
            let _ = r.send(Reply::Ok(v));
        }
    };
    if why.is_none() {
        let v = value.as_ref().expect("checked");
        let decided = v["outcome"] == "decided";
        let answer = v["answer"].as_str().unwrap_or_default().trim().to_string();
        let ld = lead.expect("checked");
        // The lead's own name (009): `lead` for the default one, so its records read as before.
        let who = if decided {
            format!("{}, decided", ld.agent)
        } else {
            ld.agent.clone()
        };
        match commit_answer(run, &l, ld, &entry, &who, &answer) {
            Ok(commit) => {
                record(
                    run,
                    &entry,
                    if decided {
                        QuestionState::Decided
                    } else {
                        QuestionState::Answered
                    },
                    Some(ld.agent.as_str()),
                    Some(answer.clone()),
                    Some(commit.clone()),
                    entry.options.clone(),
                    None,
                    cost,
                    Vec::new(),
                    None,
                )?;
                send(json!({
                    "state": if decided { "decided" } else { "answered" },
                    "answer": answer, "by": ld.agent, "by_lead": true, "commit": commit
                }));
                return Ok(());
            }
            Err(e) => {
                return relay(
                    run,
                    &entry,
                    &value,
                    cost,
                    &format!("Fallow could not commit the answer: {e}"),
                    send,
                );
            }
        }
    }
    relay(
        run,
        &entry,
        &value,
        cost,
        why.as_deref().unwrap_or(""),
        send,
    )
}

fn relay(
    run: &mut Run,
    entry: &QuestionEntry,
    value: &Result<Value, String>,
    cost: f64,
    why: &str,
    send: impl Fn(Value),
) -> Result<(), String> {
    let v = value.as_ref().ok();
    let mut options: Vec<String> = v
        .and_then(|v| v["options"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|o| o["label"].as_str().map(str::to_string))
        .filter(|o| !o.trim().is_empty())
        .collect();
    let mut recommended = v
        .and_then(|v| v["options"].as_array())
        .into_iter()
        .flatten()
        .find(|o| o["recommended"] == true)
        .and_then(|o| o["label"].as_str().map(str::to_string));
    // What the lead would have said, when it answered or decided and Fallow relays all the same,
    // is its recommendation. When the lead relays, its `answer` is its reasoning, never a
    // choice (T086: it showed as an option).
    if let Some(a) = v
        .filter(|v| v["outcome"] != "relay")
        .and_then(|v| v["answer"].as_str())
        .filter(|a| !a.trim().is_empty())
    {
        if recommended.is_none() {
            recommended = Some(a.trim().to_string());
        }
        if !options.iter().any(|o| o == a.trim()) {
            options.insert(0, a.trim().to_string());
        }
    }
    if options.is_empty() {
        options = entry.options.clone();
    }
    record(
        run,
        entry,
        QuestionState::WaitingEngineer,
        None,
        None,
        None,
        options,
        recommended,
        cost,
        Vec::new(),
        Some(why.to_string()).filter(|w| !w.is_empty()),
    )?;
    send(json!({
        "state": "waiting",
        "question": entry.id,
        "why": why,
        "next": format!("End your step now with status blocked and question {:?}. You resume in this session with the engineer's answer.", entry.id),
    }));
    Ok(())
}

/// Commits an answer to the feature branch's `spec.md`, under `## Clarifications`, as
/// `- Q (T004, builder): … → A (lead): …` (contracts/mcp-tools.md).
fn commit_answer(
    run: &Run,
    l: &LoopState,
    lead: &Lead,
    q: &QuestionEntry,
    who: &str,
    answer: &str,
) -> Result<String, String> {
    let rel = format!("specs/{}/spec.md", l.feature);
    let path = lead.workdir.join(&rel);
    // Only the clarification goes into an answer's commit: an edit of spec.md not committed
    // would reach the build with it, past the lock (Phase 6 review).
    let text = git::show(&lead.workdir, "HEAD", &rel)?
        .ok_or_else(|| format!("{rel} is not committed on the feature branch"))?;
    let now = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if now.replace("\r\n", "\n") != text.replace("\r\n", "\n") {
        return Err(format!(
            "{rel} has changes in the spec session that are not committed; Fallow does not commit them with an answer"
        ));
    }
    let asked_by = run
        .state
        .steps
        .get(&q.step)
        .and_then(|s| s.def.clone())
        .unwrap_or_else(|| "builder".into());
    let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let entry = format!(
        "Q ({}, {asked_by}): {} → A ({who}): {}",
        q.task.as_deref().unwrap_or("loop"),
        one_line(&q.text),
        one_line(answer)
    );
    let date = super::time::rfc3339(super::time::unix_now());
    let new = speckit::spec::add_clarification(&text, &date[..10], &entry);
    std::fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
    git::commit_all(
        &lead.workdir,
        &format!("{}: answer {} ({who})", l.feature, q.id),
        Some(&[rel.as_str()]),
    )?
    .ok_or_else(|| "nothing to commit".to_string())
}

/// The newest commit of an answer, which the build branch takes in at the next task boundary.
pub fn latest_answer(l: &LoopState) -> Option<String> {
    l.questions
        .values()
        .filter(|q| q.commit.is_some())
        .max_by_key(|q| q.seq)
        .and_then(|q| q.commit.clone())
}

/// Every commit holding an answer: the locked documents may differ from the lock's by these.
pub fn answer_commits(l: &LoopState) -> Vec<String> {
    l.questions
        .values()
        .flat_map(|q| q.history.iter().filter_map(|h| h.commit.clone()))
        .collect()
}

/// The engineer answers a question that waits for them: the answer is committed, and the step
/// that asked resumes in its session with it (FR-021).
pub fn engineer_answer(run: &mut Run, lead: &Lead, q: &str, answer: &str) -> Result<Value, String> {
    let l = lp(run)?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("the answer is empty".into());
    }
    let entry = l
        .questions
        .get(q)
        .cloned()
        .ok_or("there is no such question")?;
    if entry.state != QuestionState::WaitingEngineer {
        return Err(format!("{q} is not waiting for you"));
    }
    if run.state.stage.is_final() {
        return Err("the loop is over".into());
    }
    if l.unlocked {
        return Err("the feature is unlocked: lock it again, then answer the question here".into());
    }
    let commit = commit_answer(run, &l, lead, &entry, "engineer", answer)?;
    record(
        run,
        &entry,
        QuestionState::Answered,
        Some("engineer"),
        Some(answer.to_string()),
        Some(commit.clone()),
        entry.options.clone(),
        entry.recommended.clone(),
        0.0,
        Vec::new(),
        None,
    )?;
    run.append(Event::Steering {
        kind: SteeringKind::Answer,
        target: entry.task.clone(),
        detail: Some(answer.to_string()),
    })?;
    // The step that asked resumes with it once it has ended.
    deliver(run)?;
    Ok(json!({ "commit": commit }))
}

/// Relayed: its question went to the engineer.
fn relayed(q: &QuestionEntry) -> bool {
    q.state == QuestionState::WaitingEngineer
        || (q.state == QuestionState::Answered && q.by.as_deref() == Some("engineer"))
}

/// The question of step `n` that went to the engineer: whatever the step returned, it ends
/// blocked on it and resumes in its session with the answer (FR-021).
pub fn relayed_of(l: &LoopState, n: u32) -> Option<&QuestionEntry> {
    l.questions.values().find(|q| q.step == n && relayed(q))
}

/// Gives the engineer's answers to the steps that asked: each asking step that has ended, was
/// judged, and has not run again since, runs again in its session with the answer. Whenever
/// the answer came (while the step ran, after it ended, across a restart), it reaches the
/// agent once. Returns whether a step was queued.
pub fn deliver(run: &mut Run) -> Result<bool, String> {
    let l = lp(run)?;
    if l.unlocked || run.state.stage.is_final() {
        return Ok(false);
    }
    let mut answered: Vec<&QuestionEntry> = l
        .questions
        .values()
        .filter(|q| q.state == QuestionState::Answered && q.by.as_deref() == Some("engineer"))
        .collect();
    answered.sort_by_key(|q| q.seq);
    for q in answered {
        let Some(s) = run.state.steps.get(&q.step).cloned() else {
            continue;
        };
        if s.end.is_none()
            || run.running.contains_key(&s.n)
            || super::machine::first_reason(&s).is_none()
        {
            continue;
        }
        // Nothing later of the same step: same kind, part and agent (two checkers of one story
        // run alongside each other, 008 R-7).
        if run
            .state
            .steps
            .values()
            .any(|x| x.n > s.n && x.part == s.part && x.kind == s.kind && x.def == s.def)
        {
            continue;
        }
        let commit = q.commit.clone().unwrap_or_default();
        let text = format!(
            "{}\n\n(Fallow committed this answer to {} to spec.md's Clarifications, {}.)",
            q.answer.as_deref().unwrap_or_default(),
            q.id,
            &commit[..commit.len().min(7)]
        );
        super::feature::answered(run, &s, &text)?;
        return Ok(true);
    }
    Ok(false)
}

/// After a restart: a question that was with the lead lost its call; it goes to the engineer.
pub fn orphans(run: &mut Run) -> Result<(), String> {
    let Some(l) = run.state.feature_loop.clone() else {
        return Ok(());
    };
    if run.state.stage.is_final() {
        return Ok(());
    }
    for q in l
        .questions
        .values()
        .filter(|q| q.state == QuestionState::WithLead)
    {
        let _ = git::worktree_remove(
            &run.state.repo,
            &run.worktrees().join(format!("lead-{}", q.id)),
        );
        relay(
            run,
            q,
            &Err(String::new()),
            0.0,
            "the lead's call was cut short when Fallow stopped",
            |_| {},
        )?;
    }
    Ok(())
}

/// The engineer confirms a decision the lead took at the Free level (FR-020a, constitution II).
pub fn confirm(run: &mut Run, q: &str) -> Result<Value, String> {
    let l = lp(run)?;
    let entry = l
        .questions
        .get(q)
        .cloned()
        .ok_or("there is no such question")?;
    if run.state.stage.is_final() {
        return Err("the loop has ended".into());
    }
    if entry.state != QuestionState::Decided {
        return Err(format!("{q} is not a decision of the lead"));
    }
    // The answer stays the lead's, with its commit: confirming changes nothing in the spec.
    record(
        run,
        &entry,
        QuestionState::Confirmed,
        entry.by.as_deref(),
        entry.answer.clone(),
        entry.commit.clone(),
        entry.options.clone(),
        entry.recommended.clone(),
        0.0,
        Vec::new(),
        None,
    )?;
    run.append(Event::Steering {
        kind: SteeringKind::Answer,
        target: entry.task.clone(),
        detail: Some(format!("confirmed the lead's decision on {q}")),
    })?;
    Ok(json!({}))
}

/// The engineer overrules an answer the lead gave or a decision it took (FR-022): the reversal
/// is committed, the tasks built on the old answer reopen, and the story is checked again.
pub fn overrule(run: &mut Run, lead: &Lead, q: &str, answer: &str) -> Result<Value, String> {
    let l = lp(run)?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("the answer is empty".into());
    }
    if l.unlocked {
        return Err("the feature is unlocked: lock it again first".into());
    }
    let entry = l
        .questions
        .get(q)
        .cloned()
        .ok_or("there is no such question")?;
    let by_lead = matches!(entry.state, QuestionState::Decided)
        || (entry.state == QuestionState::Answered && entry.by_lead());
    if !by_lead {
        return Err(format!("{q} was not answered by the lead"));
    }
    if run.state.stage.is_final() {
        return Err("the loop is over".into());
    }
    let commit = commit_answer(
        run,
        &l,
        lead,
        &entry,
        "engineer, overruling the lead",
        answer,
    )?;
    let reopened = built_on(run, &l, &entry);
    record(
        run,
        &entry,
        QuestionState::Overruled,
        Some("engineer"),
        Some(answer.to_string()),
        Some(commit.clone()),
        entry.options.clone(),
        entry.recommended.clone(),
        0.0,
        reopened.clone(),
        None,
    )?;
    if run.state.stage == Stage::Ready {
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("the engineer overruled the lead's answer to {q}"),
        })?;
    }
    Ok(json!({ "commit": commit, "reopened": reopened }))
}

/// The tasks built on an answer: the accepted tasks of the asking step's phase, and of the later
/// phases of the same story, with a step at or after the question's (the asking task itself
/// included). T086: a question asked at a phase's start (the test-writer's) had all of its
/// phase's tasks built on it, and none was found.
fn built_on(run: &Run, l: &LoopState, q: &QuestionEntry) -> Vec<String> {
    let phase = run.state.steps.get(&q.step).and_then(|s| s.phase);
    let mut out = Vec::new();
    for p in &l.phases {
        let same_story = q.affects.is_some()
            && (p.spec.story == q.affects || p.spec.tasks.iter().any(|t| t.story == q.affects));
        let later = phase.is_some_and(|n| p.spec.n > n);
        if Some(p.spec.n) != phase && !(later && same_story) {
            continue;
        }
        for t in &p.spec.tasks {
            let accepted = l
                .tasks
                .get(&t.id)
                .is_some_and(|s| s.state == TaskState::Accepted);
            let after = run
                .state
                .steps
                .values()
                .any(|s| s.part.as_deref() == Some(t.id.as_str()) && s.n >= q.step);
            if accepted && after {
                out.push(t.id.clone());
            }
        }
    }
    out
}

/// The question an overrule left to be built on again, and the part its repair works on: the
/// story it affects, or the asking step's phase.
pub fn overruled(l: &LoopState) -> Vec<&QuestionEntry> {
    l.questions
        .values()
        .filter(|q| q.state == QuestionState::Overruled)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(outcome: &str, answer: &str, basis: &str) -> Value {
        json!({ "outcome": outcome, "answer": answer, "basis": basis,
                "reserved": false, "changes_criterion": false,
                "touches_interface": false, "adds_dependency": false })
    }

    #[test]
    fn an_answer_needs_a_basis() {
        let q = "Floats?";
        assert_eq!(
            relay_reason(Level::Normal, &said("answer", "no", "spec.md FR-2"), q, &[]),
            None
        );
        for basis in ["none", "", "  None "] {
            assert!(
                relay_reason(Level::Normal, &said("answer", "no", basis), q, &[]).is_some(),
                "{basis:?}"
            );
        }
        assert!(relay_reason(Level::Normal, &said("answer", " ", "spec.md"), q, &[]).is_some());
    }

    #[test]
    fn decided_only_at_free() {
        let v = said("decided", "no floats", "none");
        assert_eq!(relay_reason(Level::Free, &v, "Floats?", &[]), None);
        // Only a small question (FR-020a): no public interface, no dependency; a missing flag
        // counts as set.
        for flag in ["touches_interface", "adds_dependency"] {
            let mut w = v.clone();
            w[flag] = json!(true);
            assert!(
                relay_reason(Level::Free, &w, "Floats?", &[]).is_some(),
                "{flag}"
            );
            let mut w = v.clone();
            w.as_object_mut().unwrap().remove(flag);
            assert!(
                relay_reason(Level::Free, &w, "Floats?", &[]).is_some(),
                "{flag} missing"
            );
        }
        assert!(relay_reason(Level::Normal, &v, "Floats?", &[]).is_some());
        assert!(relay_reason(Level::Strict, &v, "Floats?", &[]).is_some());
    }

    #[test]
    fn strict_reserved_items_and_criteria_are_always_relayed() {
        let ok = said("answer", "no", "spec.md US1");
        assert!(relay_reason(Level::Strict, &ok, "Floats?", &[]).is_some());
        for level in [Level::Normal, Level::Free] {
            let mut v = ok.clone();
            v["reserved"] = json!(true);
            assert!(relay_reason(level, &v, "Floats?", &[]).is_some());
            let mut v = ok.clone();
            v["changes_criterion"] = json!(true);
            assert!(relay_reason(level, &v, "Floats?", &[]).is_some());
            // The lead's flags are claims: Fallow finds a reserved item in the question or the
            // answer itself, and a missing flag counts as set.
            let why = relay_reason(
                level,
                &ok,
                "What should the Refusal Wording be?",
                &["the refusal wording".into()],
            );
            assert!(why.unwrap().contains("reserved"));
            let why = relay_reason(
                level,
                &said("answer", "use EUR prices", "spec.md"),
                "Currency?",
                &["EUR prices".into()],
            );
            assert!(why.is_some());
            let mut v = ok.clone();
            v.as_object_mut().unwrap().remove("reserved");
            assert!(relay_reason(level, &v, "Floats?", &[]).is_some());
        }
    }

    #[test]
    fn relay_and_nonsense_are_relayed() {
        assert!(relay_reason(Level::Free, &said("relay", "", "none"), "q", &[]).is_some());
        assert!(relay_reason(Level::Free, &said("maybe", "x", "spec.md"), "q", &[]).is_some());
    }
}
