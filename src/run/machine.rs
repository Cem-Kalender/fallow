//! The stage machine (006 tasks T026, T027, T031 to T034): what a run does next, decided only
//! from its folded state, so a restarted Fallow carries on from the log alone (constitution
//! VII). Agent steps, gates and merges run on threads and come back through `on_done`.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::engine::{Busy, Ctx, Done, Msg, Run};
use super::events::*;
use crate::agent::{self, Agent, Call, Session, prompts};
use crate::config::{Failures, GateDef};
use crate::plan::{self, Build};
use crate::verify::{claims, gates, git, verdict};

pub fn plan_branch(id: u64) -> String {
    format!("fallow/r{id}/plan")
}

pub fn part_branch(id: u64, part: &str) -> String {
    format!("fallow/r{id}/{part}")
}

/// The integrated result. Not `fallow/r<id>` itself: git cannot hold a branch and branches
/// below its name at once.
pub fn integration_branch(id: u64) -> String {
    format!("fallow/r{id}/result")
}

/// How many times in a row a review may fail or be incomplete before the engineer is asked.
const REVIEW_TRIES: usize = 3;

fn gate_defs(run: &Run) -> Vec<GateDef> {
    run.state
        .gates
        .iter()
        .map(|g| GateDef {
            name: g.name.clone(),
            command: g.command.clone(),
            timeout_min: g.timeout_min,
            baseline: g.baseline,
            failures: match g.failures.as_str() {
                "auto" => Failures::Auto,
                "exit-code" => Failures::ExitCode,
                p => Failures::Regex(p.to_string()),
            },
        })
        .collect()
}

fn spec_kit(run: &Run) -> bool {
    run.state.repo.join(".specify").is_dir()
}

/// Reads the approved plan when it is not in memory yet.
fn load_plan(run: &mut Run) -> Result<(), String> {
    if run.plan.is_some() {
        return Ok(());
    }
    let Some(commit) = run.state.approved_plan.clone() else {
        return Ok(());
    };
    match plan::at(&run.state.repo, &commit) {
        Ok(Some(p)) => {
            run.plan = Some(p);
            Ok(())
        }
        Ok(None) => Err(format!("the approved plan commit {commit} has no plan")),
        Err(e) => Err(e.join("; ")),
    }
}

/// Writes a step's instruction and queues it.
fn queue(
    run: &mut Run,
    kind: StepKind,
    part: Option<&str>,
    resume: Option<String>,
    reason: &str,
    instruction: String,
) -> Result<u32, String> {
    let n = run.state.next_step();
    let dir = run.steps_dir();
    crate::platform::private_dir(&dir)?;
    std::fs::write(dir.join(format!("{n}.prompt")), &instruction)
        .map_err(|e| format!("cannot write step {n}'s instruction: {e}"))?;
    run.append(Event::StepQueued {
        n,
        kind,
        part: part.map(str::to_string),
        resume,
        reason: reason.to_string(),
    })?;
    Ok(n)
}

fn instruction_of(run: &Run, n: u32) -> String {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.prompt"))).unwrap_or_default()
}

/// The first planning turn: the plan branch and its worktree, and the planner's first draft.
pub fn start_planning(run: &mut Run) -> Result<(), String> {
    let id = run.id();
    let repo = run.state.repo.clone();
    git::create_branch(&repo, &plan_branch(id), &run.state.start_commit)?;
    let wt = run.worktrees().join("plan");
    git::worktree_add(&repo, &wt, Some(&plan_branch(id)), &run.state.start_commit)?;
    let cx = prompts::Context {
        task: &run.state.task,
        plan: None,
        gates: &run.state.gates,
        spec_kit: spec_kit(run),
    };
    let text = prompts::plan_turn(&cx, "", true);
    queue(
        run,
        StepKind::PlanTurn,
        None,
        None,
        "the planner drafts the plan",
        text,
    )?;
    Ok(())
}

/// After a restart: steps that were running when Fallow stopped are resumed in their sessions
/// (research R-2); nothing completed runs again.
pub fn resume(run: &mut Run) -> Result<(), String> {
    let running: Vec<(u32, StepKind, Option<String>, Option<String>)> = run
        .state
        .steps
        .values()
        .filter(|s| s.running())
        .map(|s| (s.n, s.kind, s.part.clone(), s.session.clone()))
        .collect();
    for (n, kind, part, session) in running {
        run.append(Event::StepFinished {
            n,
            end: StepEnd::Failed {
                why: "interrupted: Fallow stopped while the step ran".into(),
            },
            cost_usd: None,
            duration_ms: 0,
            denials: Vec::new(),
        })?;
        run.append(Event::Verified {
            n,
            accepted: false,
            reasons: vec!["interrupted; resumed after Fallow started again".into()],
            commit: None,
        })?;
        let text = match &session {
            Some(_) => format!(
                "Fallow step: {} {}\n\n{}",
                kind.label(),
                part.as_deref()
                    .map(|p| format!("(part {p})"))
                    .unwrap_or_default(),
                prompts::INTERRUPTED
            ),
            None => instruction_of(run, n),
        };
        queue(
            run,
            kind,
            part.as_deref(),
            session,
            "resumed after Fallow stopped",
            text,
        )?;
    }
    Ok(())
}

