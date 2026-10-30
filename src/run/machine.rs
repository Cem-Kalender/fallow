//! The stage machine (006 tasks T026, T027, T031 to T034): what a run does next.
//!
//! Results are recorded first (`process` ends with a `verified` event); what follows from them is
//! derived on every pass from the folded state, and each decision begins with the one event that
//! settles it (a queued step, a stage, a question to the engineer). A Fallow that stopped between
//! two events therefore makes the same decision again, never a second one (constitution VII).
//! Agent steps, gates and merges run on threads and come back through `on_done`.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::engine::{Busy, Ctx, Done, Msg, Run};
use super::events::*;
use super::state::Step;
use crate::agent::{self, Call, Runtime, Session, prompts, rights};
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

/// How many reviews a round may take before the engineer is asked.
const REVIEW_TRIES: usize = 3;

/// Markers at the start of a `verified` reason that the next pass acts on.
pub const BLOCKED: &str = "blocked: ";
pub const INTERRUPTED: &str = "interrupted: ";
pub const SESSION_LOST: &str = "session lost: ";
/// The start of a question raised because Fallow itself could not go on (a transient error).
pub const COULD_NOT_GO_ON: &str = "Fallow could not go on: ";
pub(crate) const INTERRUPTED_WHY: &str = "interrupted: Fallow stopped while the step ran";
const REVIEW_GAVE_NOTHING: &str = "The review did not give a usable verdict";
const RETRY_REVIEW: &str = "retry review";

pub(crate) fn gate_defs(run: &Run) -> Vec<GateDef> {
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
        task: None,
        phase: None,
        def: None,
    })?;
    Ok(n)
}

pub(crate) fn instruction_of(run: &Run, n: u32) -> String {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.prompt"))).unwrap_or_default()
}

pub(crate) fn activity_lines(run: &Run, n: u32) -> usize {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.jsonl")))
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

fn context<'a>(run: &'a Run, plan: Option<&'a plan::Plan>) -> prompts::Context<'a> {
    prompts::Context {
        task: &run.state.task,
        plan,
        gates: &run.state.gates,
        spec_kit: spec_kit(run),
    }
}

/// Repair rounds a part has had: its repair steps (FR-008), without those that were cut short
/// by Fallow stopping or by a lost session, which are taken up again by the next step.
pub fn repairs(run: &Run, part: &str) -> u32 {
    run.state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Repair && s.part.as_deref() == Some(part))
        .filter(|s| !cut_short(s))
        .count() as u32
}

/// A step cut short by Fallow stopping or by a lost session: the next step takes it up again,
/// so it is no attempt of its own.
pub(crate) fn cut_short(s: &Step) -> bool {
    first_reason(s).is_some_and(|r| r.starts_with(INTERRUPTED) || r.starts_with(SESSION_LOST))
}

