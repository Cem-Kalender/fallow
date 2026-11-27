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
/// A step whose agent could not be started: it never saw its instruction, so it is no attempt
/// at the work, and the same step goes again (for example while the runtime updates itself).
pub const NOT_STARTED: &str = "not started: ";
/// After [`NOT_STARTED`]: the agent was not signed in to its provider. No attempt at the work
/// either, but trying again cannot help until the engineer signs in, so they are asked at once.
pub const SIGNED_OUT: &str = "signed out: ";

/// 011: a merge refused because the target moved right before it (FR-003).
pub const MOVED: &str = "target moved: ";
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

/// The instruction of `last`'s work: a step queued to go on after Fallow stopped was given only
/// "you were interrupted, go on" (in the session that had the work); its work's instruction is
/// that of the step it went on from, of the same kind, part and agent.
pub(crate) fn own_instruction(run: &Run, last: &Step) -> String {
    let mut n = last.n;
    loop {
        let text = instruction_of(run, n);
        if !text.contains(crate::agent::prompts::LOOP_INTERRUPTED) {
            return text;
        }
        let before = run
            .state
            .steps
            .values()
            .filter(|x| x.n < n && x.kind == last.kind && x.part == last.part && x.def == last.def)
            .map(|x| x.n)
            .max();
        match before {
            Some(p) => n = p,
            None => return text,
        }
    }
}

/// Whether step `n`'s agent program ran: the runtime said so (`Event::Launched`), and Fallow
/// kept it beside the step.
pub(crate) fn launched(run: &Run, n: u32) -> bool {
    run.steps_dir().join(format!("{n}.launched")).is_file()
}