/// Whether a limit stops the next agent step (FR-038).
fn limit_hit(run: &Run) -> Option<(String, String)> {
    let s = &run.state;
    if s.cost_usd >= s.limits.usd {
        return Some((
            "cost".into(),
            format!("${:.2} of ${:.2}", s.cost_usd, s.limits.usd),
        ));
    }
    let used = s.minutes_used(super::time::unix_now());
    if s.building_started.is_some() && used >= s.limits.minutes {
        return Some(("time".into(), format!("{used} of {} min", s.limits.minutes)));
    }
    None
}

fn pause_for_limit(run: &mut Run, which: String, used: String) -> Result<(), String> {
    run.append(Event::LimitReached {
        which: which.clone(),
        used: used.clone(),
    })?;
    run.append(Event::Stage {
        to: Stage::Paused,
        why: format!("the {which} limit is reached ({used})"),
    })?;
    run.append(Event::NeedsEngineer {
        why: format!(
            "The run's {which} limit is reached ({used}). Raise the limit to go on, or stop the run."
        ),
        options: vec!["raise the limit".into(), "stop".into()],
    })
}

/// Decides and starts what comes next. Does nothing while something runs.
pub fn drive(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    if run.busy.is_some() || run.state.stage.is_final() {
        return Ok(());
    }
    match run.state.stage {
        Stage::Planning => {
            if run.state.needs.is_some() {
                return Ok(());
            }
            if let Some(n) = unprocessed(run) {
                return process(run, n);
            }
            if let Some(n) = queued(run) {
                if let Some((w, u)) = limit_hit(run) {
                    return pause_for_limit(run, w, u);
                }
                return start(run, ctx, n);
            }
            Ok(())
        }
        Stage::Building => building(run, ctx),
        Stage::Reviewing => reviewing(run, ctx),
        _ => Ok(()),
    }
}

/// The oldest queued step.
fn queued(run: &Run) -> Option<u32> {
    run.state.steps.values().find(|s| s.queued()).map(|s| s.n)
}

/// A finished step whose result has not been handled (after a restart).
fn unprocessed(run: &Run) -> Option<u32> {
    run.state
        .steps
        .values()
        .find(|s| s.end.is_some() && s.verified.is_none())
        .map(|s| s.n)
}

fn building(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    load_plan(run)?;
    if run.state.needs.is_some() {
        return Ok(());
    }
    if let Some(n) = unprocessed(run) {
        return process(run, n);
    }
    if let Some(n) = queued(run) {
        if let Some((w, u)) = limit_hit(run) {
            return pause_for_limit(run, w, u);
        }
        return start(run, ctx, n);
    }
    // The baseline, once, before anything is judged against it.
    let missing_baseline: Vec<GateDef> = gate_defs(run)
        .into_iter()
        .filter(|g| g.baseline && run.state.baseline(&g.name).is_none())
        .collect();
    if !missing_baseline.is_empty() {
        let commit = run.state.start_commit.clone();
        run_gates(run, ctx, GateKind::Baseline, None, commit, missing_baseline);
        return Ok(());
    }
    let Some(plan) = run.plan.clone() else {
        return Err("the run is building without an approved plan".into());
    };
    if plan.build == Build::Parts {
        return run.append(Event::NeedsEngineer {
            why: "This plan splits the work into parallel parts, which arrives with user story 5. Change the plan to one builder, or stop the run.".into(),
            options: vec!["stop".into()],
        });
    }
    for p in &plan.parts {
        let status = run.state.parts.get(&p.id).cloned();
        match status.as_ref().map(|s| s.state) {
            None | Some(PartState::Waiting) => {
                run.append(Event::Part {
                    id: p.id.clone(),
                    state: PartState::Building,
                    result_commit: None,
                    repairs: 0,
                })?;
                let branch = part_branch(run.id(), &p.id);
                let cx = prompts::Context {
                    task: &run.state.task,
                    plan: Some(&plan),
                    gates: &run.state.gates,
                    spec_kit: spec_kit(run),
                };
                let text = prompts::build(&cx, &p.id, &branch);
                let n = queue(
                    run,
                    StepKind::Build,
                    Some(&p.id),
                    None,
                    "build the part",
                    text,
                )?;
                if let Some((w, u)) = limit_hit(run) {
                    return pause_for_limit(run, w, u);
                }
                return start(run, ctx, n);
            }
            Some(PartState::Building | PartState::Repairing | PartState::Reopened) => {
                // The latest accepted result still owes its gates.
                let last = run
                    .state
                    .steps
                    .values()
                    .rev()
                    .find(|s| s.part.as_deref() == Some(p.id.as_str()) && s.end.is_some());
                let accepted = last.and_then(|s| match &s.verified {
                    Some((true, _)) => s.verified_commit.clone(),
                    _ => None,
                });
                if let Some(c) = accepted {
                    let defs = gate_defs(run);
                    if defs.is_empty() {
                        let repairs = status.map_or(0, |s| s.repairs);
                        run.append(Event::Part {
                            id: p.id.clone(),
                            state: PartState::Done,
                            result_commit: Some(c),
                            repairs,
                        })?;
                        continue;
                    }
                    let ran = run.state.gate_runs.iter().any(|g| {
                        g.kind == GateKind::Part
                            && g.part.as_deref() == Some(p.id.as_str())
                            && g.result.commit == c
                    });
                    if !ran {
                        run_gates(run, ctx, GateKind::Part, Some(p.id.clone()), c, defs);
                    }
                }
                return Ok(());
            }
            Some(PartState::Done) => continue,
            Some(_) => return Ok(()),
        }
    }
    // Every part is done: integrate. With one builder the integrated result is its commit.
    let main = plan
        .parts
        .first()
        .and_then(|p| run.state.parts.get(&p.id))
        .and_then(|s| s.result_commit.clone())
        .ok_or("the part is done without a commit")?;
    git::set_branch(&run.state.repo, &integration_branch(run.id()), &main)?;
    run.append(Event::Integrated { commit: main })?;
    run.append(Event::Stage {
        to: Stage::Reviewing,
        why: "the work is verified and integrated".into(),
    })?;
    reviewing(run, ctx)
}

