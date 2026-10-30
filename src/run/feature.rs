//! The loop over a locked feature's `tasks.md` (007 US2; research S-9, FR-010 to FR-016a).
//!
//! Phase by phase and task by task: a builder session per phase, resumed from task to task;
//! Fallow ticks and commits each task, runs the gates against the baseline, and accepts the task
//! only when they pass; a handover ends each phase; checkers check every user story at its
//! checkpoint by running something; failures go back to the phase's session as repairs, within
//! the pipeline's caps. Each decision is derived again from the event log, so a restart resumes
//! where the loop was without repeating a completed task, gate run or review (constitution VII).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use super::engine::{Busy, Ctx, Done, Msg, Run};
use super::events::*;
use super::machine::{self, BLOCKED, INTERRUPTED, SESSION_LOST};
use super::state::{LoopState, PhaseStatus, Step};
use crate::agent::{self, Call, Runtime, Session, prompts, rights};
use crate::config::GateDef;
use crate::defs::pipeline::{self, At, OnFail, Pipeline, When};
use crate::defs::{Kind, Library};
use crate::speckit;
use crate::verify::git;

/// How many times a phase's handover is asked for again before the engineer is asked.
const HANDOVER_TRIES: usize = 2;
/// How many times a checker is asked for a complete verdict before the engineer is asked.
const CHECK_TRIES: usize = 3;
/// The start of a reason that a verdict was incomplete.
const INCOMPLETE: &str = "incomplete verdict: ";

fn lp(run: &Run) -> Result<LoopState, String> {
    run.state
        .feature_loop
        .clone()
        .ok_or_else(|| "not a feature's loop".to_string())
}

pub(crate) fn library(run: &Run, ctx: &Ctx) -> Library {
    Library::load(ctx.config.as_deref(), Some(&run.state.repo))
}

pub(crate) fn pipeline_of(run: &Run, lib: &Library) -> Result<Pipeline, String> {
    let l = lp(run)?;
    let kinds = |n: &str| lib.agent(n).map(|a| a.kind);
    pipeline::parse(
        "the loop's pipeline",
        "",
        &l.pipeline,
        crate::defs::Source::Default,
        &kinds,
    )
    .or_else(|_| {
        // The recorded TOML names itself; read it with its own name.
        let name = l
            .pipeline
            .lines()
            .find_map(|x| x.strip_prefix("name = "))
            .map(|x| x.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        pipeline::parse(
            "the loop's pipeline",
            &name,
            &l.pipeline,
            crate::defs::Source::Default,
            &kinds,
        )
    })
    .map_err(|e| format!("the loop's pipeline cannot be read: {}", e.join("; ")))
}

fn build_wt(run: &Run) -> PathBuf {
    run.worktrees().join("build")
}

/// The locked documents a maker may never change (FR-009): the feature's spec documents and
/// the constitution. `tasks.md` changes only by Fallow's ticks.
fn protected(l: &LoopState) -> Vec<String> {
    let dir = format!("specs/{}", l.feature);
    let mut v: Vec<String> = [
        "spec.md",
        "plan.md",
        "tasks.md",
        "research.md",
        "data-model.md",
        "quickstart.md",
    ]
    .iter()
    .map(|f| format!("{dir}/{f}"))
    .collect();
    v.push(format!("{dir}/contracts/**"));
    v.push(".specify/memory/constitution.md".into());
    v
}

fn is_protected(l: &LoopState, path: &str) -> bool {
    let dir = format!("specs/{}/", l.feature);
    (path.starts_with(&dir) && !path.starts_with(&format!("{dir}handover/")))
        || path == ".specify/memory/constitution.md"
}

pub(crate) fn doc_at(run: &Run, commit: &str, name: &str) -> Option<String> {
    let l = run.state.feature_loop.as_ref()?;
    git::show(
        &run.state.repo,
        commit,
        &format!("specs/{}/{name}", l.feature),
    )
    .ok()
    .flatten()
}

fn handover_path(l: &LoopState, n: u32) -> String {
    format!("specs/{}/handover/phase-{n}.md", l.feature)
}

/// The commands a step may run besides reading: its definition's `may_run`, with the gates and
/// the locked plan's commands expanded.
fn commands(run: &Run, may_run: &[String]) -> Vec<String> {
    let l = run.state.feature_loop.as_ref();
    let plan_commands = l
        .and_then(|l| doc_at(run, &l.lock_commit, "plan.md"))
        .and_then(|t| speckit::plan::commands("plan.md", &t).ok())
        .unwrap_or_default();
    rights::expand_may_run(may_run, &run.state.gates, &plan_commands)
}

#[allow(clippy::too_many_arguments)]
fn queue(
    run: &mut Run,
    kind: StepKind,
    part: Option<&str>,
    task: Option<&str>,
    phase: Option<u32>,
    def: &str,
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
        task: task.map(str::to_string),
        phase,
        def: Some(def.to_string()),
    })
    .map(|()| n)
}

/// The phase's builder session: the latest session of a builder step of that phase that can
/// still be resumed (FR-010a: one session per phase).
fn phase_session(run: &Run, phase: u32) -> Option<String> {
    // A phase reopened or renumbered by a new lock starts a fresh session (research S-17).
    let since = run
        .state
        .feature_loop
        .as_ref()
        .and_then(|l| l.phases.iter().find(|p| p.spec.n == phase))
        .and_then(|p| p.reopened)
        .unwrap_or(0);
    run.state
        .steps
        .values()
        .rev()
        .filter(|s| {
            s.phase == Some(phase)
                && s.queued_seq > since
                && matches!(
                    s.kind,
                    StepKind::Build | StepKind::Repair | StepKind::Handover
                )
        })
        .find_map(|s| s.session.clone())
        .filter(|s| !run.state.lost_sessions.contains(s))
}

fn handovers_before(l: &LoopState, n: u32) -> Vec<String> {
    l.phases
        .iter()
        .filter(|p| p.spec.n < n)
        .filter_map(|p| p.handover.clone())
        .collect()
}

/// Decides and starts what comes next for a feature's loop. Several decisions may follow one
/// another in one pass; it stops when something starts running or nothing changes.
pub fn drive(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    for _ in 0..64 {
        let before = run.state.seq;
        step(run, ctx)?;
        if run.busy.is_some()
            || run.state.needs.is_some()
            || run.state.seq == before
            || run.state.stage != Stage::Building
        {
            return Ok(());
        }
    }
    Ok(())
}

fn step(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    let l = lp(run)?;
    if l.unlocked || l.outcome.is_some() {
        return Ok(());
    }
    if let Some(n) = machine::unprocessed(run) {
        return process(run, ctx, n);
    }
    if requeue_cut_short(run)? {
        return Ok(());
    }
    if let Some(n) = machine::queued(run) {
        if let Some((w, u)) = machine::limit_hit(run) {
            return machine::pause_for_limit(run, w, u);
        }
        if !machine::slot(run, ctx) {
            return Ok(());
        }
        return start(run, ctx, n);
    }
    let lib = library(run, ctx);
    let pl = pipeline_of(run, &lib)?;
    // The build worktree, on the build branch (research S-9).
    let wt = build_wt(run);
    if !wt.join(".git").exists() {
        git::worktree_add(&run.state.repo, &wt, Some(&l.build_branch), &l.lock_commit)?;
    }
    // A new lock is taken into the build branch first.
    if !git::is_ancestor(&run.state.repo, &l.lock_commit, &git::head(&wt)?) {
        return take_lock(run, &l, &wt);
    }
    // The baseline, on the lock commit, before anything is judged against it; a new lock gets
    // a new baseline.
    let missing: Vec<GateDef> = machine::gate_defs(run)
        .into_iter()
        .filter(|g| {
            g.baseline
                && !run.state.gate_runs.iter().any(|r| {
                    r.kind == GateKind::Baseline
                        && r.result.gate == g.name
                        && r.result.commit == l.lock_commit
                })
        })
        .collect();
    if !missing.is_empty() {
        return machine::run_gates(
            run,
            ctx,
            GateKind::Baseline,
            None,
            l.lock_commit.clone(),
            missing,
        );
    }
    let last_phase = l.phases.iter().map(|p| p.spec.n).max().unwrap_or(1);
    for s in pl.steps.iter().filter(|s| s.when == When::Once && s.at() == At::Start) {
        let what = "Do your work for the whole feature before the builder starts, as your definition says.";
        if !extra(run, ctx, &l, &pl, &lib, s, &format!("{}@once-start", s.agent), 1, None, what)? {
            return Ok(());
        }
    }
    for p in &l.phases {
        if p.state == PhaseState::Done {
            continue;
        }
        return phase(run, ctx, &l, &pl, &lib, p);
    }
    for s in pl.steps.iter().filter(|s| s.when == When::Once && s.at() == At::End) {
        let what = "Every phase of the feature is built and verified. Do your work for the whole feature, as your definition says.";
        if !extra(run, ctx, &l, &pl, &lib, s, &format!("{}@once-end", s.agent), last_phase, None, what)? {
            return Ok(());
        }
    }
    finish(run, &pl)
}

