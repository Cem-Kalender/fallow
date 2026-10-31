//! The stage machine's shared part (006, kept for 007 loops): resuming after a restart, limits
//! and agent slots, gates, results coming back from threads, and the engineer's steering. What a
//! feature's loop does next is decided in `feature.rs` (research S-9).
//!
//! Results are recorded first (`process` ends with a `verified` event); what follows from them is
//! derived on every pass from the folded state, and each decision begins with the one event that
//! settles it (a queued step, a stage, a question to the engineer). A Fallow that stopped between
//! two events therefore makes the same decision again, never a second one (constitution VII).
//! Agent steps, gates and merges run on threads and come back through `on_done`.
//!
//! 006's own runs (a planner, parts, one review) were replaced by loops (research S-18): their
//! logs still load and show, and they are not driven any more.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::engine::{Busy, Ctx, Done, Msg, Run};
use super::events::*;
use super::state::Step;
use crate::config::{Failures, GateDef};
use crate::verify::{gates, git};

/// Markers at the start of a `verified` reason that the next pass acts on.
pub const BLOCKED: &str = "blocked: ";
pub const INTERRUPTED: &str = "interrupted: ";
pub const SESSION_LOST: &str = "session lost: ";
/// The start of a question raised because Fallow itself could not go on (a transient error).
pub const COULD_NOT_GO_ON: &str = "Fallow could not go on: ";
/// The detail of the engineer's "go on" after a pause (a retry's detail is "retry").
pub(crate) const RESUMED: &str = "resume";
/// The detail of "allow one more repair", with the part as its target.
pub(crate) const ONE_MORE: &str = "one more repair";
pub(crate) const INTERRUPTED_WHY: &str = "interrupted: Fallow stopped while the step ran";

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

pub(crate) fn instruction_of(run: &Run, n: u32) -> String {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.prompt"))).unwrap_or_default()
}

pub(crate) fn activity_lines(run: &Run, n: u32) -> usize {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.jsonl")))
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

/// A step cut short by Fallow stopping or by a lost session: the next step takes it up again,
/// so it is no attempt of its own.
pub(crate) fn cut_short(s: &Step) -> bool {
    first_reason(s).is_some_and(|r| r.starts_with(INTERRUPTED) || r.starts_with(SESSION_LOST))
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

/// Takes an agent slot for the next step, or records why it waits (research S-19).
pub(crate) fn slot(run: &mut Run, ctx: &Ctx) -> bool {
    match ctx.slots.take() {
        Ok(()) => {
            run.waiting = None;
            true
        }
        Err(why) => {
            run.waiting = Some(why);
            false
        }
    }
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
    // A 006 run: shown, not driven (research S-18).
    if run.state.feature_loop.is_none() {
        return Ok(());
    }
    if run.state.stage == Stage::Paused {
        // The engineer's own pause holds until they let the loop go on.
        if super::feature::paused_by_engineer(run) {
            return Ok(());
        }
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
    super::feature::drive(run, ctx)
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
pub fn on_done(run: &mut Run, ctx: &Ctx, done: Done) -> Result<(), String> {
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
                return super::feature::process(run, ctx, n);
            }
            run.append(Event::Verified {
                n,
                accepted: false,
                reasons: vec!["a run from an earlier version of Fallow is not driven".into()],
                commit: None,
            })
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

/// The engineer answers a builder's question (a `blocked` result): the step is asked again in
/// its session, with the answer.
pub fn answer(run: &mut Run, text: &str) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if text.trim().is_empty() {
        return Err("the answer is empty".into());
    }
    if run.state.feature_loop.is_none() {
        return Err("this run is from an earlier version of Fallow and cannot go on".into());
    }
    let Some(last) = run
        .state
        .steps
        .values()
        .rev()
        .find(|s| first_reason(s).is_some_and(|r| r.starts_with(BLOCKED)))
        .cloned()
        .filter(|_| run.state.needs.is_some())
    else {
        return Err("no builder is waiting for an answer".into());
    };
    run.append(Event::Steering {
        kind: SteeringKind::Answer,
        target: last.part.clone(),
        detail: Some(text.to_string()),
    })?;
    super::feature::answered(run, &last, text)
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

/// The engineer pauses the loop (FR-019): the step under way ends, nothing new starts.
pub fn pause(run: &mut Run) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if super::feature::paused_by_engineer(run) {
        return Err("the loop is already paused".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::Pause,
        target: None,
        detail: None,
    })?;
    if run.state.stage != Stage::Paused {
        run.append(Event::Stage {
            to: Stage::Paused,
            why: "paused by the engineer".into(),
        })?;
    }
    Ok(())
}

/// The engineer lets a paused loop go on.
pub fn go_on(run: &mut Run) -> Result<(), String> {
    if !super::feature::paused_by_engineer(run) {
        return Err("the loop is not paused".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::Resume,
        target: None,
        detail: Some(RESUMED.into()),
    })?;
    // The next pass goes back to building, or asks about a limit that holds.
    Ok(())
}

/// A note for the next step, for one agent or any (FR-019).
pub fn note(run: &mut Run, text: &str, agent: Option<String>) -> Result<(), String> {
    if run.state.stage.is_final() {
        return Err("the run is over".into());
    }
    if text.trim().is_empty() {
        return Err("the note is empty".into());
    }
    run.append(Event::Steering {
        kind: SteeringKind::Note,
        target: agent,
        detail: Some(text.trim().to_string()),
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
    if !n
        .options
        .iter()
        .any(|o| o == "retry" || o == "allow one more repair")
    {
        return Err("this question needs an answer, not a retry".into());
    }
    // "Allow one more repair" raises that part's cap by one; a retry starts a new round of
    // tries for what was asked about.
    if n.options.iter().any(|o| o == "allow one more repair") {
        let part = run
            .state
            .steps
            .values()
            .next_back()
            .and_then(|s| s.part.clone());
        return run.append(Event::Steering {
            kind: SteeringKind::Resume,
            target: part,
            detail: Some(ONE_MORE.into()),
        });
    }
    run.append(Event::Steering {
        kind: SteeringKind::Resume,
        target: None,
        detail: Some("retry".into()),
    })
}

/// The engineer's merge of the verified result (FR-009).
pub fn merge(run: &mut Run, tx: std::sync::mpsc::Sender<Msg>) -> Result<(), String> {
    if run.state.stage != Stage::Ready {
        return Err("the run is not ready to merge".into());
    }
    if run.state.feature_loop.as_ref().is_some_and(|l| l.unlocked) {
        return Err("the feature is unlocked: lock it again before merging".into());
    }
    // A loop merges only what it finished and judged (constitution I).
    if run
        .state
        .feature_loop
        .as_ref()
        .is_some_and(|l| l.outcome.is_none())
    {
        return Err("the loop has not finished checking this result".into());
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
    // The lead's call for a question stops too.
    if let Some(a) = &run.asking {
        a.stop.store(true, Ordering::Relaxed);
    }
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