fn reviewing(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    load_plan(run)?;
    if run.state.needs.is_some() {
        return Ok(());
    }
    if let Some(n) = unprocessed(run) {
        return process(run, n);
    }
    if let Some(n) = queued(run) {
        if let Some((w, u)) = limit_hit(run) {
            return pause_for_limit(run, w, u);
        }
        return start(run, ctx, n);
    }
    let round = run.state.integrated_seq;
    let reviewed = run
        .state
        .steps
        .values()
        .any(|s| s.kind == StepKind::Review && s.queued_seq > round);
    if !reviewed {
        let n = queue_fresh_review(run)?;
        if let Some((w, u)) = limit_hit(run) {
            return pause_for_limit(run, w, u);
        }
        return start(run, ctx, n);
    }
    Ok(())
}

fn queue_fresh_review(run: &mut Run) -> Result<u32, String> {
    let commit = run
        .state
        .integrated
        .clone()
        .ok_or("nothing is integrated")?;
    let cx = prompts::Context {
        task: &run.state.task,
        plan: run.plan.as_ref(),
        gates: &run.state.gates,
        spec_kit: false,
    };
    let text = prompts::review(&cx, &commit);
    queue(
        run,
        StepKind::Review,
        None,
        None,
        "review the integrated result",
        text,
    )
}