/// A maker's accepted work goes through the gates like a task's, when the pipeline has gates.
fn maker_gated(
    run: &mut Run,
    ctx: &Ctx,
    pl: &Pipeline,
    part: &str,
    phase: u32,
    commit: Option<String>,
) -> Result<bool, String> {
    let Some(c) = commit else {
        return Ok(true);
    };
    if !pl.steps.iter().any(|s| s.agent == pipeline::GATES) {
        return Ok(true);
    }
    match gated(run, ctx, part, &c)? {
        None => Ok(false),
        Some(failed) if !failed.is_empty() => {
            repair_or_ask(run, part, None, phase, pl.caps.task_repairs, &builder_of(pl), failed)?;
            Ok(false)
        }
        Some(_) => Ok(true),
    }
}

/// Whether the engineer paused the loop (and has not let it go on since).
pub fn paused_by_engineer(run: &Run) -> bool {
    run.state
        .steering
        .iter()
        .rev()
        .find(|s| {
            s.kind == SteeringKind::Pause
                || (s.kind == SteeringKind::Resume && s.detail.as_deref() == Some(machine::RESUMED))
        })
        .is_some_and(|s| s.kind == SteeringKind::Pause)
}

/// The engineer's notes for the next step (FR-019) that no step has received yet: those after
/// the latest step that started, for this agent or for any.
fn notes_for(run: &Run, n: u32, def: &str) -> Vec<String> {
    let since = run
        .state
        .steps
        .values()
        .filter(|s| s.n != n && s.started.is_some())
        .map(|s| s.queued_seq)
        .max()
        .unwrap_or(0);
    run.state
        .steering
        .iter()
        .filter(|s| s.kind == SteeringKind::Note && s.seq > since)
        .filter(|s| s.target.as_deref().is_none_or(|t| t == def))
        .filter_map(|s| s.detail.clone())
        .collect()
}

/// Where a new round of tries and repairs starts: the engineer's latest "retry" or "allow one
/// more repair", or the latest new lock (FR-013, research S-17). Caps count from there.
fn fresh_since(run: &Run) -> u64 {
    let retry = run
        .state
        .steering
        .iter()
        .filter(|s| s.kind == SteeringKind::Resume && s.detail.as_deref() == Some("retry"))
        .map(|s| s.seq)
        .max()
        .unwrap_or(0);
    let relock = run
        .state
        .feature_loop
        .as_ref()
        .map_or(0, |l| l.relocked_seq);
    retry.max(relock)
}

/// Whether any of `changed` matches one of the globs.
fn paths_match(globs: &[String], changed: &[String]) -> bool {
    globs.iter().any(|g| {
        globset::Glob::new(g)
            .map(|g| g.compile_matcher())
            .is_ok_and(|m| changed.iter().any(|f| m.is_match(f)))
    })
}

/// The checker that answers a story's acceptance scenarios: the first one at the checkpoint's
/// end without `paths`, which makes the pipeline verifying (contracts/pipeline.md). Other
/// checkers answer their own checks in `findings` (contracts/step-results.md).
fn scenario_checker(pl: &Pipeline, lib: &Library) -> Option<String> {
    pl.steps
        .iter()
        .find(|s| {
            s.when == When::Checkpoint
                && s.at() == At::End
                && s.paths.is_empty()
                && lib.agent(&s.agent).is_some_and(|a| a.kind == Kind::Checker)
        })
        .map(|s| s.agent.clone())
}

/// A pipeline step around the tasks (research S-9): a maker or a checker at a phase's start or
/// end, at a story's checkpoint start, or once for the whole loop. `Ok(true)` when it passed or
/// its `paths` were not touched; otherwise something was queued, is running, or was asked.
#[allow(clippy::too_many_arguments)]
fn extra(
    run: &mut Run,
    ctx: &Ctx,
    l: &LoopState,
    pl: &Pipeline,
    lib: &Library,
    s: &pipeline::Step,
    part: &str,
    phase: u32,
    at_phase: Option<u32>,
    what: &str,
) -> Result<bool, String> {
    let def = lib
        .agent(&s.agent)
        .cloned()
        .ok_or_else(|| format!("there is no agent named {:?}", s.agent))?;
    let checker = def.kind == Kind::Checker;
    let kind = if checker { StepKind::Review } else { StepKind::Make };
    let fresh = fresh_since(run);
    let of_part = |x: &&Step| x.part.as_deref() == Some(part) && x.queued_seq > fresh;
    let repair = run
        .state
        .steps
        .values()
        .rev()
        .filter(|x| x.kind == StepKind::Repair)
        .find(of_part)
        .cloned();
    let since = repair.as_ref().map_or(fresh, |r| r.queued_seq);
    let last = run
        .state
        .steps
        .values()
        .rev()
        .filter(|x| x.kind == kind && x.queued_seq > since)
        .find(of_part)
        .cloned();
    let builder = builder_of(pl);
    match last.as_ref().map(|x| x.verified.clone()) {
        None => {
            if let Some(r) = &repair {
                match &r.verified {
                    None => return Ok(false),
                    Some((false, reasons)) => {
                        repair_or_ask(run, part, None, phase, pl.caps.task_repairs, &builder, reasons.clone())?;
                        return Ok(false);
                    }
                    // A maker's work, repaired, is gated; a checker looks again.
                    Some((true, _)) if !checker => {
                        let c = r.verified_commit.clone();
                        return maker_gated(run, ctx, pl, part, phase, c);
                    }
                    Some((true, _)) => {}
                }
            } else if !s.paths.is_empty() {
                // Only when the changes since its last run touch its paths.
                let from = run
                    .state
                    .steps
                    .values()
                    .rev()
                    .find(|x| {
                        x.kind == kind
                            && x.part.as_deref() == Some(part)
                            && x.verified.as_ref().is_some_and(|v| v.0)
                    })
                    .and_then(|x| x.base.clone())
                    .unwrap_or_else(|| l.lock_commit.clone());
                let head = git::head(&build_wt(run))?;
                let changed = git::files_between(&run.state.repo, &from, &head).unwrap_or_default();
                if !paths_match(&s.paths, &changed) {
                    return Ok(true);
                }
            }
            let may = may_of(run, lib, &s.agent);
            let hand = handovers_before(l, phase + 1);
            let cx = loop_cx(l, &may, &hand);
            let text = if checker {
                let head = git::head(&build_wt(run))?;
                prompts::loop_check_findings(&cx, part, &s.agent, what, &head)
            } else {
                prompts::loop_make(&cx, part, &s.agent, what)
            };
            queue(
                run,
                kind,
                Some(part),
                None,
                at_phase,
                &s.agent,
                None,
                &format!("{} ({part})", s.agent),
                text,
            )?;
            Ok(false)
        }
        Some(None) => Ok(false),
        Some(Some((true, _))) if checker => Ok(true),
        Some(Some((true, _))) => {
            let c = last.and_then(|x| x.verified_commit);
            maker_gated(run, ctx, pl, part, phase, c)
        }
        Some(Some((false, reasons))) => {
            let last = last.unwrap_or_else(|| unreachable!());
            // A checker that gave no usable verdict is asked again (FR-012).
            if checker && reasons.iter().all(|r| r.starts_with(INCOMPLETE)) {
                let tries = run
                    .state
                    .steps
                    .values()
                    .filter(|x| x.kind == kind && x.queued_seq > since && !machine::cut_short(x))
                    .filter(of_part)
                    .count();
                if tries < CHECK_TRIES {
                    let text = machine::instruction_of(run, last.n);
                    queue(
                        run,
                        kind,
                        Some(part),
                        None,
                        at_phase,
                        &s.agent,
                        None,
                        &format!("{} ({part}) again", s.agent),
                        text,
                    )?;
                    return Ok(false);
                }
            }
            if s.on_fail == OnFail::Engineer
                || reasons.first().is_some_and(|r| r.starts_with(BLOCKED))
                || (checker && reasons.iter().all(|r| r.starts_with(INCOMPLETE)))
            {
                run.append(Event::NeedsEngineer {
                    why: format!("{} ({part}) did not pass: {}", s.agent, reasons.join("; ")),
                    options: vec!["retry".into(), "stop".into()],
                })?;
                return Ok(false);
            }
            repair_or_ask(run, part, None, phase, pl.caps.task_repairs, &builder, reasons)?;
            Ok(false)
        }
    }
}