/// The first planning turn: the plan branch and its worktree, and the planner's first draft.
pub fn start_planning(run: &mut Run) -> Result<(), String> {
    let id = run.id();
    let repo = run.state.repo.clone();
    git::create_branch(&repo, &plan_branch(id), &run.state.start_commit)?;
    let wt = run.worktrees().join("plan");
    git::worktree_add(&repo, &wt, Some(&plan_branch(id)), &run.state.start_commit)?;
    let text = prompts::plan_turn(&context(run, None), "", true);
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

/// After a restart (research R-2): steps that were running are recorded as interrupted and
/// resumed in their sessions on the next pass; a question that only a transient error raised is
/// cleared, so the run is tried again.
pub fn resume(run: &mut Run) -> Result<(), String> {
    // A stop recorded without its stage: the run is stopped.
    if run.state.stopped.is_some() {
        return run.append(Event::Stage {
            to: Stage::Stopped,
            why: "stopped by the engineer".into(),
        });
    }
    // A merge asked for and not recorded: it went through if the target holds the commit.
    let asked = run
        .state
        .steering
        .iter()
        .any(|s| s.kind == SteeringKind::Merge);
    if run.state.stage == Stage::Ready && asked && run.state.merged.is_none() {
        if let Some(c) = run.state.integrated.clone() {
            let target = format!("refs/heads/{}", run.state.target);
            if let Ok(tip) = git::rev(&run.state.repo, &target) {
                if git::is_ancestor(&run.state.repo, &c, &tip) {
                    run.append(Event::Merged {
                        commit: tip,
                        how: "local".into(),
                    })?;
                    return run.append(Event::Stage {
                        to: Stage::Done,
                        why: "merged by the engineer".into(),
                    });
                }
            }
        }
    }
    if let Some(n) = &run.state.needs {
        if n.why.starts_with(COULD_NOT_GO_ON) {
            let why = n.why.clone();
            run.append(Event::NeedsCleared { why })?;
        }
    }
    let running: Vec<u32> = run
        .state
        .steps
        .values()
        .filter(|s| s.running())
        .map(|s| s.n)
        .collect();
    for n in running {
        run.append(Event::StepFinished {
            n,
            end: StepEnd::Failed {
                why: INTERRUPTED_WHY.into(),
            },
            cost_usd: None,
            duration_ms: 0,
            denials: Vec::new(),
        })?;
    }
    Ok(())
}

pub(crate) fn pause_for_limit(run: &mut Run, which: String, used: String) -> Result<(), String> {
    run.append(Event::LimitReached {
        which: which.clone(),
        used: used.clone(),
    })?;
    if run.state.stage != Stage::Paused {
        run.append(Event::Stage {
            to: Stage::Paused,
            why: format!("the {which} limit is reached ({used})"),
        })?;
    }
    ask_limit(run, &which, &used)
}

fn ask_limit(run: &mut Run, which: &str, used: &str) -> Result<(), String> {
    run.append(Event::NeedsEngineer {
        why: format!(
            "The run's {which} limit is reached ({used}). Raise the limit, continue for one step, or stop the run."
        ),
        options: vec![
            "raise the limit".into(),
            "continue for one step".into(),
            "stop".into(),
        ],
    })
}

pub(crate) fn limit_hit(run: &Run) -> Option<(String, String)> {
    run.state.limit_hit(super::time::unix_now())
}

/// Decides and starts what comes next. Does nothing while something runs.
pub fn drive(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    if run.busy.is_some() || run.state.stage.is_final() || run.state.stopped.is_some() {
        return Ok(());
    }
    if run.state.stage == Stage::Paused {
        // A pause that lost its question (the engineer's action cleared it) asks again, or
        // ends when the limits allow the run to go on.
        if run.state.needs.is_none() {
            match limit_hit(run) {
                Some((w, u)) => ask_limit(run, &w, &u)?,
                None => {
                    let back = run.state.paused_from.unwrap_or(Stage::Building);
                    run.append(Event::Stage {
                        to: back,
                        why: "the limits allow the run to go on".into(),
                    })?;
                }
            }
        }
        return Ok(());
    }
    if run.state.needs.is_some() {
        return Ok(());
    }
    // 007: a feature's loop has its own machine (research S-9).
    if run.state.feature_loop.is_some() {
        return super::feature::drive(run, ctx);
    }
    if let Some(n) = unprocessed(run) {
        return process(run, n);
    }
    if requeue_last(run)? {
        return Ok(());
    }
    if let Some(n) = queued(run) {
        if let Some((w, u)) = limit_hit(run) {
            return pause_for_limit(run, w, u);
        }
        return start(run, ctx, n);
    }
    match run.state.stage {
        Stage::Planning => planning(run),
        Stage::Building => building(run, ctx),
        Stage::Reviewing => reviewing(run, ctx),
        _ => Ok(()),
    }
}

/// The engineer's last line has no reply and no turn is queued for it (Fallow stopped between
/// the two): queue the planner's turn.
fn planning(run: &mut Run) -> Result<(), String> {
    match run.state.conversation.last() {
        Some(t) if t.who == "engineer" => {
            let text = t.text.clone();
            queue_plan_turn(run, &text)
        }
        _ => Ok(()),
    }
}

/// The oldest queued step.
pub(crate) fn queued(run: &Run) -> Option<u32> {
    run.state.steps.values().find(|s| s.queued()).map(|s| s.n)
}

/// A finished step whose result has not been recorded as handled.
pub(crate) fn unprocessed(run: &Run) -> Option<u32> {
    run.state
        .steps
        .values()
        .find(|s| s.end.is_some() && s.verified.is_none())
        .map(|s| s.n)
}

pub(crate) fn first_reason(s: &Step) -> Option<&str> {
    match &s.verified {
        Some((false, r)) => r.first().map(String::as_str),
        _ => None,
    }
}

/// When the last step was interrupted, or its session was lost, and nothing followed it yet:
/// queue it again, resumed in its session, or in a new one.
fn requeue_last(run: &mut Run) -> Result<bool, String> {
    let Some(last) = run.state.steps.values().next_back().cloned() else {
        return Ok(false);
    };
    let Some(reason) = first_reason(&last).map(str::to_string) else {
        return Ok(false);
    };
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
                prompts::INTERRUPTED
            ),
            None => instruction_of(run, last.n),
        };
        queue(
            run,
            last.kind,
            last.part.as_deref(),
            session,
            "resumed after Fallow stopped",
            text,
        )?;
        return Ok(true);
    }
    if reason.starts_with(SESSION_LOST) {
        let text = fresh_instruction(run, &last)?;
        queue(
            run,
            last.kind,
            last.part.as_deref(),
            None,
            "a new session takes over a session that could not be resumed",
            text,
        )?;
        return Ok(true);
    }
    Ok(false)
}

/// The whole instruction for a new session taking over `last`'s work.
fn fresh_instruction(run: &mut Run, last: &Step) -> Result<String, String> {
    Ok(match last.kind {
        StepKind::PlanTurn | StepKind::Decide => {
            let said: Vec<String> = run
                .state
                .conversation
                .iter()
                .filter(|t| t.who == "engineer")
                .map(|t| t.text.clone())
                .collect();
            prompts::plan_turn(&context(run, None), &said.join("\n"), true)
        }
        StepKind::Review => {
            let commit = run.state.integrated.clone().unwrap_or_default();
            load_plan(run)?;
            prompts::review(&context(run, run.plan.as_ref()), &commit)
        }
        _ => {
            load_plan(run)?;
            let part = last.part.clone().unwrap_or_else(|| plan::MAIN.to_string());
            let branch = part_branch(run.id(), &part);
            let mut text = as_kind(
                prompts::build(&context(run, run.plan.as_ref()), &part, &branch),
                last.kind,
            );
            // The previous instruction still applies unless it was the whole build prompt (a
            // repair's reasons, the engineer's answer).
            let before = instruction_of(run, last.n);
            if !before.is_empty() && !before.contains("\nTask: ") {
                text.push_str(
                    "\n\nFallow's last instruction for this part, which still applies:\n\n",
                );
                text.push_str(&before);
            }
            text
        }
    })
}