/// Starts step `n`: its worktree, its call, and a thread that runs it.
fn start(run: &mut Run, ctx: &Ctx, n: u32) -> Result<(), String> {
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    let id = run.id();
    let repo = run.state.repo.clone();
    let role = step.kind.role();
    let (worktree, base) = match step.kind {
        StepKind::PlanTurn | StepKind::Decide => {
            let wt = run.worktrees().join("plan");
            git::worktree_add(&repo, &wt, Some(&plan_branch(id)), &run.state.start_commit)?;
            let base = git::head(&wt)?;
            (wt, base)
        }
        StepKind::Build | StepKind::Repair | StepKind::Take | StepKind::Plain => {
            let part = step.part.clone().unwrap_or_else(|| plan::MAIN.to_string());
            let branch = part_branch(id, &part);
            let from = run
                .state
                .approved_plan
                .clone()
                .unwrap_or_else(|| run.state.start_commit.clone());
            git::create_branch(&repo, &branch, &from)?;
            let wt = run.worktrees().join(&part);
            git::worktree_add(&repo, &wt, Some(&branch), &from)?;
            let base = git::head(&wt)?;
            (wt, base)
        }
        StepKind::Review => {
            let commit = run.state.integrated.clone().ok_or("nothing to review")?;
            let wt = run.worktrees().join(format!("review-{n}"));
            if step.resume.is_none() {
                let _ = git::worktree_remove(&repo, &wt);
            }
            // A resumed review keeps its worktree, so its earlier checks still apply.
            let prev = step.resume.as_ref().and_then(|_| {
                run.state
                    .steps
                    .values()
                    .rev()
                    .find(|s| s.kind == StepKind::Review && s.n < n && s.worktree.is_some())
                    .and_then(|s| s.worktree.clone())
            });
            let wt = prev.filter(|p| p.join(".git").exists()).unwrap_or(wt);
            git::worktree_add(&repo, &wt, None, &commit)?;
            (wt, commit)
        }
    };
    let choice = run.state.roles.get(role).cloned().unwrap_or_default();
    let def = ctx
        .settings
        .agents
        .get(&choice.agent)
        .cloned()
        .ok_or_else(|| format!("no agent named {:?} in the settings", choice.agent))?;
    let extra: Vec<String> = run.state.allow.get(role).cloned().unwrap_or_default();
    let allowed = match role {
        "planner" => agent::allow::planner(&extra),
        "reviewer" => agent::allow::reviewer(&run.state.gates, &extra),
        _ => agent::allow::builder(&run.state.gates, &extra),
    };
    let session = match &step.resume {
        Some(s) => Session::Resume(s.clone()),
        None => Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?),
    };
    let remaining = (run.state.limits.usd - run.state.cost_usd).max(0.01);
    let call = Call {
        agent: Agent {
            command: def.command.clone(),
            args: def.args.clone(),
        },
        instruction: instruction_of(run, n),
        schema: agent::schemas::for_kind(step.kind),
        allowed,
        mcp_config: None,
        session: session.clone(),
        model: Some(choice.model.clone()).filter(|m| !m.is_empty()),
        budget_usd: Some(remaining),
        cwd: worktree.clone(),
        time_limit: Duration::from_secs(run.state.limits.step_minutes * 60),
        system: Some(prompts::system(role)),
        unrestricted: false,
    };
    run.append(Event::StepStarted {
        n,
        agent: choice.agent.clone(),
        model: choice.model.clone(),
        session: session.id().to_string(),
        worktree,
        base,
    })?;
    run.busy = Some(Busy::Step(n));
    run.stop = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&run.stop);
    let tx = ctx.tx.clone();
    let activity_path = run.steps_dir().join(format!("{n}.jsonl"));
    let run_id = id;
    std::thread::Builder::new()
        .name(format!("step-{run_id}-{n}"))
        .spawn(move || {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&activity_path)
                .ok();
            let tx2 = tx.clone();
            let mut on = |a: agent::Activity| {
                if let Some(f) = file.as_mut() {
                    if let Ok(line) = serde_json::to_string(&a) {
                        let _ = writeln!(f, "{line}");
                    }
                }
                let _ = tx2.send(Msg::Activity {
                    run: run_id,
                    n,
                    line: a,
                });
            };
            let outcome = agent::claude::run(&call, &mut on, &stop);
            let _ = tx.send(Msg::Done {
                run: run_id,
                done: Done::Step { n, outcome },
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// Runs gates on `commit` in a temporary worktree, on a thread.
fn run_gates(
    run: &mut Run,
    ctx: &Ctx,
    kind: GateKind,
    part: Option<String>,
    commit: String,
    defs: Vec<GateDef>,
) {
    run.busy = Some(Busy::Gates);
    let repo = run.state.repo.clone();
    let wt = run.worktrees().join("gate");
    let tx = ctx.tx.clone();
    let id = run.id();
    let _ = std::thread::Builder::new()
        .name(format!("gates-{id}"))
        .spawn(move || {
            let _ = git::worktree_remove(&repo, &wt);
            let mut results = Vec::new();
            match git::worktree_add(&repo, &wt, None, &commit) {
                Ok(()) => {
                    for d in &defs {
                        let mut ignore = |_: &str| {};
                        results.push(gates::run(d, &wt, &commit, &mut ignore));
                    }
                }
                Err(e) => {
                    for d in &defs {
                        results.push(gates::GateResult {
                            gate: d.name.clone(),
                            commit: commit.clone(),
                            outcome: gates::Outcome::NotStarted { why: e.clone() },
                            failing: Default::default(),
                        });
                    }
                }
            }
            let _ = git::worktree_remove(&repo, &wt);
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Gates {
                    kind,
                    part,
                    results,
                },
            });
        });
}

/// Handles what a worker reports.
pub fn on_done(run: &mut Run, _ctx: &Ctx, done: Done) -> Result<(), String> {
    match done {
        Done::Step { n, outcome } => {
            let end = match (&outcome.result, &outcome.failure) {
                (Some(v), None) => StepEnd::Result { value: v.clone() },
                (_, Some(why)) => StepEnd::Failed { why: why.clone() },
                (None, None) => StepEnd::Failed {
                    why: "no result".into(),
                },
            };
            run.append(Event::StepFinished {
                n,
                end,
                cost_usd: outcome.cost_usd,
                duration_ms: outcome.duration.as_millis() as u64,
                denials: outcome.denials,
            })?;
            if run.state.stage.is_final() {
                return run.append(Event::Verified {
                    n,
                    accepted: false,
                    reasons: vec!["the run was stopped".into()],
                    commit: None,
                });
            }
            process(run, n)
        }
        Done::Gates {
            kind,
            part,
            results,
        } => {
            let mut failed = Vec::new();
            for r in results {
                let class = if kind == GateKind::Baseline {
                    None
                } else {
                    let c = gates::classify(&r, run.state.baseline(&r.gate));
                    if !c.ok() {
                        let names: Vec<&str> =
                            r.failing.iter().map(String::as_str).take(8).collect();
                        failed.push(format!(
                            "gate {} {} ({}){}",
                            r.gate,
                            r.outcome.label(),
                            c.label(),
                            if names.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", names.join(", "))
                            }
                        ));
                    }
                    Some(c)
                };
                run.append(Event::GateRun {
                    kind,
                    part: part.clone(),
                    result: r,
                    class,
                })?;
            }
            if kind == GateKind::Part {
                let p = part.unwrap_or_else(|| plan::MAIN.to_string());
                let status = run.state.parts.get(&p).cloned();
                let commit = run.state.gate_runs.last().map(|g| g.result.commit.clone());
                if failed.is_empty() {
                    run.append(Event::Part {
                        id: p,
                        state: PartState::Done,
                        result_commit: commit,
                        repairs: status.map_or(0, |s| s.repairs),
                    })?;
                } else {
                    reject(run, &p, failed)?;
                }
            }
            Ok(())
        }
        Done::Merge(r) => match r {
            Ok(commit) => {
                run.append(Event::Merged {
                    commit,
                    how: "local".into(),
                })?;
                run.append(Event::Stage {
                    to: Stage::Done,
                    why: "merged by the engineer".into(),
                })
            }
            Err(e) => run.append(Event::NeedsEngineer {
                why: format!("The merge did not go through: {e}"),
                options: vec!["merge again".into()],
            }),
        },
    }
}

