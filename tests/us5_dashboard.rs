//! 007 US5 (T066; FR-003, FR-003a, FR-004): two loops in two projects on the dashboard, with
//! their phase progress; their questions in the "Needs you" inbox; a feature ready to merge and
//! a loop stopped at its limit there too; and the cap on agents at once, with the loop above it
//! waiting for a slot.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;
use std::time::{Duration, Instant};

const FLOATS: &str = "Should add() accept floats?";

fn dashboard(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>) -> Value {
    ask(tx, Request::Dashboard).unwrap()
}

/// Waits until the dashboard satisfies `pred`, checking on the way that no more agents run at
/// once than the cap allows.
fn wait_dash(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, what: &str, seen_slot: &mut bool, pred: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let d = dashboard(tx);
        assert!(d["agents_running"].as_u64().unwrap() <= 1, "within the cap: {}", d["agents_running"]);
        if d["loops"].as_array().unwrap().iter().any(|l| l["waiting"].as_str().is_some_and(|w| w.starts_with("waiting for a slot"))) {
            *seen_slot = true;
        }
        if pred(&d) {
            return d;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {d}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn two_loops_their_progress_and_what_needs_you() {
    let root = scratch("dashboard");
    std::fs::write(root.join("settings.extra.toml"), "[defaults]\nmax_parallel = 1\n").unwrap();
    // Both loops' T001 ask a question the spec does not settle; the lead relays both.
    let ask_t001 = task_step("T001", &[("calc/__init__.py", "")], &format!("ask = {FLOATS:?}\nsleep_ms = 6000\ncost = 0.05"))
        .replacen("status = \"done\"", "status = \"blocked\", question = \"$ANSWER\"", 1);
    let relay = "[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = { outcome = \"relay\", answer = \"\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{ label = \"refuse them\", recommended = true }] }\n\n";
    let rest = calc_loop_ok().split_once("[[step]]\nkind = \"handover\"").map(|(_, r)| format!("[[step]]\nkind = \"handover\"{r}")).unwrap();
    let steps = [
        lead_writes_calc(),
        lead_writes_calc(),
        ask_t001.clone(),
        ask_t001,
        relay.to_string(),
        relay.to_string(),
        task_step("T001", &[("calc/__init__.py", "")], "when = \"The engineer answers\""),
        rest,
    ]
    .concat();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, steps).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let shop = lock_calc_in(tx, &repo_named(&root, "shop"), None, Some((5.0, 60)), None);
    // The second loop's budget is spent by its first step: it stops at its limit later.
    let notes = lock_calc_in(tx, &repo_named(&root, "notes"), None, Some((0.02, 60)), None);
    let mut slot = false;

    // Both loops on the dashboard, with their phases; then both questions in the inbox.
    let d = wait_dash(tx, "both questions", &mut slot, |d| {
        d["needs_you"].as_array().unwrap().iter().filter(|n| n["kind"] == "question").count() == 2
    });
    assert!(slot, "the loop above the cap waited for a slot");
    let loops = d["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 2, "{loops:?}");
    for l in loops {
        assert_eq!(l["phases"].as_array().unwrap().len(), 3, "{l}");
        assert_eq!(l["tasks"], 4);
        assert_eq!(l["current"]["n"], 1);
        assert_eq!(l["task"], "T001");
        assert!(l["pipeline"].is_string());
        let builder = l["agents"].as_array().unwrap().iter().find(|a| a["name"] == "builder").unwrap();
        assert_eq!(builder["state"], "asks", "{builder}");
        assert!(builder["text"].as_str().unwrap().contains("waits for your answer"));
    }
    let names: Vec<&str> = loops.iter().map(|l| l["project_name"].as_str().unwrap()).collect();
    assert!(names.contains(&"shop") && names.contains(&"notes"), "{names:?}");
    let needs = d["needs_you"].as_array().unwrap();
    for n in needs {
        assert_eq!(n["text"], FLOATS);
        assert_eq!(n["feature"], "001-calc");
        assert_eq!(n["task"], "T001");
        assert_eq!(n["agent"], "builder");
        assert!(n["at"].as_str().unwrap().len() >= 19);
    }
    let from: Vec<&str> = needs.iter().map(|n| n["project_name"].as_str().unwrap()).collect();
    assert!(from.contains(&"shop") && from.contains(&"notes"), "questions from either loop: {from:?}");

    // The engineer answers both: one loop goes on to ready, the other stops at its limit.
    for id in [shop, notes] {
        ask(tx, Request::QuestionAnswer { id, q: "q1".into(), answer: "Ints only".into() }).unwrap();
    }
    let d = wait_dash(tx, "ready and stopped", &mut slot, |d| {
        let n = d["needs_you"].as_array().unwrap();
        n.iter().any(|x| x["kind"] == "ready") && n.iter().any(|x| x["kind"] == "stopped")
    });
    let needs = d["needs_you"].as_array().unwrap();
    assert_eq!(needs.len(), 2, "{needs:?}");
    let ready = needs.iter().find(|x| x["kind"] == "ready").unwrap();
    assert_eq!(ready["run"], shop);
    assert_eq!(ready["project_name"], "shop");
    assert_eq!(ready["tasks"], 4);
    let text = ready["text"].as_str().unwrap();
    assert!(text.contains("2 user stories reviewed") && text.contains("4/4 criteria") && text.contains("gates green"), "{text}");
    let stopped = needs.iter().find(|x| x["kind"] == "stopped").unwrap();
    assert_eq!(stopped["run"], notes);
    assert_eq!(stopped["what"], "stopped at a limit", "{stopped}");
    // The features table: one ready to merge, one still building.
    let stages: Vec<&str> = d["features"].as_array().unwrap().iter().map(|f| f["stage"].as_str().unwrap()).collect();
    assert!(stages.contains(&"ready_to_merge") && stages.contains(&"building"), "{stages:?}");
    // A loop that is ready is no longer running; the stopped one still is.
    let loops = d["loops"].as_array().unwrap();
    assert!(loops.iter().any(|l| l["run"] == notes && l["paused"] == true), "{loops:?}");
    assert!(d["cost_today"].as_f64().unwrap() >= 0.1);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
