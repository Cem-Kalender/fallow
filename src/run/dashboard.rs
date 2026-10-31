//! The dashboard's view of the loops (007 FR-003, FR-003a, FR-004; mockup screen 1): what needs
//! the engineer, as typed items, and each running loop's card with its phase progress and what
//! its agents do, in a few words.

use serde_json::{Value, json};

use super::engine::{Busy, Ctx, Run};
use super::events::*;
use super::state::LoopState;
use crate::agent::Activity;

/// A phase's title as the dashboard shows it: "US1 Add to cart" rather than Spec Kit's
/// "User Story 1 - Add to cart (Priority: P1) 🎯 MVP".
pub fn short_title(story: Option<&str>, title: &str) -> String {
    let t = title.split(" (Priority").next().unwrap_or(title);
    let t = t
        .trim_end_matches(|c: char| !c.is_ascii() || c.is_whitespace())
        .trim();
    match (story, t.split_once(" - ")) {
        (Some(s), Some((_, rest))) => format!("{s} {}", rest.trim()),
        _ => t.to_string(),
    }
}

/// The task the loop is on: the current or waiting one, else the first not accepted.
fn current_task(l: &LoopState) -> Option<String> {
    let all = l.phases.iter().flat_map(|p| p.spec.tasks.iter());
    all.clone()
        .find(|t| {
            l.tasks
                .get(&t.id)
                .is_some_and(|s| matches!(s.state, TaskState::Current | TaskState::Waiting))
        })
        .or_else(|| {
            all.clone().find(|t| {
                l.tasks
                    .get(&t.id)
                    .is_none_or(|s| s.state != TaskState::Accepted)
            })
        })
        .map(|t| t.id.clone())
}

/// What the step is doing now, from its last line: "running `npm test`", "reading api/tags.ts".
fn doing(run: &Run, n: u32) -> Option<String> {
    // Only the end of the step's record: it can be long, and the dashboard is read often.
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(run.steps_dir().join(format!("{n}.jsonl"))).ok()?;
    let len = f.metadata().ok()?.len();
    let from = len.saturating_sub(64 * 1024);
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A line cut by the start is skipped by the parse below.
    let last: Activity = text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Activity>(l).ok())
        .find(|a| !matches!(a.kind.as_str(), "say" | "result" | "error" | "refused"))?;
    let what: String = last
        .text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    Some(match last.kind.as_str() {
        "run" => format!("running `{what}`"),
        "read" => format!("reading {what}"),
        "edit" | "write" => format!("editing {what}"),
        "search" => format!("searching {what}"),
        "ask" => "asking a question".to_string(),
        "progress" => what,
        _ => format!("using {what}"),
    })
}

/// A running loop's card.
pub fn card(run: &Run, _ctx: &Ctx, project_name: &str) -> Option<Value> {
    let s = &run.state;
    let l = s.feature_loop.as_ref()?;
    let phases: Vec<Value> = l
        .phases
        .iter()
        .map(|p| {
            let done = p
                .spec
                .tasks
                .iter()
                .filter(|t| {
                    l.tasks
                        .get(&t.id)
                        .is_some_and(|x| x.state == TaskState::Accepted)
                })
                .count();
            json!({
                "n": p.spec.n, "title": short_title(p.spec.story.as_deref(), &p.spec.title),
                "story": p.spec.story, "tasks": p.spec.tasks.len(), "done": done, "state": p.state,
            })
        })
        .collect();
    let total: usize = l.phases.iter().map(|p| p.spec.tasks.len()).sum();
    let done = l
        .tasks
        .values()
        .filter(|t| t.state == TaskState::Accepted)
        .count();
    let current = l.phases.iter().find(|p| p.state != PhaseState::Done);
    let task = current_task(l);
    // The agents, each in a few words.
    // The pipeline as the loop recorded it, without reading the definitions again.
    let pl: Option<crate::defs::pipeline::Raw> = toml::from_str(&l.pipeline).ok();
    let running = match run.busy {
        Some(Busy::Step(n)) => s.steps.get(&n),
        _ => None,
    };
    let waiting = super::questions::waiting(l);
    let mut names: Vec<String> = Vec::new();
    if let Some(pl) = &pl {
        for st in &pl.steps {
            if st.agent != crate::defs::pipeline::GATES && !names.contains(&st.agent) {
                names.push(st.agent.clone());
            }
        }
    }
    let agents: Vec<Value> = names
        .iter()
        .map(|name| {
            let mine = running.filter(|r| r.def.as_deref() == Some(name.as_str()));
            let asks = waiting.filter(|q| {
                s.steps.get(&q.step).and_then(|x| x.def.as_deref()) == Some(name.as_str())
            });
            let (state, text) = if let Some(q) = asks {
                ("asks", format!("{name} waits for your answer ({})", q.id))
            } else if let Some(r) = mine {
                let on = r
                    .task
                    .clone()
                    .or_else(|| r.part.clone())
                    .unwrap_or_default();
                let what = match r.kind {
                    StepKind::Review => format!("checking {on}"),
                    StepKind::Handover => format!("writing the handover of {on}"),
                    StepKind::Repair => format!("repairing {on}"),
                    _ => format!("on {on}"),
                };
                let text = match doing(run, r.n) {
                    Some(d) => format!("{name} {what} · {d}"),
                    None => format!("{name} {what}"),
                };
                ("working", text)
            } else if s
                .steps
                .values()
                .any(|x| x.def.as_deref() == Some(name.as_str()))
            {
                ("idle", format!("{name} idle"))
            } else {
                let checker = pl.as_ref().is_some_and(|p| {
                    p.steps.iter().any(|st| {
                        &st.agent == name && st.when == crate::defs::pipeline::When::Checkpoint
                    })
                });
                let until = current
                    .and_then(|p| p.spec.story.clone())
                    .map(|st| format!(" until the {st} checkpoint"))
                    .unwrap_or_default();
                (
                    "idle",
                    if checker {
                        format!("{name} idle{until}")
                    } else {
                        format!("{name} idle")
                    },
                )
            };
            json!({ "name": name, "state": state, "text": text })
        })
        .collect();
    let now = super::time::unix_now();
    Some(json!({
        "run": s.id, "project": l.project, "project_name": project_name, "feature": l.feature,
        "pipeline": pl.as_ref().map_or("custom", |p| p.name.as_str()), "stage": s.stage, "minutes": s.minutes_used(now), "cost_usd": s.cost_usd,
        "phases": phases, "tasks": total, "done": done,
        "current": current.map(|p| json!({ "n": p.spec.n, "title": short_title(p.spec.story.as_deref(), &p.spec.title) })),
        "task": task, "agents": agents, "waiting": run.waiting, "paused": s.stage == Stage::Paused,
        "needs": super::engine::needs(run),
    }))
}