/// Handles a finished step's result; ends with a `verified` event, which marks it handled.
fn process(run: &mut Run, n: u32) -> Result<(), String> {
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    match step.kind {
        StepKind::PlanTurn | StepKind::Decide => plan_turn_done(run, &step),
        StepKind::Build | StepKind::Repair | StepKind::Take | StepKind::Plain => {
            build_done(run, &step)
        }
        StepKind::Review => review_done(run, &step),
    }
}

fn plan_turn_done(run: &mut Run, step: &super::state::Step) -> Result<(), String> {
    let n = step.n;
    let id = run.id();
    let repo = run.state.repo.clone();
    let (reply, ready) = match &step.end {
        Some(StepEnd::Result { value }) => (
            value["reply"].as_str().unwrap_or_default().to_string(),
            value["ready"].as_bool().unwrap_or(false),
        ),
        Some(StepEnd::Failed { why }) => (
            format!("(The planner's turn did not finish: {why}. Say something to try again.)"),
            false,
        ),
        None => return Ok(()),
    };
    run.append(Event::PlanReply {
        step: n,
        reply,
        ready,
    })?;
    // Fallow commits the plan's files the planner changed, and nothing else (research R-4).
    if let Some(wt) = &step.worktree {
        let msg = format!("Plan (Fallow run r{id}, planning turn {n})");
        let paths: Vec<&str> = crate::agent::allow::PLAN_DIRS.to_vec();
        if let Err(e) = git::commit_all(wt, &msg, Some(&paths)) {
            run.append(Event::Note {
                text: format!("Fallow could not commit the plan: {e}"),
            })?;
        }
        let _ = git::discard_changes(wt);
    }
    // The plan is what the plan branch holds now.
    let tip = git::rev(&repo, &plan_branch(id))?;
    let changed = tip != run.state.start_commit && Some(&tip) != run.state.plan_commit.as_ref();
    if changed {
        let mut problems = Vec::new();
        if let Err(why) =
            claims::check_plan_commit(&repo, &plan_branch(id), &run.state.start_commit, &tip)
        {
            problems.extend(why);
        }
        match plan::at(&repo, &tip) {
            Ok(Some(p)) => problems.extend(plan::approval_problems(
                &repo,
                &p,
                &tip,
                &run.state.test_patterns,
            )),
            Ok(None) => problems.push(format!(
                "there is no {} at the plan commit",
                plan::PLAN_FILE
            )),
            Err(e) => problems.extend(e),
        }
        run.append(Event::PlanCommitted {
            commit: tip,
            approvable: problems.is_empty(),
            problems,
        })?;
    }
    run.append(Event::Verified {
        n,
        accepted: true,
        reasons: Vec::new(),
        commit: None,
    })
}