/// A step cut short (Fallow stopped, or its session was lost) with nothing after it: queued
/// again, in its session or a new one.
fn requeue_cut_short(run: &mut Run) -> Result<bool, String> {
    let Some(last) = run.state.steps.values().next_back().cloned() else {
        return Ok(false);
    };
    let Some(reason) = machine::first_reason(&last).map(str::to_string) else {
        return Ok(false);
    };
    let def = last.def.clone().unwrap_or_else(|| "builder".into());
    if reason.starts_with(INTERRUPTED) {
        let session = last
            .session
            .clone()
            .filter(|s| !run.state.lost_sessions.contains(s));
        let text = match &session {
            Some(_) => format!(
                "Fallow step: {}{}\n\n{}",
                last.kind.label(),
                last.part
                    .as_deref()
                    .map(|p| format!(" (part {p})"))
                    .unwrap_or_default(),
                prompts::LOOP_INTERRUPTED
            ),
            None => machine::instruction_of(run, last.n),
        };
        queue(
            run,
            last.kind,
            last.part.as_deref(),
            last.task.as_deref(),
            last.phase,
            &def,
            session,
            "resumed after Fallow stopped",
            text,
        )?;
        return Ok(true);
    }
    if reason.starts_with(SESSION_LOST) {
        let text = machine::instruction_of(run, last.n);
        queue(
            run,
            last.kind,
            last.part.as_deref(),
            last.task.as_deref(),
            last.phase,
            &def,
            None,
            "a new session takes over a session that could not be resumed",
            text,
        )?;
        return Ok(true);
    }
    Ok(false)
}

fn loop_cx<'a>(
    l: &'a LoopState,
    may: &'a [String],
    handovers: &'a [String],
) -> prompts::LoopCx<'a> {
    prompts::LoopCx {
        feature_dir: Box::leak(format!("specs/{}", l.feature).into_boxed_str()),
        title: &l.feature,
        may_run: may,
        handovers,
    }
}

/// The builder's definition name in the pipeline.
fn builder_of(pl: &Pipeline) -> String {
    pl.builder()
        .map(|s| s.agent.clone())
        .unwrap_or_else(|| "builder".into())
}

fn may_of(run: &Run, lib: &Library, def: &str) -> Vec<String> {
    let m = lib
        .agent(def)
        .map(|a| a.may_run.clone())
        .unwrap_or_default();
    commands(run, &m)
}

/// The steps of a task: its build and repairs.
fn task_steps<'a>(run: &'a Run, task: &str) -> impl Iterator<Item = &'a Step> {
    run.state.steps.values().filter(move |s| {
        s.task.as_deref() == Some(task) && matches!(s.kind, StepKind::Build | StepKind::Repair)
    })
}

fn repairs_of(run: &Run, part: &str) -> u32 {
    run.state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Repair && s.part.as_deref() == Some(part))
        .filter(|s| !machine::cut_short(s) && !s.reason.starts_with(RELOCK))
        .filter(|s| s.queued_seq > fresh_since(run))
        .count() as u32
}

/// The reason of the step that brings a reopened story in line with a new lock.
const RELOCK: &str = "the new lock";

/// Takes a new lock's documents into the build branch at a task boundary (research S-9, S-17),
/// keeping the ticks of the tasks already accepted.
fn take_lock(run: &mut Run, l: &LoopState, wt: &std::path::Path) -> Result<(), String> {
    let msg = format!("lock {} of {}", l.lock, l.feature);
    if let Err(e) = git::merge_theirs(wt, &l.lock_commit) {
        return run.append(Event::NeedsEngineer {
            why: format!("Fallow could not take lock {} into the build branch: {e}", l.lock),
            options: vec!["retry".into(), "stop".into()],
        });
    }
    let rel = format!("specs/{}/tasks.md", l.feature);
    let file = wt.join(&rel);
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    for (id, t) in &l.tasks {
        if t.state == TaskState::Accepted {
            if let Some(ticked) = speckit::tasks::tick(&text, id) {
                text = ticked;
            }
        }
    }
    std::fs::write(&file, &text).map_err(|e| format!("{}: {e}", file.display()))?;
    git::commit_merge(wt, &format!("{msg}, the accepted tasks still ticked"))?;
    run.append(Event::Note {
        text: format!("lock {} was taken into the build branch", l.lock),
    })
}

/// The gates of a commit, recorded under `part`: the failures, or `None` while some have not
/// run (and are then started).
fn gated(
    run: &mut Run,
    ctx: &Ctx,
    part: &str,
    commit: &str,
) -> Result<Option<Vec<String>>, String> {
    let defs = machine::gate_defs(run);
    let missing: Vec<GateDef> = defs
        .iter()
        .filter(|d| {
            !run.state.gate_runs.iter().any(|g| {
                g.kind == GateKind::Part
                    && g.part.as_deref() == Some(part)
                    && g.result.commit == commit
                    && g.result.gate == d.name
            })
        })
        .cloned()
        .collect();
    if !missing.is_empty() {
        machine::run_gates(
            run,
            ctx,
            GateKind::Part,
            Some(part.to_string()),
            commit.to_string(),
            missing,
        )?;
        return Ok(None);
    }
    Ok(Some(
        run.state
            .gate_runs
            .iter()
            .filter(|g| {
                g.kind == GateKind::Part
                    && g.part.as_deref() == Some(part)
                    && g.result.commit == commit
                    && !gate_ok(run, g)
            })
            .map(|g| {
                let names: Vec<&str> = g
                    .result
                    .failing
                    .iter()
                    .map(String::as_str)
                    .take(8)
                    .collect();
                format!(
                    "gate {} {} ({}){}",
                    g.result.gate,
                    g.result.outcome.label(),
                    g.class.map_or("not classified", |k| k.label()),
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", names.join(", "))
                    }
                )
            })
            .collect(),
    ))
}

/// Whether a gate run lets the work through (FR-011): it passed, or failed only where the
/// baseline failed, or ran no tests while the baseline had none either (a project's first
/// tasks; no test that passed in the baseline fails).
pub(crate) fn gate_ok(run: &Run, g: &super::state::GateEntry) -> bool {
    if g.class.is_some_and(|k| k.ok()) {
        return true;
    }
    use crate::verify::gates::Outcome;
    g.result.outcome == Outcome::NoTests
        && run
            .state
            .baseline(&g.result.gate)
            .is_none_or(|b| matches!(b.outcome, Outcome::NoTests | Outcome::NotStarted { .. }))
}