/// A build instruction headed as a step of `kind`, for a new session doing a repair.
fn as_kind(text: String, kind: StepKind) -> String {
    text.replacen(
        "Fallow step: build",
        &format!("Fallow step: {}", kind.label()),
        1,
    )
}

fn building(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    load_plan(run)?;
    // The baseline, once, before anything is judged against it.
    let missing_baseline: Vec<GateDef> = gate_defs(run)
        .into_iter()
        .filter(|g| g.baseline && run.state.baseline(&g.name).is_none())
        .collect();
    if !missing_baseline.is_empty() {
        let commit = run.state.start_commit.clone();
        return run_gates(run, ctx, GateKind::Baseline, None, commit, missing_baseline);
    }
    let Some(plan) = run.plan.clone() else {
        return Err("the run is building without an approved plan".into());
    };
    if plan.build == Build::Parts {
        return Err("parallel parts arrive with user story 5".into());
    }
    for p in &plan.parts {
        if run
            .state
            .parts
            .get(&p.id)
            .is_some_and(|s| s.state == PartState::Done)
        {
            continue;
        }
        let last = run
            .state
            .steps
            .values()
            .rev()
            .find(|s| s.part.as_deref() == Some(p.id.as_str()))
            .cloned();
        let Some(last) = last else {
            // Nothing built yet.
            run.append(Event::Part {
                id: p.id.clone(),
                state: PartState::Building,
                result_commit: None,
                repairs: 0,
            })?;
            let text = prompts::build(
                &context(run, Some(&plan)),
                &p.id,
                &part_branch(run.id(), &p.id),
            );
            let n = queue(
                run,
                StepKind::Build,
                Some(&p.id),
                None,
                "build the part",
                text,
            )?;
            return start_unless_limited(run, ctx, n);
        };
        match &last.verified {
            None => return Ok(()),
            Some((false, reasons)) => {
                let reasons = reasons.clone();
                return refused(run, &p.id, reasons);
            }
            Some((true, _)) => {
                let Some(c) = last.verified_commit.clone() else {
                    return Err(format!("step {} was accepted without a commit", last.n));
                };
                let defs = gate_defs(run);
                let missing: Vec<GateDef> = defs
                    .iter()
                    .filter(|d| {
                        !run.state.gate_runs.iter().any(|g| {
                            g.kind == GateKind::Part
                                && g.part.as_deref() == Some(p.id.as_str())
                                && g.result.commit == c
                                && g.result.gate == d.name
                        })
                    })
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    return run_gates(run, ctx, GateKind::Part, Some(p.id.clone()), c, missing);
                }
                let failed: Vec<String> = run
                    .state
                    .gate_runs
                    .iter()
                    .filter(|g| {
                        g.kind == GateKind::Part
                            && g.part.as_deref() == Some(p.id.as_str())
                            && g.result.commit == c
                            && !g.class.is_some_and(|k| k.ok())
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
                    .collect();
                if failed.is_empty() {
                    let r = repairs(run, &p.id);
                    run.append(Event::Part {
                        id: p.id.clone(),
                        state: PartState::Done,
                        result_commit: Some(c),
                        repairs: r,
                    })?;
                    continue;
                }
                return reject(run, ctx, &p.id, failed).map(|_| ());
            }
        }
    }
    // Every part is done: integrate. With one builder the integrated result is its commit.
    let main = plan
        .parts
        .first()
        .and_then(|p| run.state.parts.get(&p.id))
        .and_then(|s| s.result_commit.clone())
        .ok_or("the part is done without a commit")?;
    if run.state.integrated.as_deref() != Some(main.as_str())
        || run.state.integrated_seq < last_repair_seq(run)
    {
        git::set_branch(&run.state.repo, &integration_branch(run.id()), &main)?;
        run.append(Event::Integrated { commit: main })?;
    }
    run.append(Event::Stage {
        to: Stage::Reviewing,
        why: "the work is verified and integrated".into(),
    })?;
    reviewing(run, ctx)
}

/// The event number of the latest repair step, so a repaired result is integrated again even
/// when it equals the earlier commit.
fn last_repair_seq(run: &Run) -> u64 {
    run.state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Repair)
        .map(|s| s.queued_seq)
        .max()
        .unwrap_or(0)
}

/// What follows from a part's last step that was not accepted: the engineer's answer when the
/// builder asked something, else a repair within the cap.
fn refused(run: &mut Run, part: &str, reasons: Vec<String>) -> Result<(), String> {
    let first = reasons.first().cloned().unwrap_or_default();
    if let Some(q) = first.strip_prefix(BLOCKED) {
        run.append(Event::Part {
            id: part.to_string(),
            state: PartState::Blocked,
            result_commit: None,
            repairs: repairs(run, part),
        })?;
        return run.append(Event::NeedsEngineer {
            why: format!("The builder of part {part} asks: {q}"),
            options: vec!["answer".into(), "stop".into()],
        });
    }
    reject_now(run, part, reasons).map(|_| ())
}