fn build_done(run: &mut Run, step: &super::state::Step) -> Result<(), String> {
    let n = step.n;
    let part = step.part.clone().unwrap_or_else(|| plan::MAIN.to_string());
    let value = match &step.end {
        Some(StepEnd::Result { value }) => value.clone(),
        Some(StepEnd::Failed { why }) => return reject_step(run, n, &part, vec![why.clone()]),
        None => return Ok(()),
    };
    let summary = value["summary"].as_str().unwrap_or_default().to_string();
    match value["status"].as_str() {
        Some("blocked") => {
            let mut q = value["question"].as_str().unwrap_or("").to_string();
            if q.trim().is_empty() {
                q = summary;
            }
            if !step.denials.is_empty() {
                q = format!(
                    "{q} (commands its permissions refused: {})",
                    step.denials.join(", ")
                );
            }
            let repairs = run.state.parts.get(&part).map_or(0, |s| s.repairs);
            run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![format!("blocked: {q}")],
                commit: None,
            })?;
            run.append(Event::Part {
                id: part.clone(),
                state: PartState::Blocked,
                result_commit: None,
                repairs,
            })?;
            run.append(Event::NeedsEngineer {
                why: format!("The builder of part {part} asks: {q}"),
                options: vec!["answer".into(), "stop".into()],
            })
        }
        Some("done") => {
            load_plan(run)?;
            let plan = run.plan.clone().ok_or("no approved plan")?;
            // Fallow commits what the step changed (research R-4): agents need no git rights.
            if let Some(wt) = &step.worktree {
                let msg = format!(
                    "{} {part}: {}

Fallow run r{}, step {n}",
                    step.kind.label(),
                    first_line(&summary, "work"),
                    run.id()
                );
                if let Err(e) = git::commit_all(wt, &msg, None) {
                    return reject_step(
                        run,
                        n,
                        &part,
                        vec![format!("Fallow could not commit the step's changes: {e}")],
                    );
                }
            }
            let takes: Vec<String> = Vec::new();
            let base = step.base.clone().unwrap_or_default();
            match claims::check_build(
                &run.state.repo,
                &part_branch(run.id(), &part),
                &base,
                &value,
                &plan,
                &part,
                &takes,
            ) {
                Ok(commit) => run.append(Event::Verified {
                    n,
                    accepted: true,
                    reasons: Vec::new(),
                    commit: Some(commit),
                }),
                Err(why) => reject_step(run, n, &part, why),
            }
        }
        _ => reject_step(
            run,
            n,
            &part,
            vec![format!(
                "the builder reported that it failed{}",
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {summary}")
                }
            )],
        ),
    }
}

fn reject_step(run: &mut Run, n: u32, part: &str, reasons: Vec<String>) -> Result<(), String> {
    run.append(Event::Verified {
        n,
        accepted: false,
        reasons: reasons.clone(),
        commit: None,
    })?;
    reject(run, part, reasons)
}

/// A part's work was not accepted: a repair within the cap, else the engineer (FR-008).
fn reject(run: &mut Run, part: &str, reasons: Vec<String>) -> Result<(), String> {
    let repairs = run.state.parts.get(part).map_or(0, |s| s.repairs) + 1;
    if repairs > run.state.repair_cap {
        run.append(Event::Part {
            id: part.to_string(),
            state: PartState::Blocked,
            result_commit: None,
            repairs: repairs - 1,
        })?;
        return run.append(Event::NeedsEngineer {
            why: format!(
                "Part {part} still fails after {} repair round(s): {}. Allow one more repair, or stop the run.",
                repairs - 1,
                reasons.join("; ")
            ),
            options: vec!["allow one more repair".into(), "stop".into()],
        });
    }
    run.append(Event::Part {
        id: part.to_string(),
        state: PartState::Repairing,
        result_commit: None,
        repairs,
    })?;
    let session = run.state.session_of("builder", Some(part));
    let text = prompts::repair(part, &reasons, &part_branch(run.id(), part));
    queue(
        run,
        StepKind::Repair,
        Some(part),
        session,
        &format!("repair round {repairs} of {}", run.state.repair_cap),
        text,
    )?;
    if run.state.stage != Stage::Building {
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("part {part} needs a repair"),
        })?;
    }
    Ok(())
}

fn review_done(run: &mut Run, step: &super::state::Step) -> Result<(), String> {
    let n = step.n;
    let round = run.state.integrated_seq;
    let tries = run
        .state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Review && s.queued_seq > round && s.n <= n)
        .count();
    let value = match &step.end {
        Some(StepEnd::Result { value }) => value.clone(),
        Some(StepEnd::Failed { why }) => {
            run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![why.clone()],
                commit: None,
            })?;
            return retry_review(
                run,
                tries,
                None,
                &format!("the review did not finish: {why}"),
            );
        }
        None => return Ok(()),
    };
    let integrated = run.state.integrated.clone().unwrap_or_default();
    if let Some(wt) = &step.worktree {
        if !git::unchanged(wt, &integrated).unwrap_or(false) {
            run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![
                    "the reviewer changed its worktree, so its verdict does not count".into(),
                ],
                commit: None,
            })?;
            let _ = git::worktree_remove(&run.state.repo, wt);
            return retry_review(run, tries, None, "the reviewer changed files");
        }
    }
    load_plan(run)?;
    let plan = run.plan.clone().ok_or("no approved plan")?;
    let expected: Vec<(String, bool)> = plan
        .criteria()
        .into_iter()
        .map(|(k, c)| (k, c.must_fail))
        .collect();
    // Every partial verdict of this review round adds up (FR-007).
    let mut all: Vec<Answer> = run
        .state
        .verdicts
        .iter()
        .filter(|v| {
            run.state
                .steps
                .get(&v.step)
                .is_some_and(|s| s.queued_seq > round)
        })
        .flat_map(|v| v.answers.clone())
        .collect();
    let mine = verdict::answers(&value);
    all.extend(mine.iter().cloned());
    let mut check = verdict::check(&all, &expected);
    // A review whose commands were refused did not check by running: it does not count, and the
    // reviewer is resumed to run them one at a time (found on a real run, 2026-10-29).
    if !step.denials.is_empty() {
        check.complete = false;
        check.approved = false;
        check.missing.push(format!(
            "checks by running: these commands were refused, run each one on its own: {}",
            step.denials.join(" | ")
        ));
    }
    run.append(Event::Verdict {
        step: n,
        answers: mine,
        complete: check.complete,
        approved: check.approved,
        missing: check.missing.clone(),
    })?;
    run.append(Event::Verified {
        n,
        accepted: true,
        reasons: Vec::new(),
        commit: None,
    })?;
    if check.approved {
        return run.append(Event::Stage {
            to: Stage::Ready,
            why: "the review approved every criterion".into(),
        });
    }
    if check.complete {
        let mut by_part: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for key in &check.failed {
            let part = key.split('/').next().unwrap_or(plan::MAIN).to_string();
            let what = all
                .iter()
                .rev()
                .find(|a| &a.criterion == key)
                .map(|a| format!("{key} failed review: ran {}; got {}", a.ran, a.result))
                .unwrap_or_else(|| format!("{key} failed review"));
            by_part.entry(part).or_default().push(what);
        }
        for (part, reasons) in by_part {
            reject(run, &part, reasons)?;
        }
        return Ok(());
    }
    let why = format!("the verdict is incomplete: {}", check.missing.join("; "));
    match step.session.clone() {
        Some(session) if tries < REVIEW_TRIES => {
            queue(
                run,
                StepKind::Review,
                None,
                Some(session),
                &why,
                prompts::review_missing(&check.missing),
            )?;
            Ok(())
        }
        _ => retry_review(run, tries, None, &why),
    }
}

