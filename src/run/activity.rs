//! What the loop page shows (007 US3, FR-017, FR-018; data-model.md, Activity event): one feed of
//! every agent's narration, reads, edits with diffs and commands with their output, attributed to
//! the agent and the task, with Fallow's own checks (commits, gates, verdicts) and the engineer's
//! steering among them; a task's whole story (SC-004); and the loop's state for its screen.
//! Agents' thinking never reaches the activity files, so it is never shown.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::engine::{Busy, Ctx, Run};
use super::events::*;
use super::feature;
use super::state::Step;
use crate::agent::Activity;
use crate::defs::Kind;
use crate::defs::pipeline::{At, GATES, When};
use crate::verify::git;

/// What the feed is filtered on (`GET /api/loops/<l>/activity?agent=&kind=&after=`).
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub agent: Option<String>,
    pub kind: Option<String>,
    pub task: Option<String>,
    /// Entries from this place on (the `i` of the last entry the page has, plus one).
    pub after: usize,
}

fn ms_of(at: &str) -> u64 {
    super::time::parse_rfc3339(at).map_or(0, |s| s * 1000)
}

/// The tool calls of a step, each with its result, in order.
fn step_lines(run: &Run, s: &Step) -> Vec<Value> {
    let text = std::fs::read_to_string(run.steps_dir().join(format!("{}.jsonl", s.n)))
        .unwrap_or_default();
    let lines: Vec<Activity> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let start = s.started.as_deref().map_or(0, ms_of);
    let agent = s.def.clone().unwrap_or_else(|| s.agent.clone().unwrap_or_default());
    let task = s.task.clone().or_else(|| s.part.clone());
    let results: BTreeMap<&str, &Activity> = lines
        .iter()
        .filter(|a| matches!(a.kind.as_str(), "result" | "error" | "refused") && !a.id.is_empty())
        .map(|a| (a.id.as_str(), a))
        .collect();
    let mut out = Vec::new();
    for a in &lines {
        let is_result = matches!(a.kind.as_str(), "result" | "error" | "refused");
        if is_result && !a.id.is_empty() {
            continue; // shown with its call
        }
        let mut e = json!({
            "at": if a.at > 0 { a.at } else { start + a.t },
            "step": s.n,
            "agent": agent,
            "task": task,
            "kind": a.kind,
            "text": a.text,
        });
        if !a.detail.is_empty() {
            e["detail"] = json!(a.detail);
        }
        if let Some(r) = results.get(a.id.as_str()).filter(|_| !a.id.is_empty()) {
            e["result"] = json!(match r.kind.as_str() {
                "refused" => "refused",
                "error" => "failed",
                _ => "ok",
            });
            let out_text = if r.detail.is_empty() { &r.text } else { &r.detail };
            if !out_text.is_empty() {
                e["output"] = json!(out_text);
            }
        }
        if a.kind == "error" {
            e["result"] = json!("failed");
        }
        out.push(e);
    }
    out
}