/// A part's work was not accepted: a repair within the cap, else the engineer (FR-008). The
/// queued repair step is what settles it. `true` when a repair was queued.
fn reject(run: &mut Run, _ctx: &Ctx, part: &str, reasons: Vec<String>) -> Result<bool, String> {
    reject_now(run, part, reasons)
}

fn reject_now(run: &mut Run, part: &str, reasons: Vec<String>) -> Result<bool, String> {
    let done = repairs(run, part);
    if done >= run.state.repair_cap {
        run.append(Event::Part {
            id: part.to_string(),
            state: PartState::AtCap,
            result_commit: None,
            repairs: done,
        })?;
        run.append(Event::NeedsEngineer {
            why: format!(
                "Part {part} still fails after {done} repair round(s): {}. Allow one more repair, or stop the run.",
                reasons.join("; ")
            ),
            options: vec!["allow one more repair".into(), "stop".into()],
        })?;
        return Ok(false);
    }
    let session = run.state.session_of("builder", Some(part));
    let text = match &session {
        Some(_) => prompts::repair(part, &reasons, &part_branch(run.id(), part)),
        None => {
            load_plan(run)?;
            let mut t = as_kind(
                prompts::build(
                    &context(run, run.plan.as_ref()),
                    part,
                    &part_branch(run.id(), part),
                ),
                StepKind::Repair,
            );
            t.push_str("\n\n");
            t.push_str(&prompts::repair(
                part,
                &reasons,
                &part_branch(run.id(), part),
            ));
            t
        }
    };
    queue(
        run,
        StepKind::Repair,
        Some(part),
        session,
        &format!("repair round {} of {}", done + 1, run.state.repair_cap),
        text,
    )?;
    run.append(Event::Part {
        id: part.to_string(),
        state: PartState::Repairing,
        result_commit: None,
        repairs: done + 1,
    })?;
    if run.state.stage == Stage::Reviewing {
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("part {part} needs a repair"),
        })?;
    }
    Ok(true)
}

fn start_unless_limited(run: &mut Run, ctx: &Ctx, n: u32) -> Result<(), String> {
    if let Some((w, u)) = limit_hit(run) {
        return pause_for_limit(run, w, u);
    }
    start(run, ctx, n)
}

/// The review steps of the current round: after the last integration, and after the engineer
/// last asked to review again.
fn round_reviews(run: &Run) -> Vec<Step> {
    let since = run.state.integrated_seq.max(last_retry_seq(run));
    run.state
        .steps
        .values()
        .filter(|s| s.kind == StepKind::Review && s.queued_seq > since)
        .cloned()
        .collect()
}

fn last_retry_seq(run: &Run) -> u64 {
    run.state
        .steering
        .iter()
        .filter(|s| s.kind == SteeringKind::Resume && s.detail.as_deref() == Some(RETRY_REVIEW))
        .map(|s| s.seq)
        .max()
        .unwrap_or(0)
}

fn reviewing(run: &mut Run, ctx: &Ctx) -> Result<(), String> {
    load_plan(run)?;
    let reviews = round_reviews(run);
    let Some(last) = reviews.last().cloned() else {
        let n = queue_fresh_review(run)?;
        return start_unless_limited(run, ctx, n);
    };
    let tries = reviews.iter().filter(|s| !cut_short(s)).count();
    match &last.verified {
        None => Ok(()),
        Some((false, reasons)) => {
            if tries >= REVIEW_TRIES {
                return ask_review_again(run, tries, &reasons.join("; "));
            }
            let n = queue_fresh_review(run)?;
            start_unless_limited(run, ctx, n)
        }
        Some((true, _)) => {
            let Some(v) = run
                .state
                .verdicts
                .iter()
                .rev()
                .find(|v| v.step == last.n)
                .cloned()
            else {
                return Err(format!("review step {} has no verdict", last.n));
            };
            if v.approved {
                remove_review_worktrees(run, &reviews);
                return run.append(Event::Stage {
                    to: Stage::Ready,
                    why: "the review approved every criterion".into(),
                });
            }
            if v.complete {
                // The criteria of the plan that failed, by part, each from its latest counted
                // answer of the round.
                let all = counted_answers(run, &reviews);
                let plan = run.plan.clone().ok_or("no approved plan")?;
                let expected: Vec<(String, bool)> = plan
                    .criteria()
                    .into_iter()
                    .map(|(k, c)| (k, c.must_fail))
                    .collect();
                let mut by_part: std::collections::BTreeMap<String, Vec<String>> =
                    Default::default();
                for key in verdict::check(&all, &expected).failed {
                    let Some(a) = all.iter().rev().find(|a| a.criterion == key) else {
                        continue;
                    };
                    let part = key.split('/').next().unwrap_or(plan::MAIN).to_string();
                    by_part.entry(part).or_default().push(format!(
                        "{} failed review: ran {}; got {}",
                        a.criterion, a.ran, a.result
                    ));
                }
                remove_review_worktrees(run, &reviews);
                let mut repairing = false;
                for (part, reasons) in by_part {
                    // Already settled for this part (a repair queued after this review).
                    let settled = run
                        .state
                        .steps
                        .values()
                        .any(|s| s.part.as_deref() == Some(part.as_str()) && s.n > last.n);
                    repairing |= settled || reject(run, ctx, &part, reasons)?;
                }
                // Back to building only for a repair; a part at its cap waits here for the
                // engineer (found in review: going back without one looped on every pass).
                if repairing && run.state.stage == Stage::Reviewing {
                    run.append(Event::Stage {
                        to: Stage::Building,
                        why: "the review found failing criteria".into(),
                    })?;
                }
                return Ok(());
            }
            if tries >= REVIEW_TRIES {
                return ask_review_again(run, tries, &v.missing.join("; "));
            }
            let session = last
                .session
                .clone()
                .filter(|s| !run.state.lost_sessions.contains(s));
            let n = match session {
                Some(s) => queue(
                    run,
                    StepKind::Review,
                    None,
                    Some(s),
                    &format!("the verdict is incomplete: {}", v.missing.join("; ")),
                    prompts::review_missing(&v.missing),
                )?,
                None => queue_fresh_review(run)?,
            };
            start_unless_limited(run, ctx, n)
        }
    }
}