/// Tries the review again with a fresh reviewer, up to `REVIEW_TRIES` times a round.
fn retry_review(
    run: &mut Run,
    tries: usize,
    _session: Option<String>,
    why: &str,
) -> Result<(), String> {
    if tries >= REVIEW_TRIES {
        return run.append(Event::NeedsEngineer {
            why: format!("The review did not give a usable verdict after {tries} tries: {why}."),
            options: vec!["stop".into()],
        });
    }
    queue_fresh_review(run)?;
    Ok(())
}

/// The engineer's turn in the planning conversation (T026).
pub fn say(run: &mut Run, text: &str) -> Result<(), String> {
    if run.state.stage != Stage::Planning {
        return Err("the plan is already approved".into());
    }
    if run.busy.is_some() || queued(run).is_some() {
        return Err("the planner is still answering".into());
    }
    if text.trim().is_empty() {
        return Err("say something".into());
    }
    run.append(Event::PlanSaid {
        text: text.trim().to_string(),
    })?;
    let first = !run
        .state
        .steps
        .values()
        .any(|s| s.kind == StepKind::PlanTurn && s.session.is_some());
    let cx = prompts::Context {
        task: &run.state.task,
        plan: None,
        gates: &run.state.gates,
        spec_kit: spec_kit(run),
    };
    let instruction = prompts::plan_turn(&cx, text, first);
    let session = run.state.session_of("planner", None);
    queue(
        run,
        StepKind::PlanTurn,
        None,
        session,
        "the engineer answered",
        instruction,
    )?;
    Ok(())
}

/// Approving the plan (FR-003): only a committed, approvable plan at the plan branch's tip.
pub fn approve(run: &mut Run) -> Result<(), String> {
    if run.state.stage != Stage::Planning {
        return Err("the plan is already approved".into());
    }
    if run.busy.is_some() {
        return Err("the planner is still answering".into());
    }
    let Some(commit) = run.state.plan_commit.clone() else {
        return Err("there is no committed plan yet".into());
    };
    if !run.state.plan_approvable {
        return Err(format!(
            "the plan has problems: {}",
            run.state.plan_problems.join("; ")
        ));
    }
    let tip = git::rev(&run.state.repo, &plan_branch(run.id()))?;
    if tip != commit {
        return Err(
            "the plan branch moved since the plan was checked; ask the planner again".into(),
        );
    }
    run.append(Event::Steering {
        kind: SteeringKind::ApprovePlan,
        target: None,
        detail: Some(commit.clone()),
    })?;
    run.append(Event::PlanApproved { commit })?;
    run.plan = None;
    run.append(Event::Stage {
        to: Stage::Building,
        why: "the engineer approved the plan".into(),
    })
}