/// Fallow's own entries: what it accepted, refused, gated and judged, and the engineer's
/// steering.
fn fallow_lines(run: &Run, last_line: &BTreeMap<u32, u64>) -> Vec<Value> {
    let mut out = Vec::new();
    let check = |at: u64, task: Option<String>, text: String, result: Option<&str>, detail: String| {
        let mut e = json!({ "at": at, "agent": "fallow", "task": task, "kind": "check", "text": text });
        if let Some(r) = result {
            e["result"] = json!(r);
        }
        if !detail.is_empty() {
            e["detail"] = json!(detail);
        }
        e
    };
    for s in run.state.steps.values() {
        let Some((ok, reasons)) = &s.verified else { continue };
        // After the step's own last line: the step's end is only known to the second.
        let at = s
            .finished
            .as_deref()
            .map_or(0, ms_of)
            .max(last_line.get(&s.n).copied().unwrap_or(0))
            + 1;
        let what = s.task.clone().or_else(|| s.part.clone()).unwrap_or_default();
        let label = s.kind.label();
        let task = s.task.clone().or_else(|| s.part.clone());
        let first = reasons.first().map(String::as_str).unwrap_or("");
        if let Some(q) = first.strip_prefix(super::machine::BLOCKED) {
            let mut e = json!({ "at": at, "step": s.n, "agent": s.def, "task": task, "kind": "ask", "text": q });
            e["result"] = Value::Null;
            out.push(e);
            continue;
        }
        if first.starts_with(super::machine::INTERRUPTED) || first.starts_with(super::machine::SESSION_LOST) {
            out.push(check(at, task, format!("{label} {what} was cut short: {first}"), None, String::new()));
            continue;
        }
        let text = match (ok, s.kind) {
            (true, StepKind::Review) => format!("{} approved {what}", s.def.as_deref().unwrap_or("the checker")),
            (true, _) => match &s.verified_commit {
                Some(c) => format!("{what} accepted: committed {}", &c[..c.len().min(7)]),
                None => format!("{what} accepted"),
            },
            (false, _) => format!("{label} {what} not accepted: {}", reasons.join("; ")),
        };
        out.push(check(at, task, text, Some(if *ok { "ok" } else { "failed" }), String::new()));
    }
    for g in &run.state.gate_runs {
        let at = ms_of(&g.at);
        let failing: Vec<&String> = g.result.failing.iter().collect();
        let (text, result) = match g.kind {
            GateKind::Baseline => (
                format!("baseline {} on the lock: {}", g.result.gate, g.result.outcome.label()),
                None,
            ),
            _ => {
                let ok = feature::gate_ok(run, g);
                let class = g
                    .class
                    .map(|c| serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default())
                    .unwrap_or_default();
                (
                    format!(
                        "gates on {}: {} {}{}",
                        g.part.as_deref().unwrap_or("the build"),
                        g.result.gate,
                        g.result.outcome.label(),
                        if ok { String::new() } else { format!(" ({class} against the baseline)") }
                    ),
                    Some(if ok { "ok" } else { "failed" }),
                )
            }
        };
        let detail = failing.iter().map(|f| f.as_str()).collect::<Vec<_>>().join("\n");
        out.push(check(at, g.part.clone(), text, result, detail));
    }
    for st in &run.state.steering {
        let (kind, text) = match st.kind {
            SteeringKind::Answer => ("answer", format!("You answered: {}", st.detail.as_deref().unwrap_or(""))),
            SteeringKind::Note => ("note", format!("Your note: {}", st.detail.as_deref().unwrap_or(""))),
            SteeringKind::Pause => ("steer", "You paused the loop".into()),
            SteeringKind::Resume => ("steer", "You let the loop go on".into()),
            SteeringKind::Stop => ("steer", "You stopped the loop".into()),
            SteeringKind::SetLimits => ("steer", format!("You changed the limits: {}", st.detail.as_deref().unwrap_or(""))),
            SteeringKind::Merge => ("steer", "You merged the feature".into()),
            SteeringKind::Unlock => ("steer", "You unlocked the feature".into()),
            _ => continue,
        };
        out.push(json!({ "at": ms_of(&st.at), "agent": "you", "task": st.target, "kind": kind, "text": text }));
    }
    out
}

/// The whole feed, oldest first, each entry with its place `i`.
pub fn feed(run: &Run) -> Vec<Value> {
    let mut all: Vec<Value> = Vec::new();
    let mut last_line: BTreeMap<u32, u64> = BTreeMap::new();
    for s in run.state.steps.values() {
        let lines = step_lines(run, s);
        if let Some(m) = lines.iter().filter_map(|e| e["at"].as_u64()).max() {
            last_line.insert(s.n, m);
        }
        all.extend(lines);
    }
    all.extend(fallow_lines(run, &last_line));
    // Stable: lines of one step keep their order.
    all.sort_by_key(|e| e["at"].as_u64().unwrap_or(0));
    for (i, e) in all.iter_mut().enumerate() {
        e["i"] = json!(i);
    }
    all
}