fn ask_review_again(run: &mut Run, tries: usize, why: &str) -> Result<(), String> {
    run.append(Event::NeedsEngineer {
        why: format!("{REVIEW_GAVE_NOTHING} after {tries} tries: {why}."),
        options: vec!["retry".into(), "stop".into()],
    })
}

fn remove_review_worktrees(run: &Run, reviews: &[Step]) {
    for r in reviews {
        if let Some(wt) = &r.worktree {
            let _ = git::worktree_remove(&run.state.repo, wt);
        }
    }
}

/// The answers of a round's reviews that count: not from a review whose commands were refused
/// (found on a real run) or whose worktree changed (recorded as not accepted).
fn counted_answers(run: &Run, reviews: &[Step]) -> Vec<Answer> {
    run.state
        .verdicts
        .iter()
        .filter(|v| {
            reviews.iter().any(|r| {
                r.n == v.step && r.denials.is_empty() && matches!(r.verified, Some((true, _)))
            })
        })
        .flat_map(|v| v.answers.clone())
        .collect()
}

fn queue_fresh_review(run: &mut Run) -> Result<u32, String> {
    let commit = run
        .state
        .integrated
        .clone()
        .ok_or("nothing is integrated")?;
    let text = prompts::review(&context(run, run.plan.as_ref()), &commit);
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
        StepKind::Build
        | StepKind::Repair
        | StepKind::Take
        | StepKind::Plain
        | StepKind::Handover
        | StepKind::Make => {
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
            // A resumed review keeps its worktree, so its earlier checks still apply.
            let prev = step.resume.as_ref().and_then(|_| {
                run.state
                    .steps
                    .values()
                    .rev()
                    .find(|s| s.kind == StepKind::Review && s.n < n && s.worktree.is_some())
                    .and_then(|s| s.worktree.clone())
            });
            let wt = match prev.filter(|p| p.join(".git").exists()) {
                Some(p) => p,
                None => {
                    let wt = run.worktrees().join(format!("review-{n}"));
                    let _ = git::worktree_remove(&repo, &wt);
                    wt
                }
            };
            git::worktree_add(&repo, &wt, None, &commit)?;
            if git::head(&wt)? != commit {
                return Err(format!(
                    "the review worktree {} is not at {}",
                    wt.display(),
                    claims::short(&commit)
                ));
            }
            (wt, commit)
        }
    };
    if let Some(p) = &step.part {
        if matches!(step.kind, StepKind::Build | StepKind::Repair) {
            let state = if step.kind == StepKind::Repair {
                PartState::Repairing
            } else {
                PartState::Building
            };
            if run.state.parts.get(p).map(|s| s.state) != Some(state) {
                let r = repairs(run, p);
                run.append(Event::Part {
                    id: p.clone(),
                    state,
                    result_commit: None,
                    repairs: r,
                })?;
            }
        }
    }
    let choice = run.state.roles.get(role).cloned().unwrap_or_default();
    let def = ctx
        .settings
        .agents
        .get(&choice.agent)
        .cloned()
        .ok_or_else(|| format!("no agent named {:?} in the settings", choice.agent))?;
    // The squad file's `[allow]` lists hold rules (`Bash(npm run lint:*)`); rights take prefixes.
    let mut commands: Vec<String> = run
        .state
        .allow
        .get(role)
        .into_iter()
        .flatten()
        .filter_map(|r| rights::prefix_of_rule(r))
        .collect();
    if role != "planner" {
        load_plan(run)?;
        commands.extend(run.state.gates.iter().map(|g| g.command.clone()));
        if let Some(p) = &run.plan {
            commands.extend(p.commands.iter().cloned());
        }
    }
    let rights = match role {
        "planner" => rights::Rights {
            edits: rights::Edits::Only(rights::PLAN_GLOBS.iter().map(|s| s.to_string()).collect()),
            ..rights::checker(commands)
        },
        "reviewer" => rights::checker(commands),
        _ => rights::maker(commands, Vec::new()),
    };
    let session = match &step.resume {
        Some(s) if !run.state.lost_sessions.contains(s) => Session::Resume(s.clone()),
        _ => Session::New(crate::platform::random::uuid_v4().map_err(|e| e.to_string())?),
    };
    let remaining = (run.state.limits.usd - run.state.cost_usd).max(0.01);
    let runtime = agent::claude::Claude::new(def.command.clone(), def.args.clone());
    let call = Call {
        instruction: instruction_of(run, n),
        schema: agent::schemas::for_kind(step.kind),
        rights,
        mcp_config: None,
        session: session.clone(),
        model: Some(choice.model.clone()).filter(|m| !m.is_empty()),
        budget_usd: Some(remaining),
        cwd: worktree.clone(),
        time_limit: Duration::from_secs(run.state.limits.step_minutes * 60),
        system: Some(prompts::system(role)),
        partial: false,
        group: None,
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
            let started = std::time::Instant::now();
            let mut on = |e: agent::Event| {
                let Some(a) = agent::activity_of(&e, started.elapsed().as_millis() as u64) else {
                    return;
                };
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
            let outcome = runtime.start(&call, &mut on, &stop);
            let _ = tx.send(Msg::Done {
                run: run_id,
                done: Done::Step { n, outcome },
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// Runs gates on `commit` in a fresh worktree of its own, on a thread. The worktree is checked
/// to be at `commit` before anything runs (found in review: a worktree left behind by a failed
/// removal must never stand in for another commit).
pub(crate) fn run_gates(
    run: &mut Run,
    ctx: &Ctx,
    kind: GateKind,
    part: Option<String>,
    commit: String,
    defs: Vec<GateDef>,
) -> Result<(), String> {
    let repo = run.state.repo.clone();
    // Gate worktrees left behind by a removal that failed (a locked file on Windows).
    if let Ok(entries) = std::fs::read_dir(run.worktrees()) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("gate-") {
                let _ = git::worktree_remove(&repo, &e.path());
            }
        }
    }
    run.busy = Some(Busy::Gates);
    run.stop = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&run.stop);
    let wt = run
        .worktrees()
        .join(format!("gate-{}", super::time::unix_ms()));
    let tx = ctx.tx.clone();
    let id = run.id();
    let spawned = std::thread::Builder::new()
        .name(format!("gates-{id}"))
        .spawn(move || {
            let mut results = Vec::new();
            let mut error = None;
            let ready =
                git::worktree_add(&repo, &wt, None, &commit).and_then(|()| match git::head(&wt) {
                    Ok(h) if h == commit => Ok(()),
                    Ok(h) => Err(format!("the gate worktree is at {h}, not at {commit}")),
                    Err(e) => Err(e),
                });
            match ready {
                Ok(()) => {
                    for d in &defs {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let mut ignore = |_: &str| {};
                        let r = gates::run(d, &wt, &commit, &mut ignore, &stop);
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        results.push(r);
                    }
                }
                Err(e) => error = Some(format!("cannot prepare the gate worktree: {e}")),
            }
            let _ = git::worktree_remove(&repo, &wt);
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Gates {
                    kind,
                    part,
                    results,
                    error,
                },
            });
        });
    if let Err(e) = spawned {
        run.busy = None;
        return Err(format!("cannot start a thread: {e}"));
    }
    Ok(())
}