/// The engineer answers a builder's question: the part's session continues with the answer.
pub fn answer(run: &mut Run, text: &str) -> Result<(), String> {
    let blocked: Vec<String> = run
        .state
        .parts
        .values()
        .filter(|p| p.state == PartState::Blocked)
        .map(|p| p.id.clone())
        .collect();
    let Some(part) = blocked.first().cloned() else {
        return Err("no builder is waiting for an answer".into());
    };
    if run
        .state
        .needs
        .as_ref()
        .is_some_and(|n| n.options.iter().any(|o| o == "allow one more repair"))
    {
        return Err("this part reached its repair cap: allow one more repair instead".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::Answer,
        target: Some(part.clone()),
        detail: Some(text.to_string()),
    })?;
    let repairs = run.state.parts.get(&part).map_or(0, |s| s.repairs);
    run.append(Event::Part {
        id: part.clone(),
        state: PartState::Building,
        result_commit: None,
        repairs,
    })?;
    let session = run.state.session_of("builder", Some(&part));
    let instruction = format!(
        "Fallow step: build (part {part})\n\nThe engineer answers your question:\n\n{text}\n\nContinue in your worktree (branch `{}`) and return your result; Fallow commits your changes when the step ends.",
        part_branch(run.id(), &part)
    );
    queue(
        run,
        StepKind::Build,
        Some(&part),
        session,
        "the engineer answered",
        instruction,
    )?;
    run.append(Event::Stage {
        to: if run.state.stage == Stage::Paused {
            run.state.paused_from.unwrap_or(Stage::Building)
        } else {
            Stage::Building
        },
        why: "the engineer answered the builder".into(),
    })
}

/// The engineer raises the limits or the repair cap (FR-038, FR-008).
pub fn raise_limits(
    run: &mut Run,
    usd: Option<f64>,
    minutes: Option<u64>,
    repair_cap: Option<u32>,
) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if usd.is_none() && minutes.is_none() && repair_cap.is_none() {
        return Err("nothing to change".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::SetLimits,
        target: None,
        detail: Some(format!(
            "usd={} min={} repair_cap={}",
            usd.map_or("-".into(), |u| format!("{u:.2}")),
            minutes.map_or("-".into(), |m| m.to_string()),
            repair_cap.map_or("-".into(), |r| r.to_string())
        )),
    })?;
    run.append(Event::LimitsSet {
        usd,
        minutes,
        repair_cap,
    })?;
    // A part held at its repair cap gets its repair now.
    if repair_cap.is_some() {
        let held: Vec<(String, u32)> = run
            .state
            .parts
            .values()
            .filter(|p| p.state == PartState::Blocked && p.repairs >= 1)
            .map(|p| (p.id.clone(), p.repairs))
            .collect();
        for (part, _) in held {
            let reasons = run
                .state
                .steps
                .values()
                .rev()
                .filter(|s| s.part.as_deref() == Some(part.as_str()))
                .find_map(|s| match &s.verified {
                    Some((false, r)) if !r.is_empty() => Some(r.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            if run.state.stage == Stage::Paused {
                let back = run.state.paused_from.unwrap_or(Stage::Building);
                run.append(Event::Stage {
                    to: back,
                    why: "the engineer allowed another repair".into(),
                })?;
            }
            reject(run, &part, reasons)?;
        }
    }
    if run.state.stage == Stage::Paused && limit_hit(run).is_none() {
        let back = run.state.paused_from.unwrap_or(Stage::Building);
        run.append(Event::Stage {
            to: back,
            why: "the engineer raised the limits".into(),
        })?;
    }
    Ok(())
}

/// The engineer's merge of the verified result (FR-009).
pub fn merge(run: &mut Run, tx: std::sync::mpsc::Sender<Msg>) -> Result<(), String> {
    if run.state.stage != Stage::Ready {
        return Err("the run is not ready to merge".into());
    }
    if run.busy.is_some() {
        return Err("a merge is already running".into());
    }
    let commit = run
        .state
        .integrated
        .clone()
        .ok_or("nothing is integrated")?;
    run.append(Event::Steering {
        kind: SteeringKind::Merge,
        target: Some(run.state.target.clone()),
        detail: Some(commit.clone()),
    })?;
    run.busy = Some(Busy::Merge);
    let repo = run.state.repo.clone();
    let target = run.state.target.clone();
    let scratch = run.worktrees().join("merge");
    let id = run.id();
    std::thread::Builder::new()
        .name(format!("merge-{id}"))
        .spawn(move || {
            let r = crate::publish::local::merge(&repo, &target, &commit, &scratch);
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Merge(r),
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// Stops the run (constitution II: confirmed on the page). Branches and worktrees stay.
pub fn stop(run: &mut Run, reason: Option<String>) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is already over".into());
    }
    run.stop.store(true, Ordering::Relaxed);
    run.append(Event::Steering {
        kind: SteeringKind::Stop,
        target: None,
        detail: reason.clone(),
    })?;
    run.append(Event::Stopped { reason })?;
    run.append(Event::Stage {
        to: Stage::Stopped,
        why: "stopped by the engineer".into(),
    })
}

/// The first line of `text`, or `default` when it is empty.
fn first_line(text: &str, default: &str) -> String {
    let l = text.lines().next().unwrap_or("").trim();
    let l = if l.is_empty() { default } else { l };
    l.chars().take(72).collect()
}

/// The answers a step's value holds, for tests and the page.
pub fn value_of(run: &Run, n: u32) -> Option<Value> {
    run.state.steps.get(&n).and_then(|s| s.result().cloned())
}