pub(crate) fn activity_lines(run: &Run, n: u32) -> usize {
    std::fs::read_to_string(run.steps_dir().join(format!("{n}.jsonl")))
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

/// A step cut short by Fallow stopping, by a lost session or by an agent that could not be
/// started: the next step takes it up again, so it is no attempt of its own.
pub(crate) fn cut_short(s: &Step) -> bool {
    first_reason(s).is_some_and(|r| {
        r.starts_with(INTERRUPTED) || r.starts_with(SESSION_LOST) || r.starts_with(NOT_STARTED)
    })
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
        .any(|s| matches!(s.kind, SteeringKind::Merge | SteeringKind::MergeUnchecked));
    if run.state.stage == Stage::Ready && asked && run.state.merged.is_none() {
        if let Some(c) = run.state.integrated.clone() {
            let target = format!("refs/heads/{}", run.state.target);
            if let Ok(tip) = git::rev(&run.state.repo, &target) {
                if git::is_ancestor(&run.state.repo, &c, &tip) {
                    // 011: the last merge asked for was without the latest check: said again.
                    let unchecked_asked = run
                        .state
                        .steering
                        .iter()
                        .rev()
                        .find(|s| {
                            matches!(s.kind, SteeringKind::Merge | SteeringKind::MergeUnchecked)
                        })
                        .is_some_and(|s| s.kind == SteeringKind::MergeUnchecked);
                    let unchecked = if unchecked_asked {
                        unchecked_in(run, &tip)
                    } else {
                        Vec::new()
                    };
                    let commit = merge_commit_of(run, &tip).unwrap_or(tip);
                    run.append(Event::Merged {
                        commit,
                        how: "local".into(),
                        unchecked,
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
            session_total_usd: None,
            own_cost: true,
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

/// Decides and starts what comes next. Does nothing while gates or a merge run; agent steps
/// may start while others run (008 US5).
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
    // 018 (FR-018, research R-4): a question from a task of the group in progress holds only
    // that task; anything else the engineer must decide holds the whole loop.
    if run.state.needs.is_some() && super::feature::held_task(run).is_none() {
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
                session_total_usd: outcome.session_total_usd,
                own_cost: true,
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
        Done::Publish { by, result } => match result {
            Ok(d) => {
                let unchecked = std::mem::take(&mut run.publish_unchecked);
                run.append(Event::Published {
                    remote: d.remote,
                    service: d.service,
                    repo: d.repo,
                    branch: d.branch,
                    commit: d.commit,
                    target: d.target,
                    pr_number: d.pr.number,
                    pr_url: d.pr.url,
                    by,
                    unchecked,
                })?;
                if run.state.stage == Stage::Ready {
                    run.append(Event::Stage {
                        to: Stage::Publishing,
                        why: "published as a pull request".into(),
                    })?;
                }
                Ok(())
            }
            Err(f) => {
                run.publish_unchecked.clear();
                run.append(Event::PublishFailed {
                    step: f.step,
                    why: f.why,
                    pushed: f.pushed,
                    by,
                    moved_to: f.moved_to,
                })
            }
        },
        // 011: handled by the engine (an update may start from them).
        Done::Fetched { .. } | Done::PushForward { .. } => Ok(()),
        Done::PrState(r) => {
            let Ok(st) = r else {
                return Ok(());
            };
            let Some(pb) = run
                .state
                .feature_loop
                .as_ref()
                .and_then(|l| l.publication.clone())
            else {
                return Ok(());
            };
            if run.state.stage != Stage::Publishing {
                return Ok(());
            }
            let changed = st.state != pb.state
                || st.checks != pb.checks
                || st.merged_at != pb.merged_at
                || st.conflicting != pb.conflicting
                || st.behind != pb.behind
                || st.strict != pb.strict;
            if changed {
                run.append(Event::PullRequestState {
                    state: st.state,
                    checks: st.checks.clone(),
                    merged_at: st.merged_at.clone(),
                    conflicting: st.conflicting,
                    behind: st.behind,
                    strict: st.strict,
                })?;
            }
            // 011 (research R-7): merged or closed on the remote during an update: its steps are
            // stopped and it is dropped; the loop then follows 010.
            if st.state != crate::publish::host::PrStatus::Open
                && run
                    .state
                    .feature_loop
                    .as_ref()
                    .is_some_and(|l| l.update.is_some())
            {
                run.stop_all();
                super::update::drop_update(
                    run,
                    if st.state == crate::publish::host::PrStatus::Merged {
                        "the pull request was merged on the remote"
                    } else {
                        "the pull request was closed on the remote"
                    },
                )?;
            }
            match st.state {
                crate::publish::host::PrStatus::Merged => {
                    let unchecked = run
                        .state
                        .feature_loop
                        .as_ref()
                        .map(|l| l.unchecked.clone())
                        .unwrap_or_default();
                    run.append(Event::Merged {
                        commit: pb.commit.clone(),
                        how: "pull request".into(),
                        unchecked,
                    })?;
                    run.append(Event::Stage {
                        to: Stage::Done,
                        why: format!("pull request #{} merged on {}", pb.pr_number, pb.repo),
                    })
                }
                crate::publish::host::PrStatus::Closed => run.append(Event::Stage {
                    to: Stage::Ready,
                    why: format!("pull request #{} closed without merging", pb.pr_number),
                }),
                crate::publish::host::PrStatus::Open => Ok(()),
            }
        }
        Done::Merge(r) => match r {
            // A merge that went through is recorded whatever the stage: it happened.
            Ok(commit) => {
                // The target's commits the result was not verified with, as merged (review 8).
                let unchecked = if std::mem::take(&mut run.merge_unchecked).is_empty() {
                    Vec::new()
                } else {
                    unchecked_in(run, &commit)
                };
                run.append(Event::Merged {
                    commit,
                    how: "local".into(),
                    unchecked,
                })?;
                if run.state.stage.is_final() {
                    return Ok(());
                }
                run.append(Event::Stage {
                    to: Stage::Done,
                    why: "merged by the engineer".into(),
                })
            }
            // In its merge line: the line's turn handles it (an update, or the limit).
            Err(e)
                if e.starts_with(MOVED)
                    && run
                        .state
                        .feature_loop
                        .as_ref()
                        .is_some_and(|l| l.lined_at.is_some()) =>
            {
                Ok(())
            }
            Err(e) if e.starts_with(MOVED) => run.append(Event::NeedsEngineer {
                why: format!(
                    "Nothing was merged: {}. Bring it up to date first.",
                    e.trim_start_matches(MOVED)
                ),
                options: vec!["bring up to date".into(), "stop".into()],
            }),
            Err(e) => run.append(Event::NeedsEngineer {
                why: format!("The merge did not go through: {e}"),
                options: vec!["merge again".into(), "stop".into()],
            }),
        },
        // 011: an update's merge was computed (research R-2).
        Done::Update { n, result } => {
            if run.state.stage.is_final() {
                super::update::discard(run, &result);
                return Ok(());
            }
            super::update::computed(run, n, result)
        }
        // 011: read by the engine (the target watch).
        Done::Moved { .. } => Ok(()),
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
    if run.busy == Some(Busy::Publish) || run.state.stage == Stage::Publishing {
        return Err(
            "a loop being published or with its pull request open has nothing to pause".into(),
        );
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
pub fn merge(
    run: &mut Run,
    tx: std::sync::mpsc::Sender<Msg>,
    unchecked: Vec<String>,
) -> Result<(), String> {
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
    if run.is_busy() {
        return Err("a merge or a step is already running".into());
    }
    let commit = run
        .state
        .integrated
        .clone()
        .ok_or("nothing is integrated")?;
    // The engineer's click is recorded by the caller (011: a merge in line runs later).
    if run.state.feature_loop.is_none() {
        run.append(Event::Steering {
            kind: SteeringKind::Merge,
            target: Some(run.state.target.clone()),
            detail: Some(commit.clone()),
        })?;
    }
    let skip_recheck = !unchecked.is_empty();
    // The merge commit says when the loop's result was not verified (constitution I).
    let not_verified = run
        .state
        .feature_loop
        .as_ref()
        .and_then(|l| l.outcome.as_ref())
        .filter(|o| !o.0)
        .map(|o| {
            if o.1.is_empty() {
                "not judged".to_string()
            } else {
                o.1.join(", ")
            }
        });
    run.merge_unchecked = unchecked;
    run.busy = Some(Busy::Merge);
    let repo = run.state.repo.clone();
    let target = run.state.target.clone();
    let scratch = run.worktrees().join("merge");
    let id = run.id();
    std::thread::Builder::new()
        .name(format!("merge-{id}"))
        .spawn(move || {
            // 011 FR-003: the target is checked again right before merging; moved since the
            // loop was verified, nothing is merged and the loop is brought up to date first.
            // Without the latest check (FR-009), the engineer decided: not checked again.
            let moved = !skip_recheck
                && git::rev(&repo, &format!("refs/heads/{target}"))
                    .is_ok_and(|tip| !git::is_ancestor(&repo, &tip, &commit));
            let r = if moved {
                Err(format!(
                    "{MOVED}{target} moved since this loop was verified"
                ))
            } else {
                crate::publish::local::merge_as(
                    &repo,
                    &target,
                    &commit,
                    &scratch,
                    skip_recheck,
                    not_verified.as_deref(),
                )
            };
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Merge(r),
            });
        })
        .map_err(|e| format!("cannot start a thread: {e}"))?;
    Ok(())
}

/// 011: the target's first-parent commits that the merge `merged` brought in beside the loop's
/// verified commit: what a merge without the latest check was not verified with.
fn unchecked_in(run: &Run, merged: &str) -> Vec<String> {
    let repo = &run.state.repo;
    let Some(integrated) = run.state.integrated.as_deref() else {
        return Vec::new();
    };
    let merge = merge_commit_of(run, merged).unwrap_or_else(|| merged.to_string());
    let before = match git::parents(repo, &merge) {
        Ok(p) if p.len() == 2 && p[1] == integrated => p[0].clone(),
        _ => merge,
    };
    git::first_parent_commits(repo, integrated, &before)
}

/// 011: the merge commit that brought the loop's verified commit into the target, on the
/// target's first-parent line up to `tip` (later commits may sit on top of it; review F3).
fn merge_commit_of(run: &Run, tip: &str) -> Option<String> {
    let repo = &run.state.repo;
    let integrated = run.state.integrated.as_deref()?;
    git::first_parent_commits(repo, integrated, tip)
        .into_iter()
        .find(|c| git::parents(repo, c).is_ok_and(|p| p.len() == 2 && p[1] == integrated))
}

/// 010: starts publishing the loop's verified commit as a pull request (FR-008, FR-009), on a
/// thread; its end comes back as `Done::Publish`.
pub fn publish(
    run: &mut Run,
    ctx: &Ctx,
    by: &str,
    remote: &crate::publish::Remote,
    body: &str,
    unchecked: Vec<String>,
) -> Result<(), String> {
    let l = run
        .state
        .feature_loop
        .as_ref()
        .ok_or("only a feature's loop is published")?;
    if let Some(p) = l
        .publication
        .as_ref()
        .filter(|p| p.state == crate::publish::host::PrStatus::Open)
    {
        return Err(format!(
            "this loop's pull request is open already: {}",
            p.pr_url
        ));
    }
    if run.state.stage != Stage::Ready {
        return Err("the loop is not ready to publish".into());
    }
    if l.unlocked {
        return Err("the feature is unlocked: lock it again before publishing".into());
    }
    // Only what the loop finished and judged (constitution I).
    if l.outcome.is_none() {
        return Err("the loop has not finished checking this result".into());
    }
    if run.is_busy() {
        return Err("a merge, a publication or a step is already running".into());
    }
    let commit = run
        .state
        .integrated
        .clone()
        .ok_or("nothing is integrated")?;
    let title = feature_title(run);
    let branch = l.build_branch.clone();
    let dir = run.dir.join("publish");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let body_file = dir.join("body.md");
    std::fs::write(&body_file, body).map_err(|e| format!("{}: {e}", body_file.display()))?;
    run.append(Event::Steering {
        kind: if unchecked.is_empty() {
            SteeringKind::Publish
        } else {
            SteeringKind::PublishUnchecked
        },
        target: remote.target.clone(),
        detail: Some(format!("{commit} ({by})")),
    })?;
    run.busy = Some(Busy::Publish);
    let job = crate::publish::Job {
        repo: run.state.repo.clone(),
        remote: remote.clone(),
        commit,
        branch,
        title,
        body: body_file,
        unchecked: !unchecked.is_empty(),
    };
    run.publish_unchecked = unchecked;
    let (tx, settings, id, who) = (
        ctx.tx.clone(),
        ctx.settings.clone(),
        run.id(),
        by.to_string(),
    );
    let spawned = std::thread::Builder::new()
        .name(format!("publish-{id}"))
        .spawn(move || {
            let result = crate::publish::publish(&job, &settings);
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Publish { by: who, result },
            });
        });
    if let Err(e) = spawned {
        run.busy = None;
        run.append(Event::PublishFailed {
            step: "check".into(),
            why: format!("cannot start a thread: {e}"),
            pushed: None,
            by: by.to_string(),
            moved_to: None,
        })?;
    }
    Ok(())
}

/// The feature's title for its pull request: the spec's heading, else the feature's name.
fn feature_title(run: &Run) -> String {
    let Some(l) = run.state.feature_loop.as_ref() else {
        return run.state.task.clone();
    };
    super::feature::doc_at(run, &l.lock_commit, "spec.md")
        .and_then(|t| {
            t.lines().find_map(|x| x.strip_prefix("# ")).map(|h| {
                h.trim_start_matches("Feature Specification:")
                    .trim()
                    .to_string()
            })
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| l.feature.clone())
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
    if run.busy == Some(Busy::Publish) {
        return Err("the loop is being published; stop it after that".into());
    }
    // 010: its pull request is followed to its end (FR-015: stop once it is closed).
    if run.state.stage == Stage::Publishing {
        return Err(
            "its pull request is open: close it on the remote first, then stop the loop".into(),
        );
    }
    // Every running step ends, and the lead's calls for their questions too.
    run.stop_all();
    // 011: an update in progress is dropped: its working branch removed, the build worktree back
    // on the loop's branch, nothing merged.
    super::update::drop_update(run, "the loop was stopped")?;
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