/// A task's or a story's work was not accepted: a repair in the phase's session within the
/// cap, else the engineer (FR-013).
#[allow(clippy::too_many_arguments)]
fn repair_or_ask(
    run: &mut Run,
    part: &str,
    task: Option<&str>,
    phase: u32,
    cap: u32,
    def: &str,
    reasons: Vec<String>,
) -> Result<(), String> {
    let first = reasons.first().cloned().unwrap_or_default();
    if let Some(q) = first.strip_prefix(BLOCKED) {
        if let Some(t) = task {
            run.append(Event::Task {
                id: t.into(),
                state: TaskState::Waiting,
                commit: None,
                repairs: repairs_of(run, part),
            })?;
        }
        return run.append(Event::NeedsEngineer {
            why: format!("The builder of {part} asks: {q}"),
            options: vec!["answer".into(), "stop".into()],
        });
    }
    let done = repairs_of(run, part);
    if done >= cap {
        if let Some(t) = task {
            run.append(Event::Task {
                id: t.into(),
                state: TaskState::Failed,
                commit: None,
                repairs: done,
            })?;
        }
        return run.append(Event::NeedsEngineer {
            why: format!(
                "{part} still fails after {done} repair round(s): {}. Allow one more repair, or stop the loop.",
                reasons.join("; ")
            ),
            options: vec!["allow one more repair".into(), "stop".into()],
        });
    }
    let session = phase_session(run, phase);
    let text = prompts::loop_repair(part, &reasons);
    let text = match &session {
        Some(_) => text,
        None => format!(
            "{}\n\n{text}",
            machine::instruction_of(
                run,
                run.state
                    .steps
                    .values()
                    .rev()
                    .find(|s| s.phase == Some(phase) && s.kind == StepKind::Build)
                    .map_or(0, |s| s.n)
            )
        ),
    };
    queue(
        run,
        StepKind::Repair,
        Some(part),
        task,
        Some(phase),
        def,
        session,
        &format!("repair round {} of {cap}", done + 1),
        text,
    )?;
    if let Some(t) = task {
        run.append(Event::Task {
            id: t.into(),
            state: TaskState::Current,
            commit: None,
            repairs: done + 1,
        })?;
    }
    Ok(())
}