/// Records what a worker reports; what follows is decided by the next pass of `drive`.
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
            if run.state.feature_loop.is_some() {
                return super::feature::process(run, n);
            }
            process(run, n)
        }
        Done::Gates {
            kind,
            part,
            results,
            error,
        } => {
            if run.state.stage.is_final() {
                return Ok(());
            }
            if let Some(e) = error {
                return run.append(Event::NeedsEngineer {
                    why: format!("{COULD_NOT_GO_ON}{e}"),
                    options: vec!["retry".into(), "stop".into()],
                });
            }
            // Gates that did not finish (stopped) are not recorded; the next pass runs them.
            for r in results {
                let class = if kind == GateKind::Baseline {
                    None
                } else {
                    Some(gates::classify(&r, run.state.baseline(&r.gate)))
                };
                run.append(Event::GateRun {
                    kind,
                    part: part.clone(),
                    result: r,
                    class,
                })?;
            }
            Ok(())
        }
        Done::Merge(r) => match r {
            // A merge that went through is recorded whatever the stage: it happened.
            Ok(commit) => {
                run.append(Event::Merged {
                    commit,
                    how: "local".into(),
                })?;
                if run.state.stage.is_final() {
                    return Ok(());
                }
                run.append(Event::Stage {
                    to: Stage::Done,
                    why: "merged by the engineer".into(),
                })
            }
            Err(e) => run.append(Event::NeedsEngineer {
                why: format!("The merge did not go through: {e}"),
                options: vec!["merge again".into(), "stop".into()],
            }),
        },
    }
}

/// Records a finished step's result; ends with a `verified` event, which marks it handled.
fn process(run: &mut Run, n: u32) -> Result<(), String> {
    let step = run.state.steps.get(&n).cloned().ok_or("no such step")?;
    if let Some(StepEnd::Failed { why }) = &step.end {
        if why.starts_with(INTERRUPTED) {
            return run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![format!("{INTERRUPTED}resumed after Fallow started again")],
                commit: None,
            });
        }
        // A resumed session that Claude Code does not know any more, and that did nothing: a new
        // session takes over (T034), and the failure costs no repair. Other failures (an
        // overloaded API, the budget) stay failures of the step.
        if step.resume.is_some()
            && activity_lines(run, n) == 0
            && why.to_lowercase().contains("no conversation found")
        {
            if let Some(s) = step.session.clone() {
                run.append(Event::SessionLost { session: s })?;
            }
            return run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![format!("{SESSION_LOST}{why}")],
                commit: None,
            });
        }
    }
    match step.kind {
        StepKind::PlanTurn | StepKind::Decide => plan_turn_done(run, &step),
        StepKind::Build
        | StepKind::Repair
        | StepKind::Take
        | StepKind::Plain
        | StepKind::Handover
        | StepKind::Make => build_done(run, &step),
        StepKind::Review => review_done(run, &step),
    }
}