/// The feed after `f.after`, filtered (`kind = "check"` is Fallow's checks).
pub fn activity(run: &Run, f: &Filter) -> Value {
    let all = feed(run);
    let total = all.len();
    let entries: Vec<Value> = all
        .into_iter()
        .skip(f.after)
        .filter(|e| f.agent.as_deref().is_none_or(|a| e["agent"] == a))
        .filter(|e| f.kind.as_deref().is_none_or(|k| e["kind"] == k))
        .filter(|e| f.task.as_deref().is_none_or(|t| e["task"] == t))
        .collect();
    json!({ "entries": entries, "next": total })
}

/// Everything about one task (SC-004): its steps, the diff of its commit, the commands it ran
/// with their results, and its gate runs.
pub fn task(run: &Run, id: &str) -> Result<Value, String> {
    let l = run
        .state
        .feature_loop
        .as_ref()
        .ok_or("not a feature's loop")?;
    let status = l.tasks.get(id).ok_or_else(|| format!("there is no task {id}"))?;
    let text = l
        .phases
        .iter()
        .flat_map(|p| p.spec.tasks.iter())
        .find(|t| t.id == id)
        .map(|t| t.text.clone())
        .unwrap_or_default();
    let steps: Vec<Value> = run
        .state
        .steps
        .values()
        .filter(|s| s.task.as_deref() == Some(id) && s.queued_seq > status.since)
        .map(|s| {
            json!({
                "n": s.n, "kind": s.kind, "agent": s.def, "model": s.model, "session": s.session,
                "resumed": s.resume.is_some(), "started": s.started, "finished": s.finished,
                "cost_usd": s.cost_usd, "verified": s.verified, "commit": s.verified_commit,
            })
        })
        .collect();
    let lines: Vec<Value> = feed(run)
        .into_iter()
        .filter(|e| e["task"] == id)
        .collect();
    let commands: Vec<Value> = lines.iter().filter(|e| e["kind"] == "run").cloned().collect();
    // All of the task's work: from where its first step started to its accepted commit (a
    // repair's commit holds only the repair).
    let first_base = run
        .state
        .steps
        .values()
        .find(|s| s.task.as_deref() == Some(id) && s.queued_seq > status.since && s.base.is_some())
        .and_then(|s| s.base.clone());
    let diff = status
        .commit
        .as_deref()
        .and_then(|c| match &first_base {
            Some(b) => git::diff_between(&run.state.repo, b, c, ".").ok(),
            None => git::patch_of(&run.state.repo, c).ok(),
        })
        .map(|d| crate::agent::trim_to(&d, crate::agent::DETAIL_MAX))
        .unwrap_or_default();
    let gates: Vec<&super::state::GateEntry> = run
        .state
        .gate_runs
        .iter()
        .filter(|g| g.part.as_deref() == Some(id))
        .collect();
    Ok(json!({
        "id": id, "text": text, "state": status.state, "phase": status.phase,
        "repairs": status.repairs, "commit": status.commit, "steps": steps, "diff": diff,
        "commands": commands, "gates": gates, "activity": lines,
    }))
}

