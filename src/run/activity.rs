//! What the loop page shows (007 US3, FR-017, FR-018; data-model.md, Activity event): one feed of
//! every agent's narration, reads, edits with diffs and commands with their output, attributed to
//! the agent and the task, with Fallow's own checks (commits, gates, verdicts) and the engineer's
//! steering among them; a task's whole story (SC-004); and the loop's state for its screen.
//! Agents' thinking never reaches the activity files, so it is never shown.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::engine::{Ctx, Run};
use super::events::*;
use super::feature;
use super::state::Step;
use crate::agent::Activity;
use crate::defs::Kind;
use crate::defs::pipeline::{At, GATES, OnFail, When};
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
    let text =
        std::fs::read_to_string(run.steps_dir().join(format!("{}.jsonl", s.n))).unwrap_or_default();
    let lines: Vec<Activity> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let start = s.started.as_deref().map_or(0, ms_of);
    let agent = s
        .def
        .clone()
        .unwrap_or_else(|| s.agent.clone().unwrap_or_default());
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
            let out_text = if r.detail.is_empty() {
                &r.text
            } else {
                &r.detail
            };
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
    let check =
        |at: u64, task: Option<String>, text: String, result: Option<&str>, detail: String| {
            let mut e =
                json!({ "at": at, "agent": "fallow", "task": task, "kind": "check", "text": text });
            if let Some(r) = result {
                e["result"] = json!(r);
            }
            if !detail.is_empty() {
                e["detail"] = json!(detail);
            }
            e
        };
    for s in run.state.steps.values() {
        let Some((ok, reasons)) = &s.verified else {
            continue;
        };
        // After the step's own last line: the step's end is only known to the second.
        let at = s
            .finished
            .as_deref()
            .map_or(0, ms_of)
            .max(last_line.get(&s.n).copied().unwrap_or(0))
            + 1;
        let what = s
            .task
            .clone()
            .or_else(|| s.part.clone())
            .unwrap_or_default();
        let label = s.kind.label();
        let task = s.task.clone().or_else(|| s.part.clone());
        let first = reasons.first().map(String::as_str).unwrap_or("");
        if let Some(q) = first.strip_prefix(super::machine::BLOCKED) {
            let mut e = json!({ "at": at, "step": s.n, "agent": s.def, "task": task, "kind": "ask", "text": q });
            e["result"] = Value::Null;
            out.push(e);
            continue;
        }
        if let Some(why) = first.strip_prefix(super::machine::NOT_STARTED) {
            let text = match why.strip_prefix(super::machine::SIGNED_OUT) {
                Some(w) => format!(
                    "{label} {what}: the agent is not signed in, it goes again once you sign in: {}",
                    w.trim_start_matches(crate::agent::ENDED_WITH_ERROR)
                ),
                None => format!("{label} {what} did not start, it goes again: {why}"),
            };
            out.push(check(at, task, text, None, String::new()));
            continue;
        }
        if first.starts_with(super::machine::INTERRUPTED)
            || first.starts_with(super::machine::SESSION_LOST)
        {
            out.push(check(
                at,
                task,
                format!("{label} {what} was cut short: {first}"),
                None,
                String::new(),
            ));
            continue;
        }
        let text = match (ok, s.kind) {
            (true, StepKind::Review) => format!(
                "{} approved {what}",
                s.def.as_deref().unwrap_or("the checker")
            ),
            (true, _) => match &s.verified_commit {
                Some(c) => format!("{what} accepted: committed {}", &c[..c.len().min(7)]),
                None => format!("{what} accepted"),
            },
            (false, _) => format!("{label} {what} not accepted: {}", reasons.join("; ")),
        };
        out.push(check(
            at,
            task,
            text,
            Some(if *ok { "ok" } else { "failed" }),
            String::new(),
        ));
    }
    for g in &run.state.gate_runs {
        let at = ms_of(&g.at);
        let failing: Vec<&String> = g.result.failing.iter().collect();
        let (text, result) = match g.kind {
            GateKind::Baseline => (
                format!(
                    "baseline {} on the lock: {}",
                    g.result.gate,
                    g.result.outcome.label()
                ),
                None,
            ),
            _ => {
                let ok = feature::gate_ok(run, g);
                let class = g
                    .class
                    .map(|c| {
                        serde_json::to_value(c)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                (
                    format!(
                        "gates on {}: {} {}{}",
                        g.part.as_deref().unwrap_or("the build"),
                        g.result.gate,
                        g.result.outcome.label(),
                        if ok {
                            String::new()
                        } else {
                            format!(" ({class} against the baseline)")
                        }
                    ),
                    Some(if ok { "ok" } else { "failed" }),
                )
            }
        };
        let detail = failing
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        out.push(check(at, g.part.clone(), text, result, detail));
    }
    // 010: every publishing step (FR-018); 011: every update's step (FR-008).
    if let Some(l) = run.state.feature_loop.as_ref() {
        for (at, text, ok) in l.publish_steps.iter().chain(l.update_steps.iter()) {
            out.push(check(
                ms_of(at) + 999,
                None,
                text.clone(),
                Some(if *ok { "ok" } else { "failed" }),
                String::new(),
            ));
        }
    }
    // Every answer, with who gave it and its commit (FR-023).
    let mut by_engineer: Vec<&str> = Vec::new();
    if let Some(l) = run.state.feature_loop.as_ref() {
        for q in l.questions.values() {
            for h in &q.history {
                let short = h.commit.as_deref().map(|c| &c[..c.len().min(7)]);
                let answer = h.answer.as_deref().unwrap_or("");
                let (agent, text, result) = match (h.state, h.by.as_deref()) {
                    (QuestionState::WithLead, _) => continue,
                    (QuestionState::Answered, by) if super::state::by_a_lead(by) => {
                        let who = by.unwrap_or("lead");
                        (
                            who,
                            format!("{who} answered {}: {answer}", q.id),
                            Some("ok"),
                        )
                    }
                    (QuestionState::Decided, by) => {
                        let who = by
                            .filter(|b| super::state::by_a_lead(Some(b)))
                            .unwrap_or("lead");
                        let the = if who == "lead" {
                            "the lead".to_string()
                        } else {
                            who.to_string()
                        };
                        (
                            who,
                            format!(
                                "decided by {the} ({}): {answer}. Confirm it or overrule it.",
                                q.id
                            ),
                            Some("ok"),
                        )
                    }
                    (QuestionState::WaitingEngineer, _) => (
                        "lead",
                        match &q.recommended {
                            Some(r) => format!(
                                "relayed to you ({}): {}{} The lead recommends: {r}",
                                q.id,
                                q.text,
                                if q.text.trim_end().ends_with(['?', '.', '!']) {
                                    ""
                                } else {
                                    "."
                                }
                            ),
                            None => format!("relayed to you ({}): {}", q.id, q.text),
                        },
                        None,
                    ),
                    (QuestionState::Answered, _) => {
                        by_engineer.push(answer);
                        (
                            "you",
                            format!("You answered {}: {answer}", q.id),
                            Some("ok"),
                        )
                    }
                    (QuestionState::Overruled, _) => {
                        by_engineer.push(answer);
                        (
                            "you",
                            format!("You overruled the lead on {}: {answer}", q.id),
                            Some("ok"),
                        )
                    }
                    (QuestionState::Confirmed, _) => (
                        "you",
                        format!("You confirmed the lead's decision on {}: {answer}", q.id),
                        Some("ok"),
                    ),
                };
                // After the agent's lines of the same second.
                let mut e = json!({
                    "at": ms_of(&h.at) + 999, "agent": agent, "task": q.task, "kind": "answer",
                    "text": text, "question": q.id,
                });
                // A line of a lead's, whichever lead (009): the page marks it as the lead's.
                if agent != "you" {
                    e["lead"] = json!(true);
                }
                if let Some(r) = result {
                    e["result"] = json!(r);
                }
                if let Some(c) = short {
                    e["commit"] = json!(c);
                    e["detail"] = json!(format!("Committed {c} to spec.md's Clarifications"));
                }
                if h.state == QuestionState::WaitingEngineer {
                    if let Some(w) = &q.why {
                        e["detail"] = json!(format!("Relayed because {w}."));
                    }
                }
                out.push(e);
            }
        }
    }
    // 018 (FR-015, FR-024): the merges and overlaps of groups of [P] tasks.
    if let Some(l) = run.state.feature_loop.as_ref() {
        // 018 (research R-6a): a group a new lock dropped, and what Fallow could not remove.
        for g in &l.groups {
            let Some(at) = &g.ended_at else {
                continue;
            };
            for t in g.tasks.iter().filter(|t| t.dropped) {
                out.push(check(
                    ms_of(at),
                    Some(t.id.clone()),
                    format!(
                        "{} built alongside was not merged before the new lock: built again",
                        t.id
                    ),
                    None,
                    "Its working copy and branch were removed; it is built again under the new lock.".into(),
                ));
            }
            for x in &g.left {
                out.push(check(
                    ms_of(at),
                    None,
                    format!("Fallow could not remove {x}: remove it by hand"),
                    Some("fail"),
                    String::new(),
                ));
            }
        }
        let mut seen_merge: Vec<(&str, Option<&str>)> = Vec::new();
        for t in l.groups.iter().flat_map(|g| g.tasks.iter()) {
            for (o, at) in t.overlaps.iter().zip(&t.overlaps_at) {
                out.push(check(
                    ms_of(at),
                    Some(t.id.clone()),
                    super::groups::overlap_line(&t.id, &o.files, &o.with),
                    Some("fail"),
                    String::new(),
                ));
            }
            // Once, though a task merged before a new lock is in two groups.
            let first = !seen_merge.contains(&(t.id.as_str(), t.merged.as_deref()));
            seen_merge.push((t.id.as_str(), t.merged.as_deref()));
            if let (Some(c), Some(at), true) = (&t.merged, &t.merged_at, first) {
                out.push(check(
                    ms_of(at),
                    Some(t.id.clone()),
                    format!("{} merged into the build branch (built alongside)", t.id),
                    Some("ok"),
                    format!("Merge commit {}", &c[..c.len().min(7)]),
                ));
            }
        }
    }
    for st in &run.state.steering {
        if st.kind == SteeringKind::Answer
            && (by_engineer.contains(&st.detail.as_deref().unwrap_or("").trim())
                || st
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.starts_with("confirmed the lead's decision")))
        {
            continue; // shown with its question
        }
        let (kind, text) = match st.kind {
            SteeringKind::Answer => (
                "answer",
                format!("You answered: {}", st.detail.as_deref().unwrap_or("")),
            ),
            SteeringKind::Note => (
                "note",
                format!("Your note: {}", st.detail.as_deref().unwrap_or("")),
            ),
            SteeringKind::Pause => ("steer", "You paused the loop".into()),
            SteeringKind::Resume => ("steer", "You let the loop go on".into()),
            SteeringKind::Stop => ("steer", "You stopped the loop".into()),
            SteeringKind::SetLimits => (
                "steer",
                format!(
                    "You changed the limits: {}",
                    st.detail.as_deref().unwrap_or("")
                ),
            ),
            SteeringKind::Merge => ("steer", "You asked to merge the feature".into()),
            // 011 FR-009: never in the words of a verified merge.
            SteeringKind::MergeUnchecked => (
                "steer",
                "You merged without the latest check: not verified on top of the target as it is now"
                    .into(),
            ),
            SteeringKind::Publish
                if st
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.ends_with("(when_verified)")) =>
            {
                continue;
            }
            SteeringKind::Publish => (
                "steer",
                "You published the feature as a pull request".into(),
            ),
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
    let status = l
        .tasks
        .get(id)
        .ok_or_else(|| format!("there is no task {id}"))?;
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
    let lines: Vec<Value> = feed(run).into_iter().filter(|e| e["task"] == id).collect();
    let commands: Vec<Value> = lines
        .iter()
        .filter(|e| e["kind"] == "run")
        .cloned()
        .collect();
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
    // The running step of an agent, if any: several may run at once (008 R-7).
    let running_of = |name: &str| -> Option<&Step> {
        run.running
            .keys()
            .filter_map(|n| s.steps.get(n))
            .find(|x| x.def.as_deref() == Some(name))
    };
    let current_phase = l
        .phases
        .iter()
        .find(|p| p.state != PhaseState::Done)
        .map(|p| p.spec.n);
    let blocked_def = s.needs.as_ref().and_then(|_| {
        super::questions::waiting(l)
            .and_then(|q| s.steps.get(&q.step))
            .and_then(|x| x.def.clone())
    });
    let blocked_def = blocked_def.or_else(|| {
        s.needs.as_ref()?;
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
            let tools = running_of(name)
                .map(|r| {
                    step_lines(run, r)
                        .iter()
                        .filter(|e| e["kind"] != "say")
                        .count()
                })
                .unwrap_or(0);
            let status = if running_of(name).is_some() {
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
                "task": running_of(name).and_then(|r| r.task.clone().or_else(|| r.part.clone())),
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
    let answerer = pl
        .as_ref()
        .and_then(|pl| feature::scenario_checker(pl, &lib));
    let spec = feature::doc_at(run, &l.lock_commit, "spec.md")
        .and_then(|t| crate::speckit::spec::read("spec.md", &t).ok())
        .unwrap_or_default();
    // The checkers that judge a story at its checkpoint's end; those without `paths` always run.
    let judges: Vec<(String, bool)> = pl
        .as_ref()
        .map(|pl| {
            pl.steps
                .iter()
                .filter(|x| {
                    x.when == When::Checkpoint
                        && x.at() == At::End
                        && lib.agent(&x.agent).is_some_and(|a| a.kind == Kind::Checker)
                })
                .map(|x| (x.agent.clone(), x.paths.is_empty()))
                .collect()
        })
        .unwrap_or_default();
    let stories: Vec<Value> = spec
        .stories
        .iter()
        .map(|st| {
            // The latest review round with a verdict (008, data-model.md): while a repair or a
            // new lock waits to be checked, the round that led to it stays shown, marked.
            let bounds = feature::round_bounds(run, l, &st.id);
            let of_story = |x: &&Step| {
                x.kind == StepKind::Review && x.part.as_deref() == Some(st.id.as_str())
            };
            let reviewed = |x: &&Step| of_story(x) && x.verified.is_some();
            let latest = s.steps.values().filter(reviewed).map(|x| x.queued_seq).max();
            let (from, to) = match latest {
                Some(q) => (
                    bounds.iter().copied().filter(|b| *b < q).max().unwrap_or(0),
                    bounds.iter().copied().find(|b| *b > q).unwrap_or(u64::MAX),
                ),
                None => (feature::round_start(run, l, &st.id), u64::MAX),
            };
            let rechecking = to != u64::MAX;
            let in_round = |x: &&Step| reviewed(x) && x.queued_seq > from && x.queued_seq < to;
            // A check of the story queued or running in the round shown: not judged yet.
            let pending = !rechecking
                && s.steps
                    .values()
                    .filter(of_story)
                    .any(|x| x.verified.is_none() && x.queued_seq > from);
            // The verdict of the checker that answers the scenarios (others report findings).
            let last = s.steps.values().rev().filter(in_round).find(|x| {
                answerer.as_deref().is_none_or(|a| x.def.as_deref() == Some(a))
            });
            let verdict = last.and_then(|x| s.verdicts.iter().find(|v| v.step == x.n));
            let criteria: Vec<Value> = st
                .scenarios
                .iter()
                .map(|sc| {
                    let a = verdict.and_then(|v| v.answers.iter().find(|a| a.criterion == sc.id));
                    // Checked only when the verdict was complete for it and says what was run
                    // and what came out (constitution I): a missing part is "not yet".
                    let missing = verdict.is_some_and(|v| {
                        !v.complete && (v.missing.is_empty() || v.missing.iter().any(|m| m.contains(sc.id.as_str())))
                    });
                    let state = match a {
                        Some(a) if !a.passed => "failed",
                        Some(a) if !missing && !a.ran.trim().is_empty() && !a.result.trim().is_empty() => "passed",
                        _ => "not yet",
                    };
                    json!({
                        "id": sc.id, "text": sc.text, "must_fail": sc.must_fail, "state": state,
                        "ran": a.map(|a| a.ran.clone()), "result": a.map(|a| a.result.clone()),
                        "expected": (state == "failed").then(|| expected_of(&sc.text)),
                    })
                })
                .collect();
            // Every checker's latest outcome on the story in this round, with its reasons
            // (FR-026).
            let mut checkers: Vec<Value> = Vec::new();
            let mut findings = false;
            for x in s.steps.values().rev().filter(in_round) {
                let def = x.def.clone().unwrap_or_default();
                if checkers.iter().any(|c| c["agent"] == def.as_str()) {
                    continue;
                }
                let (ok, reasons) = x.verified.clone().unwrap_or_default();
                findings |= !ok && feature::has_findings(&reasons);
                checkers.push(json!({ "agent": def, "step": x.n, "passed": ok, "reasons": reasons }));
            }
            // Every checker that always runs passed in this round.
            let all_passed = judges.iter().filter(|(_, always)| *always).all(|(agent, _)| {
                checkers.iter().any(|c| c["agent"] == agent.as_str() && c["passed"] == true)
            });
            let total = criteria.len();
            let count = |state: &str| criteria.iter().filter(|c| c["state"] == state).count();
            let (checked, failed) = (count("passed"), count("failed"));
            let state = if answerer.is_none() {
                "not_checked"
            } else if failed > 0 || findings {
                "failed"
            } else if checked == total
                && verdict.is_some_and(|v| v.approved && v.complete)
                && all_passed
                // Also a checker that runs only for some paths, or did not finish (constitution
                // I: never green while a checker of the round has not passed).
                && checkers.iter().all(|c| c["passed"] == true)
                && !pending
                && !rechecking
            {
                "checked"
            } else {
                "progress"
            };
            json!({ "id": st.id, "title": st.title, "criteria": criteria, "checkers": checkers,
                    "approved": verdict.map(|v| v.approved), "reviewer": last.and_then(|x| x.def.clone()),
                    "checked_by_pipeline": answerer.is_some(), "total": total, "checked": checked,
                    "failed": failed, "findings": findings, "state": state, "rechecking": rechecking,
                    "round": last.map(|x| x.n) })
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
    // 011: each update's merge commit, the latest first (FR-008).
    for (n, c, target) in l.update_merges.iter().rev() {
        let short: String = target.chars().take(7).collect();
        commits.insert(
            0,
            json!({ "commit": c, "task": format!("update {n}"), "text": format!("{} at {short} merged in", run.state.target) }),
        );
    }
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
            // The stories reviewed after this phase: those whose last task is in it (T086: a
            // story spread over two phases is reviewed once, after the second).
            let checkpoints = feature::stories_ending_at(l, p.spec.n);
            json!({ "n": p.spec.n, "label": p.spec.label, "title": p.spec.title, "story": p.spec.story,
                    "state": p.state, "done": done, "tasks": tasks,
                    "checkpoint": !checkpoints.is_empty(), "checkpoints": checkpoints })
        })
        .collect();
    let task_total: usize = l.tasks.len();
    let task_done = l
        .tasks
        .values()
        .filter(|t| t.state == TaskState::Accepted)
        .count();
    let repairs = s
        .steps
        .values()
        .filter(|x| x.kind == StepKind::Repair && !super::machine::cut_short(x))
        .count();
    let pipeline = pl.as_ref().map(|pl| {
        json!({
            "name": pl.name, "verifying": pl.verifying, "missing": pl.missing,
            // 018 (data-model "Loop view"): the most builders on [P] tasks; `at` of every
            // step, task steps included (the builder and the gates are "end").
            "caps": { "task_repairs": pl.caps.task_repairs, "review_rounds": pl.caps.review_rounds, "builders": pl.caps.builders },
            "steps": pl.steps.iter().enumerate().map(|(i, st)| json!({
                "agent": st.agent, "when": format!("{:?}", st.when).to_lowercase(),
                // 018: the one maker per task without `at`, for the graph (every step has `at`
                // here).
                "builder": pl.builder_index() == Some(i),
                "at": if st.at() == At::Start { "start" } else { "end" },
                "paths": st.paths, "checker": lib.agent(&st.agent).is_some_and(|d| d.kind == Kind::Checker),
                // For the live graph (008 T043): what runs alongside, and where a failure goes.
                "parallel_with": st.parallel_with,
                "on_fail": if st.on_fail == OnFail::Engineer { "engineer" } else { "repair" },
            })).collect::<Vec<_>>(),
            "reviews": pl.steps.iter().any(|st| st.when == When::Checkpoint && st.at() == At::End),
        })
    });
    // 018 (contracts/web-api.md "The loop view"): the groups of [P] tasks, a row per task.
    let groups: Vec<Value> = l
        .groups
        .iter()
        .map(|g| {
            json!({
                "phase": g.phase, "done": g.done, "dropped": g.dropped,
                "tasks": g.tasks.iter().map(|t| json!({
                    "id": t.id, "state": group_task_state(run, t),
                    "worktree": format!("task-{}", t.id), "merged": t.merged,
                    "overlaps": t.overlaps.iter().map(|o| json!({ "files": o.files, "with": o.with })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let questions: Vec<Value> = l
        .questions
        .values()
        .map(|q| {
            let mut v = json!({
                "id": q.id, "step": q.step, "task": q.task, "text": q.text, "options": q.options,
                "state": q.state, "by": q.by, "by_lead": q.by_lead(), "answer": q.answer, "commit": q.commit,
                "affects": q.affects, "recommended": q.recommended, "reopened": q.reopened,
                "why": q.why,
            });
            v["confirmable"] = json!(q.state == QuestionState::Decided && !s.stage.is_final());
            v["agent"] = json!(s.steps.get(&q.step).and_then(|x| x.def.clone()));
            v["overrulable"] = json!(
                !s.stage.is_final()
                    && (q.state == QuestionState::Decided
                        || (q.state == QuestionState::Answered && q.by_lead()))
            );
            v
        })
        .collect();
    let stage = match (&s.stage, &l.outcome) {
        (Stage::Ready, Some((false, _))) => "done_unverified".to_string(),
        (st, _) => serde_json::to_value(st)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
    };
    Ok(json!({
        "id": s.id, "project": l.project, "feature": l.feature, "lock": l.lock,
        "lock_commit": l.lock_commit, "build_branch": l.build_branch, "base_branch": l.base_branch,
        "stage": stage, "unlocked": l.unlocked, "outcome": l.outcome, "busy": super::engine::view(run)["busy"],
        "waiting": run.waiting, "needs": s.needs, "limit_reached": s.limit_reached,
        // Where the loop waits for the engineer, for the live graph (008 T043): the step that
        // failed or asked, when nothing runs and no limit was reached; its agent and its part.
        "waits_at": s.needs.as_ref().filter(|_| run.running.is_empty() && s.limit_reached.is_none())
            .and_then(|_| s.steps.values().filter(|x| x.def.is_some()).max_by_key(|x| x.n))
            .filter(|x| matches!(x.verified, Some((false, _))) || matches!(x.end, Some(StepEnd::Failed { .. })))
            .map(|x| json!({ "agent": x.def, "part": x.task.clone().or_else(|| x.part.clone()) })),
        "current_phase": current_phase, "phases": phases, "pipeline": pipeline, "agents": agents,
        "checks": { "gates": gates.into_values().collect::<Vec<_>>(), "stories": stories, "commits": commits },
        "budget": { "usd": s.limits.usd, "minutes": s.limits.minutes },
        "used": { "usd": s.cost_usd, "minutes": s.minutes_used(now), "tasks": task_done, "of": task_total },
        "repairs": repairs, "merged": s.merged, "integrated": s.integrated,
        "steering": s.steering, "stopped": s.stopped,
        "paused_by_engineer": feature::paused_by_engineer(run),
        "questions": questions,
        "level": l.level,
        "groups": groups,
    }))
}

/// 018: where a task of a group stands (contracts/web-api.md): `queued | building | repairing |
/// accepted | merged | waiting | dropped`.
fn group_task_state(run: &Run, t: &super::state::GroupTask) -> &'static str {
    if t.merged.is_some() {
        return "merged";
    }
    if t.dropped {
        return "dropped";
    }
    let Some(l) = run.state.feature_loop.as_ref() else {
        return "queued";
    };
    let status = l.tasks.get(&t.id);
    match status.map(|s| s.state) {
        Some(TaskState::Waiting) => return "waiting",
        Some(TaskState::Accepted) if t.working.is_none() => return "accepted",
        _ => {}
    }
    if t.working.is_some() {
        return "repairing";
    }
    let since = status.map_or(0, |s| s.since);
    match run
        .state
        .steps
        .values()
        .rev()
        .find(|s| {
            s.task.as_deref() == Some(t.id.as_str())
                && s.queued_seq > since
                && matches!(s.kind, StepKind::Build | StepKind::Repair)
        })
        .map(|s| s.kind)
    {
        None => "queued",
        Some(StepKind::Repair) => "repairing",
        Some(_) => "building",
    }
}

/// What a failed criterion expected: its scenario's **Then** part, without the emphasis and the
/// `(must fail)` mark (008, contracts/loop-view.md).
fn expected_of(text: &str) -> String {
    let plain = text.replace("**", "");
    // The marker of the scenario's outcome: its **Then**, else the last "Then " (a "Then" in the
    // Given part is no outcome).
    let then = text
        .rfind("**Then**")
        .map(|i| text[i + "**Then**".len()..].replace("**", ""))
        .or_else(|| {
            plain
                .rfind("Then ")
                .map(|i| plain[i + "Then ".len()..].to_string())
        })
        .unwrap_or_else(|| plain.clone());
    then.replace("(must fail)", "")
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::expected_of;

    #[test]
    fn what_a_failed_criterion_expected() {
        assert_eq!(
            expected_of("**Given** a cart, **When** it is shown, **Then** the total is `4.98`."),
            "the total is `4.98`"
        );
        assert_eq!(
            expected_of(
                "**Given** a file \"Then what\", **When** read, **Then** it is refused (must fail)"
            ),
            "it is refused"
        );
        assert_eq!(expected_of("Given x, When y, Then z happens."), "z happens");
        assert_eq!(expected_of("The total is exact."), "The total is exact");
    }
}