fn plan_turn_done(run: &mut Run, step: &Step) -> Result<(), String> {
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
    if !run.state.conversation.iter().any(|t| t.step == Some(n)) {
        run.append(Event::PlanReply {
            step: n,
            reply,
            ready,
        })?;
    }
    // Fallow commits the plan's files the planner changed, and nothing else (research R-4).
    if let Some(wt) = &step.worktree {
        let msg = format!("Plan (Fallow run r{id}, planning turn {n})");
        let paths: Vec<&str> = crate::agent::rights::PLAN_DIRS.to_vec();
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
            Ok(Some(p)) => {
                if p.build == Build::Parts {
                    problems.push(
                        "parallel parts arrive with user story 5: plan this with one builder for now"
                            .into(),
                    );
                }
                problems.extend(plan::approval_problems(
                    &repo,
                    &p,
                    &tip,
                    &run.state.test_patterns,
                ));
            }
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

/// Where the files a part may change are counted from: its last accepted commit, else the
/// approved plan. A commit that was refused stays counted until a later one is accepted
/// (found in review: checking from each step's own start let a refused change through).
fn part_base(run: &Run, part: &str, before: u32) -> String {
    run.state
        .steps
        .values()
        .rev()
        .filter(|s| s.part.as_deref() == Some(part) && s.n < before)
        .find_map(|s| match &s.verified {
            Some((true, _)) => s.verified_commit.clone(),
            _ => None,
        })
        .or_else(|| run.state.approved_plan.clone())
        .unwrap_or_else(|| run.state.start_commit.clone())
}

fn build_done(run: &mut Run, step: &Step) -> Result<(), String> {
    let n = step.n;
    let part = step.part.clone().unwrap_or_else(|| plan::MAIN.to_string());
    let verified = |run: &mut Run, accepted: bool, reasons: Vec<String>, commit: Option<String>| {
        run.append(Event::Verified {
            n,
            accepted,
            reasons,
            commit,
        })
    };
    let value = match &step.end {
        Some(StepEnd::Result { value }) => value.clone(),
        Some(StepEnd::Failed { why }) => return verified(run, false, vec![why.clone()], None),
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
            verified(run, false, vec![format!("{BLOCKED}{q}")], None)
        }
        Some("done") => {
            load_plan(run)?;
            let plan = run.plan.clone().ok_or("no approved plan")?;
            // Fallow commits what the step changed (research R-4): agents need no git rights.
            if let Some(wt) = &step.worktree {
                let msg = format!(
                    "{} {part}: {}\n\nFallow run r{}, step {n}",
                    step.kind.label(),
                    first_line(&summary, "work"),
                    run.id()
                );
                if let Err(e) = git::commit_all(wt, &msg, None) {
                    return verified(
                        run,
                        false,
                        vec![format!("Fallow could not commit the step's changes: {e}")],
                        None,
                    );
                }
            }
            let takes: Vec<String> = Vec::new();
            let step_base = step.base.clone().unwrap_or_default();
            let files_base = part_base(run, &part, n);
            match claims::check_build(
                &run.state.repo,
                &part_branch(run.id(), &part),
                &step_base,
                &files_base,
                &value,
                &plan,
                &part,
                &takes,
            ) {
                Ok(commit) => verified(run, true, Vec::new(), Some(commit)),
                Err(why) => verified(run, false, why, None),
            }
        }
        _ => verified(
            run,
            false,
            vec![format!(
                "the builder reported that it failed{}",
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

fn review_done(run: &mut Run, step: &Step) -> Result<(), String> {
    let n = step.n;
    let value = match &step.end {
        Some(StepEnd::Result { value }) => value.clone(),
        Some(StepEnd::Failed { why }) => {
            return run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![format!("the review did not finish: {why}")],
                commit: None,
            });
        }
        None => return Ok(()),
    };
    let integrated = run.state.integrated.clone().unwrap_or_default();
    if let Some(wt) = &step.worktree {
        if !git::unchanged(wt, &integrated).unwrap_or(false) {
            let _ = git::worktree_remove(&run.state.repo, wt);
            return run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec![
                    "the reviewer changed its worktree, so its verdict does not count".into(),
                ],
                commit: None,
            });
        }
    }
    load_plan(run)?;
    let plan = run.plan.clone().ok_or("no approved plan")?;
    let expected: Vec<(String, bool)> = plan
        .criteria()
        .into_iter()
        .map(|(k, c)| (k, c.must_fail))
        .collect();
    // Every counted partial verdict of this round adds up (FR-007).
    let reviews: Vec<Step> = round_reviews(run).into_iter().filter(|s| s.n < n).collect();
    let mut all = counted_answers(run, &reviews);
    let mine = verdict::answers(&value);
    if step.denials.is_empty() {
        all.extend(mine.iter().cloned());
    }
    let mut check = verdict::check(&all, &expected);
    // A review whose commands were refused did not check by running: it does not count, and the
    // reviewer is resumed with what it may run (found on real runs, 2026-10-29).
    if !step.denials.is_empty() {
        check.complete = false;
        check.approved = false;
        check.missing = vec![refused_note(&step.denials, &plan, &run.state.gates)];
    }
    if !run.state.verdicts.iter().any(|v| v.step == n) {
        run.append(Event::Verdict {
            step: n,
            answers: mine,
            complete: check.complete,
            approved: check.approved,
            missing: check.missing.clone(),
        })?;
    }
    run.append(Event::Verified {
        n,
        accepted: true,
        reasons: Vec::new(),
        commit: None,
    })
}

/// What the reviewer is told about refused commands: chains are refused as a whole, and only
/// the gates and the plan's commands may run.
fn refused_note(denials: &[String], plan: &plan::Plan, gates: &[GateSpec]) -> String {
    let mut may: Vec<String> = gates.iter().map(|g| g.command.clone()).collect();
    may.extend(plan.commands.iter().cloned());
    format!(
        "every check again, by running: these commands were refused: {}. Run one command per call, without `;`, `&&` or `|`. Besides reading files, you may run only these commands, with any arguments: {}",
        denials.join(" | "),
        if may.is_empty() {
            "none".to_string()
        } else {
            may.join(", ")
        }
    )
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
    queue_plan_turn(run, text.trim())
}

/// Queues the planner's turn answering the engineer's `text`.
fn queue_plan_turn(run: &mut Run, text: &str) -> Result<(), String> {
    let session = run.state.session_of("planner", None);
    let first = session.is_none();
    let said = if first {
        run.state
            .conversation
            .iter()
            .filter(|t| t.who == "engineer")
            .map(|t| t.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.to_string()
    };
    let instruction = prompts::plan_turn(&context(run, None), &said, first);
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
    if run.busy.is_some() || queued(run).is_some() {
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
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if text.trim().is_empty() {
        return Err("the answer is empty".into());
    }
    let Some(part) = run
        .state
        .parts
        .values()
        .find(|p| p.state == PartState::Blocked)
        .map(|p| p.id.clone())
    else {
        return Err("no builder is waiting for an answer".into());
    };
    run.append(Event::Steering {
        kind: SteeringKind::Answer,
        target: Some(part.clone()),
        detail: Some(text.to_string()),
    })?;
    let session = run.state.session_of("builder", Some(&part));
    let branch = part_branch(run.id(), &part);
    let instruction = match &session {
        Some(_) => format!(
            "Fallow step: build (part {part})\n\nThe engineer answers your question:\n\n{text}\n\nContinue in your worktree (branch `{branch}`) and return your result; Fallow commits your changes when the step ends."
        ),
        None => {
            load_plan(run)?;
            let mut t = prompts::build(&context(run, run.plan.as_ref()), &part, &branch);
            t.push_str(&format!(
                "\n\nThe engineer answered a question about this part:\n\n{text}"
            ));
            t
        }
    };
    queue(
        run,
        StepKind::Build,
        Some(&part),
        session,
        "the engineer answered",
        instruction,
    )?;
    Ok(())
}

/// The engineer raises the limits or the repair cap, or lets one more step run (FR-038,
/// FR-008). What it allows follows on the next pass.
pub fn raise_limits(
    run: &mut Run,
    usd: Option<f64>,
    minutes: Option<u64>,
    repair_cap: Option<u32>,
    once: bool,
) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if usd.is_none() && minutes.is_none() && repair_cap.is_none() && !once {
        return Err("nothing to change".into());
    }
    if once && run.state.stage != Stage::Paused {
        return Err("continue once is for a run paused at its limit".into());
    }
    if usd.is_some_and(|u| !u.is_finite() || u <= 0.0) || minutes == Some(0) {
        return Err("limits must be above zero".into());
    }
    if repair_cap.is_some_and(|c| c > 20) {
        return Err("a repair cap above 20 is not accepted".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::SetLimits,
        target: None,
        detail: Some(format!(
            "usd={} min={} repair_cap={}{}",
            usd.map_or("-".into(), |u| format!("{u:.2}")),
            minutes.map_or("-".into(), |m| m.to_string()),
            repair_cap.map_or("-".into(), |r| r.to_string()),
            if once { " once" } else { "" }
        )),
    })?;
    run.append(Event::LimitsSet {
        usd,
        minutes,
        repair_cap,
        once,
    })
}

/// The engineer asks Fallow to try again after a question it raised (a transient error, or a
/// review that gave no usable verdict).
pub fn retry(run: &mut Run) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    let Some(n) = &run.state.needs else {
        return Err("nothing is waiting".into());
    };
    if !n.options.iter().any(|o| o == "retry") {
        return Err("this question needs an answer, not a retry".into());
    }
    // Only a retry of the review starts a new review round.
    let detail = if n.why.starts_with(REVIEW_GAVE_NOTHING) {
        RETRY_REVIEW
    } else {
        "retry"
    };
    run.append(Event::Steering {
        kind: SteeringKind::Resume,
        target: None,
        detail: Some(detail.into()),
    })
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

/// Stops the run (constitution II: confirmed on the page). Agents and gates end; branches and
/// worktrees stay. Not while a merge runs: it would land after the stop.
pub fn stop(run: &mut Run, reason: Option<String>) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is already over".into());
    }
    if run.busy == Some(Busy::Merge) {
        return Err("a merge is running; stop the run after it".into());
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