/// One pass over the first phase that is not done.
fn phase(
    run: &mut Run,
    ctx: &Ctx,
    l: &LoopState,
    pl: &Pipeline,
    lib: &Library,
    p: &PhaseStatus,
) -> Result<(), String> {
    let n = p.spec.n;
    let builder = builder_of(pl);
    if p.state == PhaseState::Pending {
        return run.append(Event::Phase {
            n,
            state: PhaseState::Building,
            session: None,
            handover: None,
        });
    }
    // A phase reopened by a new lock: its story's builder brings the work in line with the
    // change, in a fresh session, before anything else (research S-17).
    let stories: Vec<String> = stories_of(p)
        .into_iter()
        .filter(|s| l.affected.contains(s))
        .collect();
    for story in stories {
        let Some(at) = p.reopened else { break };
        let first = l
            .phases
            .iter()
            .filter(|q| q.reopened == Some(at) && stories_of(q).contains(&story))
            .map(|q| q.spec.n)
            .min();
        let mine = run
            .state
            .steps
            .values()
            .rev()
            .find(|s| {
                s.kind == StepKind::Repair
                    && s.part.as_deref() == Some(story.as_str())
                    && s.queued_seq > at
            })
            .cloned();
        match mine {
            None if first == Some(n) => {
                let may = may_of(run, lib, &builder);
                let hand = handovers_before(l, n);
                let change = l
                    .prev_lock_commit
                    .as_deref()
                    .and_then(|old| {
                        git::diff_between(
                            &run.state.repo,
                            old,
                            &l.lock_commit,
                            &format!("specs/{}", l.feature),
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                let text = prompts::loop_relock(&loop_cx(l, &may, &hand), &story, l.lock, &change);
                return queue(
                    run,
                    StepKind::Repair,
                    Some(&story),
                    None,
                    Some(n),
                    &builder,
                    None,
                    &format!("{RELOCK} ({}) changed {story}", l.lock),
                    text,
                )
                .map(|_| ());
            }
            Some(s) if s.verified.is_none() => return Ok(()),
            _ => {}
        }
    }
    // Makers at the start of the phase (a test-writer at a story's phase).
    for s in pl.steps.iter().filter(|s| {
        s.at() == At::Start
            && (s.when == When::Phase || (s.when == When::Checkpoint && p.spec.story.is_some()))
    }) {
        let what = match &p.spec.story {
            Some(st) if s.when == When::Checkpoint => format!(
                "Before the builder starts phase {} ({}), write the acceptance tests of user story {st}: one per acceptance scenario in spec.md, including the must-fail ones, and show they fail because the behaviour is missing.",
                p.spec.label, p.spec.title
            ),
            _ => format!(
                "Work on phase {} ({}) before the builder starts it, as your definition says.",
                p.spec.label, p.spec.title
            ),
        };
        let part = format!("{}@phase-{n}-start", s.agent);
        if !extra(run, ctx, l, pl, lib, s, &part, n, Some(n), &what)? {
            return Ok(());
        }
    }
    let gates_per_task = pl
        .steps
        .iter()
        .any(|s| s.agent == pipeline::GATES && s.when == When::Task);
    // The tasks, in order.
    for t in &p.spec.tasks {
        let status = l.tasks.get(&t.id).cloned();
        if status
            .as_ref()
            .is_some_and(|s| s.state == TaskState::Accepted)
        {
            continue;
        }
        let last = task_steps(run, &t.id).last().cloned();
        let Some(last) = last else {
            let session = phase_session(run, n);
            let may = may_of(run, lib, &builder);
            let hand = handovers_before(l, n);
            let text = prompts::loop_task(
                &loop_cx(l, &may, &hand),
                (&p.spec.label, &p.spec.title),
                (&t.id, &t.text),
                session.is_none(),
            );
            run.append(Event::Task {
                id: t.id.clone(),
                state: TaskState::Current,
                commit: None,
                repairs: 0,
            })?;
            queue(
                run,
                StepKind::Build,
                Some(&t.id),
                Some(&t.id),
                Some(n),
                &builder,
                session,
                &format!("task {}", t.id),
                text,
            )?;
            return Ok(());
        };
        match &last.verified {
            None => return Ok(()),
            Some((false, reasons)) => {
                let reasons = reasons.clone();
                return repair_or_ask(
                    run,
                    &t.id,
                    Some(&t.id),
                    n,
                    pl.caps.task_repairs,
                    &builder,
                    reasons,
                );
            }
            Some((true, _)) => {
                let c = last
                    .verified_commit
                    .clone()
                    .ok_or_else(|| format!("step {} was accepted without a commit", last.n))?;
                if gates_per_task {
                    match gated(run, ctx, &t.id, &c)? {
                        None => return Ok(()),
                        Some(failed) if !failed.is_empty() => {
                            return repair_or_ask(
                                run,
                                &t.id,
                                Some(&t.id),
                                n,
                                pl.caps.task_repairs,
                                &builder,
                                failed,
                            );
                        }
                        Some(_) => {}
                    }
                }
                return run.append(Event::Task {
                    id: t.id.clone(),
                    state: TaskState::Accepted,
                    commit: Some(c),
                    repairs: repairs_of(run, &t.id),
                });
            }
        }
    }
    // Gates per phase, when the pipeline runs them so.
    if pl
        .steps
        .iter()
        .any(|s| s.agent == pipeline::GATES && s.when == When::Phase)
    {
        let part = format!("phase-{n}");
        let fresh = fresh_since(run);
        let repair = run
            .state
            .steps
            .values()
            .rev()
            .find(|s| s.kind == StepKind::Repair && s.part.as_deref() == Some(part.as_str()) && s.queued_seq > fresh)
            .cloned();
        match repair.as_ref().map(|r| &r.verified) {
            Some(None) => return Ok(()),
            Some(Some((false, reasons))) => {
                let reasons = reasons.clone();
                return repair_or_ask(run, &part, None, n, pl.caps.task_repairs, &builder, reasons);
            }
            _ => {}
        }
        let head = git::head(&build_wt(run))?;
        match gated(run, ctx, &part, &head)? {
            None => return Ok(()),
            Some(failed) if !failed.is_empty() => {
                return repair_or_ask(run, &part, None, n, pl.caps.task_repairs, &builder, failed);
            }
            Some(_) => {}
        }
    }
    // The handover (FR-010a).
    if p.handover.is_none() {
        let hs: Vec<Step> = run
            .state
            .steps
            .values()
            .filter(|s| s.kind == StepKind::Handover && s.phase == Some(n))
            .cloned()
            .collect();
        match hs.last().map(|s| s.verified.clone()) {
            None => {
                let session = phase_session(run, n);
                let may = may_of(run, lib, &builder);
                let hand = handovers_before(l, n);
                let path = handover_path(l, n);
                let text = prompts::loop_handover(
                    &loop_cx(l, &may, &hand),
                    n,
                    (&p.spec.label, &p.spec.title),
                    &path,
                );
                queue(
                    run,
                    StepKind::Handover,
                    Some(&format!("phase-{n}")),
                    None,
                    Some(n),
                    &builder,
                    session,
                    &format!("the handover of phase {}", p.spec.label),
                    text,
                )?;
                return Ok(());
            }
            Some(None) => return Ok(()),
            Some(Some((false, reasons))) => {
                let fresh = fresh_since(run);
                if hs
                    .iter()
                    .filter(|s| !machine::cut_short(s) && s.queued_seq > fresh)
                    .count()
                    >= HANDOVER_TRIES
                {
                    return run.append(Event::NeedsEngineer {
                        why: format!(
                            "The handover of phase {} was not usable: {}",
                            p.spec.label,
                            reasons.join("; ")
                        ),
                        options: vec!["retry".into(), "stop".into()],
                    });
                }
                let session = phase_session(run, n);
                let mut text = prompts::loop_handover(
                    &loop_cx(l, &[], &[]),
                    n,
                    (&p.spec.label, &p.spec.title),
                    &handover_path(l, n),
                );
                text.push_str(&format!(
                    "\n\nThe last handover was not accepted: {}",
                    reasons.join("; ")
                ));
                queue(
                    run,
                    StepKind::Handover,
                    Some(&format!("phase-{n}")),
                    None,
                    Some(n),
                    &builder,
                    session,
                    "the handover, again",
                    text,
                )?;
                return Ok(());
            }
            Some(Some((true, _))) => {
                return run.append(Event::Phase {
                    n,
                    state: PhaseState::Checkpoint,
                    session: None,
                    handover: Some(handover_path(l, n)),
                });
            }
        }
    }
    // Steps at the end of the phase (FR-011, contracts/pipeline.md).
    for s in pl
        .steps
        .iter()
        .filter(|s| s.when == When::Phase && s.at() == At::End)
    {
        let what = format!(
            "Phase {} ({}) is built and handed over. Do your work on it, as your definition says.",
            p.spec.label, p.spec.title
        );
        let part = format!("{}@phase-{n}-end", s.agent);
        if !extra(run, ctx, l, pl, lib, s, &part, n, Some(n), &what)? {
            return Ok(());
        }
    }
    // The checkpoints of the stories whose last tasks are in this phase.
    for story in stories_ending_at(l, n) {
        if let Some(done) = checkpoint(run, ctx, l, pl, lib, &story, n)? {
            if !done {
                return Ok(());
            }
        }
    }
    run.append(Event::Phase {
        n,
        state: PhaseState::Done,
        session: None,
        handover: None,
    })
}

/// The stories a phase works on: its own, and its tasks'.
fn stories_of(p: &PhaseStatus) -> Vec<String> {
    let mut out: Vec<String> = p.spec.story.iter().cloned().collect();
    for t in &p.spec.tasks {
        if let Some(s) = &t.story {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
    }
    out
}

/// The stories whose last task (or story phase) is phase `n`: their checkpoint is there.
fn stories_ending_at(l: &LoopState, n: u32) -> Vec<String> {
    let mut stories: Vec<String> = Vec::new();
    for p in &l.phases {
        if let Some(s) = &p.spec.story {
            if !stories.contains(s) {
                stories.push(s.clone());
            }
        }
        for t in &p.spec.tasks {
            if let Some(s) = &t.story {
                if !stories.contains(s) {
                    stories.push(s.clone());
                }
            }
        }
    }
    stories
        .into_iter()
        .filter(|s| {
            let last = l
                .phases
                .iter()
                .filter(|p| {
                    p.spec.story.as_deref() == Some(s.as_str())
                        || p.spec
                            .tasks
                            .iter()
                            .any(|t| t.story.as_deref() == Some(s.as_str()))
                })
                .map(|p| p.spec.n)
                .max();
            last == Some(n)
        })
        .collect()
}

/// One story's checkpoint: every checker of the pipeline that applies, on the current build
/// head. `Some(true)` when they all passed since the last repair, `Some(false)` while waiting.
fn checkpoint(
    run: &mut Run,
    ctx: &Ctx,
    l: &LoopState,
    pl: &Pipeline,
    lib: &Library,
    story: &str,
    n: u32,
) -> Result<Option<bool>, String> {
    let head = git::head(&build_wt(run))?;
    let checkers: Vec<pipeline::Step> = pl
        .steps
        .iter()
        .filter(|s| s.when == When::Checkpoint && s.at() == At::End)
        .filter(|s| lib.agent(&s.agent).is_some_and(|a| a.kind == Kind::Checker))
        .filter(|s| {
            if s.paths.is_empty() {
                return true;
            }
            // Only when the changes since its last run on this story touch its paths.
            let from = run
                .state
                .steps
                .values()
                .rev()
                .find(|x| {
                    x.kind == StepKind::Review
                        && x.part.as_deref() == Some(story)
                        && x.def.as_deref() == Some(s.agent.as_str())
                        && x.verified.as_ref().is_some_and(|v| v.0)
                })
                .and_then(|x| x.base.clone())
                .unwrap_or_else(|| l.lock_commit.clone());
            let changed = git::files_between(&run.state.repo, &from, &head).unwrap_or_default();
            paths_match(&s.paths, &changed)
        })
        .cloned()
        .collect();
    if checkers.is_empty() {
        return Ok(Some(true));
    }
    let answerer = scenario_checker(pl, lib);
    // The latest repair of the story, or a new lock that reopened it: checks before it are
    // stale (research S-17).
    let reopened = l
        .phases
        .iter()
        .filter(|p| stories_of(p).iter().any(|s| s == story))
        .filter_map(|p| p.reopened)
        .max()
        .unwrap_or(0);
    let last_repair = run
        .state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Repair && s.part.as_deref() == Some(story))
        .map(|s| s.queued_seq)
        .max()
        .unwrap_or(0)
        .max(reopened);
    let fresh = fresh_since(run).max(last_repair);
    let mut all = true;
    for c in &checkers {
        let last = run
            .state
            .steps
            .values()
            .rev()
            .find(|s| {
                s.kind == StepKind::Review
                    && s.part.as_deref() == Some(story)
                    && s.def.as_deref() == Some(c.agent.as_str())
                    && s.queued_seq > last_repair
            })
            .cloned();
        match last.as_ref().map(|s| s.verified.clone()) {
            None => {
                // A repair still running or not yet gated: wait for it.
                let open = run.state.steps.values().any(|s| {
                    s.kind == StepKind::Repair
                        && s.part.as_deref() == Some(story)
                        && s.verified.is_none()
                });
                if open {
                    return Ok(Some(false));
                }
                if let Some(r) = run
                    .state
                    .steps
                    .values()
                    .rev()
                    .find(|s| s.kind == StepKind::Repair && s.part.as_deref() == Some(story))
                    .cloned()
                {
                    match &r.verified {
                        Some((false, reasons)) => {
                            let reasons = reasons.clone();
                            repair_or_ask(
                                run,
                                story,
                                None,
                                n,
                                pl.caps.review_rounds,
                                &builder_of(pl),
                                reasons,
                            )?;
                            return Ok(Some(false));
                        }
                        Some((true, _)) => {
                            if let Some(c) = r.verified_commit.clone() {
                                if pl.steps.iter().any(|s| s.agent == pipeline::GATES) {
                                    // The repair is gated like a task before the checkers
                                    // look again.
                                    match gated(run, ctx, story, &c)? {
                                        None => return Ok(Some(false)),
                                        Some(f) if !f.is_empty() => {
                                            repair_or_ask(
                                                run,
                                                story,
                                                None,
                                                n,
                                                pl.caps.review_rounds,
                                                &builder_of(pl),
                                                f,
                                            )?;
                                            return Ok(Some(false));
                                        }
                                        Some(_) => {}
                                    }
                                }
                            }
                        }
                        None => return Ok(Some(false)),
                    }
                }
                let spec_text = doc_at(run, &l.lock_commit, "spec.md").unwrap_or_default();
                let spec = speckit::spec::read("spec.md", &spec_text).map_err(|e| {
                    e.iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                })?;
                let st = spec
                    .story(story)
                    .ok_or_else(|| format!("{story} is not in the locked spec.md"))?;
                let scenarios: Vec<prompts::Scenario> = st
                    .scenarios
                    .iter()
                    .map(|s| prompts::Scenario {
                        id: &s.id,
                        text: &s.text,
                        must_fail: s.must_fail,
                    })
                    .collect();
                let may = may_of(run, lib, &c.agent);
                let text = if answerer.as_deref() == Some(c.agent.as_str()) {
                    prompts::loop_check(
                        &loop_cx(l, &may, &[]),
                        &c.agent,
                        story,
                        &st.title,
                        &scenarios,
                        &head,
                    )
                } else {
                    prompts::loop_check_findings(
                        &loop_cx(l, &may, &[]),
                        story,
                        &c.agent,
                        &format!("Check user story {story} (\"{}\") of the feature.", st.title),
                        &head,
                    )
                };
                queue(
                    run,
                    StepKind::Review,
                    Some(story),
                    None,
                    Some(n),
                    &c.agent,
                    None,
                    &format!("{} checks {story}", c.agent),
                    text,
                )?;
                return Ok(Some(false));
            }
            Some(None) => return Ok(Some(false)),
            Some(Some((false, reasons))) => {
                // Incomplete: the checker is asked again, not the engineer (FR-012).
                if reasons.iter().all(|r| r.starts_with(INCOMPLETE)) {
                    let tries = run
                        .state
                        .steps
                        .values()
                        .filter(|s| {
                            s.kind == StepKind::Review
                                && s.part.as_deref() == Some(story)
                                && s.def.as_deref() == Some(c.agent.as_str())
                                && s.queued_seq > fresh
                                && !machine::cut_short(s)
                        })
                        .count();
                    if tries >= CHECK_TRIES {
                        run.append(Event::NeedsEngineer {
                            why: format!(
                                "{} gave no usable verdict on {story} after {tries} tries: {}",
                                c.agent,
                                reasons.join("; ")
                            ),
                            options: vec!["retry".into(), "stop".into()],
                        })?;
                        return Ok(Some(false));
                    }
                    let missing: Vec<String> = reasons
                        .iter()
                        .map(|r| r.trim_start_matches(INCOMPLETE).to_string())
                        .collect();
                    let session = last
                        .as_ref()
                        .and_then(|s| s.session.clone())
                        .filter(|s| !run.state.lost_sessions.contains(s))
                        .filter(|_| !reasons.iter().any(|r| r.contains("worktree changed")));
                    let text = match &session {
                        Some(_) => prompts::loop_check_missing(story, &missing),
                        None => machine::instruction_of(run, last.as_ref().map_or(0, |s| s.n)),
                    };
                    queue(
                        run,
                        StepKind::Review,
                        Some(story),
                        None,
                        Some(n),
                        &c.agent,
                        session,
                        &format!("{} checks {story} again", c.agent),
                        text,
                    )?;
                    return Ok(Some(false));
                }
                let allowed = last.as_ref().is_some_and(|s| s.queued_seq < fresh_since(run));
                if c.on_fail == OnFail::Engineer && !allowed {
                    run.append(Event::NeedsEngineer {
                        why: format!(
                            "{} found problems in {story}: {}",
                            c.agent,
                            reasons.join("; ")
                        ),
                        options: vec!["allow one more repair".into(), "stop".into()],
                    })?;
                    return Ok(Some(false));
                }
                repair_or_ask(
                    run,
                    story,
                    None,
                    n,
                    pl.caps.review_rounds,
                    &builder_of(pl),
                    reasons,
                )?;
                return Ok(Some(false));
            }
            Some(Some((true, _))) => {}
        }
        all &= true;
    }
    Ok(Some(all))
}

/// Every phase is done: the outcome (FR-015) and ready to merge.
fn finish(run: &mut Run, pl: &Pipeline) -> Result<(), String> {
    let head = git::head(&build_wt(run))?;
    // What is merged is the gated work: after the newest gated commit in the build's history,
    // only Fallow's handovers and ticks, and the locked documents as locked, may follow (FR-014).
    let gated: std::collections::BTreeSet<String> = run
        .state
        .gate_runs
        .iter()
        .filter(|g| g.kind == GateKind::Part)
        .map(|g| g.result.commit.clone())
        .collect();
    let history = git::commits_between(&run.state.repo, &lp(run)?.lock_commit, &head).unwrap_or_default();
    if let Some(g) = history.iter().find(|c| gated.contains(*c)) {
        let l = lp(run)?;
        let handovers = format!("specs/{}/handover/", l.feature);
        let tasks = format!("specs/{}/tasks.md", l.feature);
        let after: Vec<String> = git::files_between(&run.state.repo, g, &head)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| !f.starts_with(&handovers) && *f != tasks)
            .filter(|f| {
                !(is_protected(&l, f)
                    && git::show(&run.state.repo, &head, f).ok()
                        == git::show(&run.state.repo, &l.lock_commit, f).ok())
            })
            .collect();
        if !after.is_empty() {
            return run.append(Event::NeedsEngineer {
                why: format!(
                    "These files changed after the last gate run and were never gated: {}",
                    after.join(", ")
                ),
                options: vec!["stop".into()],
            });
        }
    }
    if run.state.integrated.as_deref() != Some(head.as_str()) {
        run.append(Event::Integrated { commit: head })?;
    }
    run.append(Event::Outcome {
        verified: pl.verifying,
        missing: pl.missing.clone(),
    })?;
    run.append(Event::Stage {
        to: Stage::Ready,
        why: if pl.verifying {
            "every phase is done and verified".into()
        } else {
            format!(
                "every phase is done, not verified by Fallow: {}",
                pl.missing.join(", ")
            )
        },
    })
}

/// Starts a queued step of the loop.
fn start(run: &mut Run, ctx: &Ctx, n: u32) -> Result<(), String> {
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    let l = lp(run)?;
    let repo = run.state.repo.clone();
    let lib = library(run, ctx);
    let defname = step.def.clone().unwrap_or_else(|| "builder".into());
    let def = lib
        .agent(&defname)
        .cloned()
        .ok_or_else(|| format!("there is no agent named {defname:?}"))?;
    let (worktree, base) = match step.kind {
        StepKind::Review => {
            let commit = git::head(&build_wt(run))?;
            // One folder per checker and part, so a checker asked again resumes its session in
            // the folder it ran in.
            let name: String = format!(
                "check-{}-{defname}",
                step.part.as_deref().unwrap_or("loop")
            )
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
            let wt = run.worktrees().join(name);
            let _ = git::worktree_remove(&repo, &wt);
            git::worktree_add(&repo, &wt, None, &commit)?;
            if git::head(&wt)? != commit {
                return Err(format!(
                    "the check worktree {} is not at {commit}",
                    wt.display()
                ));
            }
            (wt, commit)
        }
        _ => {
            let wt = build_wt(run);
            git::worktree_add(&repo, &wt, Some(&l.build_branch), &l.lock_commit)?;
            let base = git::head(&wt)?;
            (wt, base)
        }
    };
    let may = commands(run, &def.may_run);
    let rights = match def.kind {
        Kind::Checker => rights::checker(may),
        _ => rights::maker(may, protected(&l)),
    };
    let session = match &step.resume {
        Some(s) if !run.state.lost_sessions.contains(s) => Session::Resume(s.clone()),
        _ => Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?),
    };
    let runtime_def = ctx
        .settings
        .agents
        .get(&def.runtime)
        .cloned()
        .ok_or_else(|| format!("{}'s runtime {:?} is not configured", def.name, def.runtime))?;
    let remaining = (run.state.limits.usd - run.state.cost_usd).max(0.01);
    let budget = def.limits.usd.map_or(remaining, |u| u.min(remaining));
    let minutes = def.limits.minutes.unwrap_or(run.state.limits.step_minutes);
    // The step's process group, sampled for the memory budget (research S-19).
    let group = Arc::new(
        crate::platform::job::Group::new()
            .map_err(|e| format!("cannot create a process group: {e}"))?,
    );
    run.group = Some(Arc::clone(&group));
    let call = Call {
        instruction: {
            let mut text = machine::instruction_of(run, n);
            for note in notes_for(run, n, &defname) {
                text.push_str(&format!("\n\nA note from the engineer for this step:\n{note}"));
            }
            text
        },
        schema: agent::schemas::for_kind(step.kind),
        rights,
        mcp_config: None,
        session: session.clone(),
        model: Some(def.model.clone()).filter(|m| !m.is_empty()),
        budget_usd: Some(budget),
        cwd: worktree.clone(),
        time_limit: Duration::from_secs(minutes * 60),
        system: Some(def.body.clone()),
        partial: false,
        group: Some(Arc::clone(&group)),
    };
    run.append(Event::StepStarted {
        n,
        agent: def.name.clone(),
        model: def.model.clone(),
        session: session.id().to_string(),
        worktree,
        base,
    })?;
    if let Some(ph) = step.phase {
        if matches!(step.kind, StepKind::Build | StepKind::Repair)
            && l.phases
                .iter()
                .find(|p| p.spec.n == ph)
                .and_then(|p| p.session.clone())
                != Some(session.id().to_string())
        {
            run.append(Event::Phase {
                n: ph,
                state: if step.kind == StepKind::Repair {
                    PhaseState::Repairing
                } else {
                    PhaseState::Building
                },
                session: Some(session.id().to_string()),
                handover: None,
            })?;
        }
    }
    run.busy = Some(Busy::Step(n));
    run.stop = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&run.stop);
    let tx = ctx.tx.clone();
    let activity_path = run.steps_dir().join(format!("{n}.jsonl"));
    let id = run.id();
    let runtime = agent::claude::Claude::new(runtime_def.command, runtime_def.args);
    std::thread::Builder::new()
        .name(format!("step-{id}-{n}"))
        .spawn(move || {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&activity_path)
                .ok();
            let tx2 = tx.clone();
            let started = std::time::Instant::now();
            let mut on = |e: agent::Event| {
                let Some(mut a) = agent::activity_of(&e, started.elapsed().as_millis() as u64) else {
                    return;
                };
                a.at = super::time::unix_ms();
                if let Some(f) = file.as_mut() {
                    if let Ok(line) = serde_json::to_string(&a) {
                        let _ = writeln!(f, "{line}");
                    }
                }
                let _ = tx2.send(Msg::Activity {
                    run: id,
                    n,
                    line: a,
                });
            };
            let outcome = runtime.start(&call, &mut on, &stop);
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Step { n, outcome },
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// Records a finished step of the loop: checks its claim and ends with `verified`.
pub fn process(run: &mut Run, ctx: &Ctx, n: u32) -> Result<(), String> {
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    let verified = |run: &mut Run, accepted: bool, reasons: Vec<String>, commit: Option<String>| {
        run.append(Event::Verified {
            n,
            accepted,
            reasons,
            commit,
        })
    };
    if let Some(StepEnd::Failed { why }) = &step.end {
        if why.starts_with(INTERRUPTED) {
            return verified(
                run,
                false,
                vec![format!("{INTERRUPTED}resumed after Fallow started again")],
                None,
            );
        }
        if step.resume.is_some()
            && machine::activity_lines(run, n) == 0
            && why.to_lowercase().contains("no conversation found")
        {
            if let Some(s) = step.session.clone() {
                run.append(Event::SessionLost { session: s })?;
            }
            return verified(run, false, vec![format!("{SESSION_LOST}{why}")], None);
        }
        // A checker that did not finish gave no verdict: it is asked again, not repaired.
        if step.kind == StepKind::Review {
            return verified(run, false, vec![format!("{INCOMPLETE}the check did not finish: {why}")], None);
        }
        return verified(run, false, vec![why.clone()], None);
    }
    let Some(value) = step.result().cloned() else {
        return Ok(());
    };
    match step.kind {
        StepKind::Review => check_done(run, ctx, &step, &value),
        StepKind::Handover => handover_done(run, &step, &value),
        _ => work_done(run, &step, &value),
    }
}

/// The engineer's answer to a builder's question (FR-015): the same step again, in its session,
/// with the answer.
pub fn answered(run: &mut Run, last: &Step, text: &str) -> Result<(), String> {
    let session = last
        .session
        .clone()
        .filter(|s| !run.state.lost_sessions.contains(s));
    let what = last.part.as_deref().unwrap_or("the task");
    let instruction = match &session {
        Some(_) => format!(
            "Fallow step: {} (part {what})\n\nThe engineer answers your question:\n\n{text}\n\nContinue the same work in your worktree and return your result; Fallow commits your changes when the step ends.",
            last.kind.label()
        ),
        None => format!(
            "{}\n\nThe engineer answered a question about this:\n\n{text}",
            machine::instruction_of(run, last.n)
        ),
    };
    let def = last.def.clone().unwrap_or_else(|| "builder".into());
    queue(
        run,
        last.kind,
        last.part.as_deref(),
        last.task.as_deref(),
        last.phase,
        &def,
        session,
        "the engineer answered",
        instruction,
    )?;
    if let Some(t) = &last.task {
        run.append(Event::Task {
            id: t.clone(),
            state: TaskState::Current,
            commit: None,
            repairs: repairs_of(run, t),
        })?;
    }
    Ok(())
}

/// Commits an agent made itself are taken apart, so their changes are judged and committed by
/// Fallow like any other; a history rewound or rewritten is put back to `base`, and said so.
fn own_history(run: &Run, wt: &std::path::Path, base: &str) -> Result<Option<String>, String> {
    let head = git::head(wt)?;
    if head == base {
        return Ok(None);
    }
    if git::is_ancestor(&run.state.repo, base, &head) {
        git::reset_mixed(wt, base)?;
        return Ok(None);
    }
    git::reset_mixed(wt, base)?;
    git::discard_changes(wt)?;
    Ok(Some(
        "the agent moved the branch away from where the step started (Fallow put it back)".into(),
    ))
}

/// The locked documents changed between `base` and `commit`, besides Fallow's own tick of
/// `task` in tasks.md.
fn locked_changes(
    run: &Run,
    l: &LoopState,
    base: &str,
    commit: &str,
    task: Option<&str>,
) -> Vec<String> {
    let files = git::files_between(&run.state.repo, base, commit).unwrap_or_default();
    let tasks_md = format!("specs/{}/tasks.md", l.feature);
    files
        .into_iter()
        .filter(|f| is_protected(l, f))
        .filter(|f| {
            if *f != tasks_md {
                return true;
            }
            // Only Fallow's tick of this task may change tasks.md.
            let before = git::show(&run.state.repo, base, f)
                .ok()
                .flatten()
                .unwrap_or_default();
            let after = git::show(&run.state.repo, commit, f)
                .ok()
                .flatten()
                .unwrap_or_default();
            match task.and_then(|t| speckit::tasks::tick(&before, t)) {
                Some(ticked) => ticked != after,
                None => before != after,
            }
        })
        .collect()
}

fn work_done(run: &mut Run, step: &Step, value: &Value) -> Result<(), String> {
    let n = step.n;
    let l = lp(run)?;
    let summary = value["summary"].as_str().unwrap_or_default().to_string();
    let verified = |run: &mut Run, accepted: bool, reasons: Vec<String>, commit: Option<String>| {
        run.append(Event::Verified {
            n,
            accepted,
            reasons,
            commit,
        })
    };
    match value["status"].as_str() {
        Some("blocked") => {
            let mut q = value["question"].as_str().unwrap_or("").to_string();
            if q.trim().is_empty() {
                q = summary;
            }
            verified(run, false, vec![format!("{BLOCKED}{q}")], None)
        }
        Some("done") => {
            let Some(wt) = step.worktree.clone() else {
                return verified(
                    run,
                    false,
                    vec!["the step had no working copy".into()],
                    None,
                );
            };
            let base = step.base.clone().unwrap_or_else(|| l.lock_commit.clone());
            let history = own_history(run, &wt, &base)?;
            // A change to the locked documents is undone by Fallow first, so a repair can go on
            // from a clean state; the step is still refused (FR-009, false claims).
            let touched: Vec<String> = git::changes_all(&wt)
                .unwrap_or_default()
                .into_iter()
                .filter(|f| is_protected(&l, f))
                .collect();
            if !touched.is_empty() {
                git::restore_paths(&wt, &base, &touched)?;
            }
            // Fallow ticks the task in tasks.md, in the task's own commit (research S-8).
            if matches!(step.kind, StepKind::Build | StepKind::Repair) {
                if let Some(t) = &step.task {
                    let p = wt.join(format!("specs/{}/tasks.md", l.feature));
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        if let Some(ticked) = speckit::tasks::tick(&text, t) {
                            let _ = std::fs::write(&p, ticked);
                        }
                    }
                }
            }
            let what = step
                .task
                .clone()
                .or_else(|| step.part.clone())
                .unwrap_or_default();
            let msg = format!(
                "{} {what}: {}\n\nFallow loop r{}, step {n}",
                step.kind.label(),
                summary
                    .lines()
                    .next()
                    .unwrap_or("work")
                    .chars()
                    .take(72)
                    .collect::<String>(),
                run.id()
            );
            let commit = match git::commit_all(&wt, &msg, None) {
                Ok(Some(c)) => c,
                Ok(None) => git::head(&wt)?,
                Err(e) => {
                    return verified(
                        run,
                        false,
                        vec![format!("Fallow could not commit the step's changes: {e}")],
                        None,
                    );
                }
            };
            let mut reasons: Vec<String> = history.into_iter().collect();
            if !touched.is_empty() {
                reasons.push(format!(
                    "the locked documents were changed (Fallow undid it): {}",
                    touched.join(", ")
                ));
            }
            if !git::on_branch(&run.state.repo, &commit, &l.build_branch) {
                reasons.push(format!("commit {commit} is not on {}", l.build_branch));
            }
            let locked = locked_changes(run, &l, &base, &commit, step.task.as_deref());
            if !locked.is_empty() {
                reasons.push(format!(
                    "the locked documents were changed: {}",
                    locked.join(", ")
                ));
            }
            if step.kind == StepKind::Build {
                if let (Some(t), Some(claimed)) = (&step.task, value["task"].as_str()) {
                    if claimed != t {
                        reasons.push(format!("the step was given {t} but reports {claimed}"));
                    }
                }
            }
            let changed = git::files_between(&run.state.repo, &base, &commit).unwrap_or_default();
            let unchanged: Vec<String> = value["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
                .map(|f| f.replace('\\', "/").trim_start_matches("./").to_string())
                .filter(|f| !changed.iter().any(|c| c == f))
                .collect();
            if !unchanged.is_empty() {
                reasons.push(format!(
                    "files claimed but not changed: {}",
                    unchanged.join(", ")
                ));
            }
            if reasons.is_empty() {
                verified(run, true, Vec::new(), Some(commit))
            } else {
                verified(run, false, reasons, None)
            }
        }
        _ => verified(
            run,
            false,
            vec![format!(
                "the agent reported that it failed{}",
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {summary}")
                }
            )],
            None,
        ),
    }
}

fn handover_done(run: &mut Run, step: &Step, value: &Value) -> Result<(), String> {
    let n = step.n;
    let l = lp(run)?;
    let Some(phase) = step.phase else {
        return Err("a handover without its phase".into());
    };
    let path = handover_path(&l, phase);
    let wt = build_wt(run);
    let base = step.base.clone().unwrap_or(git::head(&wt)?);
    let mut reasons: Vec<String> = own_history(run, &wt, &base)?.into_iter().collect();
    if value["status"].as_str() != Some("done") {
        reasons.push("the handover step did not finish".to_string());
    }
    let text = std::fs::read_to_string(wt.join(&path)).unwrap_or_default();
    if text.trim().is_empty() {
        reasons.push(format!("{path} is missing or empty"));
    }
    for sec in ["## Built", "## Decisions", "## For the next phase"] {
        if !text.lines().any(|x| x.trim_end() == sec) {
            reasons.push(format!("{path} has no `{sec}` section"));
        }
    }
    let others: Vec<String> = git::changes_all(&wt)
        .unwrap_or_default()
        .into_iter()
        .filter(|f| *f != path)
        .collect();
    if !others.is_empty() {
        reasons.push(format!(
            "the handover step changed other files: {}",
            others.join(", ")
        ));
        git::discard_changes(&wt)?;
    }
    if !reasons.is_empty() {
        return run.append(Event::Verified {
            n,
            accepted: false,
            reasons,
            commit: None,
        });
    }
    let commit = git::commit_all(
        &wt,
        &format!(
            "handover of phase {phase}\n\nFallow loop r{}, step {n}",
            run.id()
        ),
        Some(&[path.as_str()]),
    )?
    .unwrap_or(git::head(&wt)?);
    run.append(Event::Verified {
        n,
        accepted: true,
        reasons: Vec::new(),
        commit: Some(commit),
    })
}

/// A checker's verdict (FR-012): every scenario answered with what was run, must-fail cases
/// shown, nothing changed in its worktree, no refused commands, no high finding.
fn check_done(run: &mut Run, ctx: &Ctx, step: &Step, value: &Value) -> Result<(), String> {
    let n = step.n;
    let l = lp(run)?;
    let story = step.part.clone().unwrap_or_default();
    let spec_text = doc_at(run, &l.lock_commit, "spec.md").unwrap_or_default();
    let spec = speckit::spec::read("spec.md", &spec_text).unwrap_or_default();
    // Only the checker that answers the scenarios answers them; the others report findings.
    let lib = library(run, ctx);
    let answers_scenarios = pipeline_of(run, &lib)
        .ok()
        .and_then(|pl| scenario_checker(&pl, &lib))
        .is_some_and(|a| step.def.as_deref() == Some(a.as_str()));
    let scenarios = spec
        .story(&story)
        .filter(|_| answers_scenarios)
        .map(|s| s.scenarios.clone())
        .unwrap_or_default();
    let answers: Vec<Answer> = value["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| serde_json::from_value(a.clone()).ok())
        .collect();
    let mut missing = Vec::new();
    let mut failed = Vec::new();
    for sc in &scenarios {
        match answers.iter().find(|a| a.criterion == sc.id) {
            None => missing.push(format!("{}: no answer", sc.id)),
            Some(a) if a.ran.trim().is_empty() || a.result.trim().is_empty() => {
                missing.push(format!("{}: what was run and its result", sc.id))
            }
            Some(a)
                if sc.must_fail
                    && a.failing_case
                        .as_deref()
                        .is_none_or(|c| c.trim().is_empty()) =>
            {
                missing.push(format!("{}: the case that must fail", sc.id))
            }
            Some(a) if !a.passed => {
                failed.push(format!("{} failed: ran {} → {}", sc.id, a.ran, a.result))
            }
            Some(_) => {}
        }
    }
    for f in value["findings"].as_array().into_iter().flatten() {
        let sev = f["severity"].as_str().unwrap_or("low");
        if matches!(sev, "high" | "critical") {
            if f["ran"].as_str().is_none_or(|r| r.trim().is_empty()) {
                missing.push(format!("a {sev} finding without what was run"));
            } else {
                failed.push(format!(
                    "{sev} finding at {}: {}",
                    f["where"].as_str().unwrap_or("?"),
                    f["result"].as_str().unwrap_or("")
                ));
            }
        }
    }
    if !step.denials.is_empty() {
        missing.push(format!(
            "checks by running: these commands were refused: {}",
            step.denials.join(" | ")
        ));
    }
    if let (Some(wt), Some(base)) = (&step.worktree, &step.base) {
        if !git::unchanged(wt, base).unwrap_or(false) {
            missing.push("a check that changes nothing: the checker's worktree changed".into());
        }
    }
    let complete = missing.is_empty();
    let approved = complete && failed.is_empty();
    run.append(Event::Verdict {
        step: n,
        answers,
        complete,
        approved,
        missing: missing.clone(),
    })?;
    if let Some(wt) = &step.worktree {
        let _ = git::worktree_remove(&run.state.repo, wt);
    }
    let mut reasons = failed;
    reasons.extend(
        missing
            .into_iter()
            .map(|m| format!("{INCOMPLETE}{m}")),
    );
    run.append(Event::Verified {
        n,
        accepted: approved,
        reasons,
        commit: None,
    })
}