/// The loop's screen (contracts/web-api.md `GET /api/loops/<l>`).
pub fn view(run: &Run, ctx: &Ctx) -> Result<Value, String> {
    let s = &run.state;
    let l = s.feature_loop.as_ref().ok_or("not a feature's loop")?;
    let lib = feature::library(run, ctx);
    let pl = feature::pipeline_of(run, &lib).ok();
    let now = super::time::unix_now();
    let running: Option<&Step> = match run.busy {
        Some(Busy::Step(n)) => s.steps.get(&n),
        _ => None,
    };
    let current_phase = l
        .phases
        .iter()
        .find(|p| p.state != PhaseState::Done)
        .map(|p| p.spec.n);
    let blocked_def = s.needs.as_ref().and_then(|_| {
        s.steps
            .values()
            .next_back()
            .filter(|x| {
                x.verified
                    .as_ref()
                    .and_then(|v| v.1.first())
                    .is_some_and(|r| r.starts_with(super::machine::BLOCKED))
            })
            .and_then(|x| x.def.clone())
    });
    // The agents: the pipeline's, each with what it does now and what it cost.
    let mut names: Vec<(String, String)> = Vec::new();
    if let Some(pl) = &pl {
        for st in &pl.steps {
            if st.agent != GATES && !names.iter().any(|(n, _)| *n == st.agent) {
                names.push((st.agent.clone(), format!("{:?}", st.when).to_lowercase()));
            }
        }
    }
    let agents: Vec<Value> = names
        .iter()
        .map(|(name, when)| {
            let def = lib.agent(name);
            let mine: Vec<&Step> = s.steps.values().filter(|x| x.def.as_deref() == Some(name)).collect();
            let cost: f64 = mine.iter().filter_map(|x| x.cost_usd).sum();
            let tools = running
                .filter(|r| r.def.as_deref() == Some(name))
                .map(|r| {
                    step_lines(run, r)
                        .iter()
                        .filter(|e| e["kind"] != "say")
                        .count()
                })
                .unwrap_or(0);
            let status = if running.is_some_and(|r| r.def.as_deref() == Some(name)) {
                "working"
            } else if blocked_def.as_deref() == Some(name) {
                "asks you"
            } else if mine.is_empty() {
                "idle"
            } else {
                "waiting"
            };
            let checker = def.is_some_and(|d| d.kind == Kind::Checker);
            json!({
                "name": name,
                "kind": if checker { "checker" } else { "maker" },
                "when": when,
                "runtime": def.map(|d| d.runtime.clone()),
                "model": def.map(|d| d.model.clone()),
                "where": if checker { "fresh per check".to_string() } else { l.build_branch.clone() },
                "status": status,
                "task": running.filter(|r| r.def.as_deref() == Some(name)).and_then(|r| r.task.clone().or_else(|| r.part.clone())),
                "tool_calls": tools,
                "steps": mine.len(),
                "cost_usd": cost,
            })
        })
        .collect();
    // Fallow's checks: the gates against the baseline, each story's criteria, the commits.
    let mut gates: BTreeMap<String, Value> = BTreeMap::new();
    for g in s.gate_runs.iter().filter(|g| g.kind == GateKind::Part) {
        let base = s.baseline(&g.result.gate);
        let newly: Vec<&String> = g
            .result
            .failing
            .iter()
            .filter(|f| base.is_none_or(|b| !b.failing.contains(*f)))
            .collect();
        gates.insert(
            g.result.gate.clone(),
            json!({
                "gate": g.result.gate,
                "command": s.gates.iter().find(|x| x.name == g.result.gate).map(|x| x.command.clone()),
                "part": g.part, "commit": g.result.commit, "outcome": g.result.outcome.label(),
                "ok": feature::gate_ok(run, g), "failing": g.result.failing, "newly_failing": newly,
                "baseline": base.map(|b| json!({ "commit": b.commit, "outcome": b.outcome.label(), "failing": b.failing.len() })),
            }),
        );
    }
    let answerer = pl.as_ref().and_then(|pl| feature::scenario_checker(pl, &lib));
    let spec = feature::doc_at(run, &l.lock_commit, "spec.md")
        .and_then(|t| crate::speckit::spec::read("spec.md", &t).ok())
        .unwrap_or_default();
    let stories: Vec<Value> = spec
        .stories
        .iter()
        .map(|st| {
            // The verdict of the checker that answers the scenarios (others report findings).
            let last = s.steps.values().rev().find(|x| {
                x.kind == StepKind::Review
                    && x.part.as_deref() == Some(st.id.as_str())
                    && x.verified.is_some()
                    && answerer.as_deref().is_none_or(|a| x.def.as_deref() == Some(a))
            });
            let verdict = last.and_then(|x| s.verdicts.iter().find(|v| v.step == x.n));
            let criteria: Vec<Value> = st
                .scenarios
                .iter()
                .map(|sc| {
                    let a = verdict.and_then(|v| v.answers.iter().find(|a| a.criterion == sc.id));
                    json!({
                        "id": sc.id, "text": sc.text, "must_fail": sc.must_fail,
                        "state": match a { None => "not yet", Some(a) if a.passed => "passed", Some(_) => "failed" },
                        "ran": a.map(|a| a.ran.clone()), "result": a.map(|a| a.result.clone()),
                    })
                })
                .collect();
            json!({ "id": st.id, "title": st.title, "criteria": criteria,
                    "approved": verdict.map(|v| v.approved), "reviewer": last.and_then(|x| x.def.clone()) })
        })
        .collect();
    let mut commits: Vec<Value> = Vec::new();
    for p in &l.phases {
        for t in &p.spec.tasks {
            if let Some(c) = l.tasks.get(&t.id).and_then(|x| x.commit.clone()) {
                commits.push(json!({ "commit": c, "task": t.id, "text": t.text }));
            }
        }
    }
    commits.reverse();
    // The phases and their tasks, for the tree.
    let phases: Vec<Value> = l
        .phases
        .iter()
        .map(|p| {
            let tasks: Vec<Value> = p
                .spec
                .tasks
                .iter()
                .map(|t| {
                    let st = l.tasks.get(&t.id);
                    json!({ "id": t.id, "text": t.text, "story": t.story,
                            "state": st.map(|x| x.state), "repairs": st.map_or(0, |x| x.repairs),
                            "commit": st.and_then(|x| x.commit.clone()) })
                })
                .collect();
            let done = tasks.iter().filter(|t| t["state"] == "accepted").count();
            json!({ "n": p.spec.n, "label": p.spec.label, "title": p.spec.title, "story": p.spec.story,
                    "state": p.state, "done": done, "tasks": tasks,
                    "checkpoint": l.phases.iter().any(|_| true) && p.spec.story.is_some() })
        })
        .collect();
    let task_total: usize = l.tasks.len();
    let task_done = l.tasks.values().filter(|t| t.state == TaskState::Accepted).count();
    let repairs = s
        .steps
        .values()
        .filter(|x| x.kind == StepKind::Repair && !super::machine::cut_short(x))
        .count();
    let pipeline = pl.as_ref().map(|pl| {
        json!({
            "name": pl.name, "verifying": pl.verifying, "missing": pl.missing,
            "caps": { "task_repairs": pl.caps.task_repairs, "review_rounds": pl.caps.review_rounds },
            "steps": pl.steps.iter().map(|st| json!({
                "agent": st.agent, "when": format!("{:?}", st.when).to_lowercase(),
                "at": if st.at() == At::Start { "start" } else { "end" },
                "paths": st.paths, "checker": lib.agent(&st.agent).is_some_and(|d| d.kind == Kind::Checker),
            })).collect::<Vec<_>>(),
            "reviews": pl.steps.iter().any(|st| st.when == When::Checkpoint && st.at() == At::End),
        })
    });
    let stage = match (&s.stage, &l.outcome) {
        (Stage::Ready, Some((false, _))) => "done_unverified".to_string(),
        (st, _) => serde_json::to_value(st).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
    };
    Ok(json!({
        "id": s.id, "project": l.project, "feature": l.feature, "lock": l.lock,
        "lock_commit": l.lock_commit, "build_branch": l.build_branch, "base_branch": l.base_branch,
        "stage": stage, "unlocked": l.unlocked, "outcome": l.outcome, "busy": super::engine::view(run)["busy"],
        "waiting": run.waiting, "needs": s.needs, "limit_reached": s.limit_reached,
        "current_phase": current_phase, "phases": phases, "pipeline": pipeline, "agents": agents,
        "checks": { "gates": gates.into_values().collect::<Vec<_>>(), "stories": stories, "commits": commits },
        "budget": { "usd": s.limits.usd, "minutes": s.limits.minutes },
        "used": { "usd": s.cost_usd, "minutes": s.minutes_used(now), "tasks": task_done, "of": task_total },
        "repairs": repairs, "merged": s.merged, "integrated": s.integrated,
        "steering": s.steering, "stopped": s.stopped,
        "paused_by_engineer": feature::paused_by_engineer(run),
    }))
}