/// What the loop needs from the engineer, as "Needs you" items (FR-003a): each question that
/// waits for an answer, in the order asked; a feature ready to merge (or done but not
/// verified); and a stop at a limit, a repair cap or an error.
pub fn needs(run: &Run, project_name: &str) -> Vec<Value> {
    let s = &run.state;
    let Some(l) = s.feature_loop.as_ref() else {
        return Vec::new();
    };
    if s.stage.is_final() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let base = |kind: &str, at: &str| {
        json!({
            "kind": kind, "run": s.id, "project": l.project, "project_name": project_name,
            "feature": l.feature, "at": at,
        })
    };
    let mut waiting: Vec<&super::state::QuestionEntry> = l
        .questions
        .values()
        .filter(|q| q.state == super::events::QuestionState::WaitingEngineer)
        .collect();
    // q2 before q10.
    waiting.sort_by_key(|q| {
        q.id.trim_start_matches('q')
            .parse::<u64>()
            .unwrap_or(u64::MAX)
    });
    for q in waiting {
        let at = q.history.last().map(|h| h.at.clone()).unwrap_or_default();
        let mut v = base("question", &at);
        v["key"] = json!(format!("{}:question:{}", s.id, q.id));
        v["question"] = json!(q.id);
        v["task"] = json!(q.task);
        v["agent"] = json!(s.steps.get(&q.step).and_then(|x| x.def.clone()));
        v["text"] = json!(q.text);
        out.push(v);
    }
    if s.stage == Stage::Ready {
        let verified = l.outcome.as_ref().is_some_and(|o| o.0);
        let mut v = base(if verified { "ready" } else { "unverified" }, &s.stage_at);
        let tasks: usize = l.phases.iter().map(|p| p.spec.tasks.len()).sum();
        let mut stories: Vec<&str> = l
            .phases
            .iter()
            .filter_map(|p| p.spec.story.as_deref())
            .collect();
        stories.dedup();
        // The latest verdict of each story's checkpoint.
        let (mut passed, mut criteria) = (0, 0);
        for st in &stories {
            if let Some(v) = s
                .steps
                .values()
                .rev()
                .filter(|x| x.kind == StepKind::Review && x.part.as_deref() == Some(*st))
                .find_map(|x| s.verdicts.iter().find(|v| v.step == x.n))
            {
                criteria += v.answers.len();
                passed += v.answers.iter().filter(|a| a.passed).count();
            }
        }
        v["key"] = json!(format!(
            "{}:ready:{}",
            s.id,
            s.integrated.clone().unwrap_or_default()
        ));
        v["tasks"] = json!(tasks);
        v["text"] = json!(if verified {
            let gates = if s.gates.is_empty() {
                ""
            } else {
                " · gates green vs baseline"
            };
            format!(
                "All {} user stor{} reviewed · {passed}/{criteria} criteria{gates}",
                stories.len(),
                if stories.len() == 1 { "y" } else { "ies" }
            )
        } else {
            format!(
                "Done, not verified by Fallow: {}",
                l.outcome
                    .as_ref()
                    .map(|o| o.1.join(", "))
                    .unwrap_or_default()
            )
        });
        out.push(v);
        return out;
    }
    let Some(n) = s.needs.as_ref() else {
        return out;
    };
    // A loop waiting for its questions waits for nothing else, unless it also hit a limit.
    if !out.is_empty() && s.limit_reached.is_none() {
        return out;
    }
    let mut v = base("stopped", &n.at);
    v["key"] = json!(format!("{}:needs:{}", s.id, n.at));
    v["text"] = json!(n.why);
    v["what"] = json!(if s.limit_reached.is_some() {
        "stopped at a limit"
    } else if n.why.contains("repair round") {
        "stopped at the repair cap"
    } else {
        "stopped and needs you"
    });
    v["task"] = json!(current_task(l));
    out.push(v);
    out
}
