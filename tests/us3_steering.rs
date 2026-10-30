//! 007 US3 (FR-019): the engineer's steering on the loop page. A pause lets the step under way
//! finish and starts nothing after it until "go on"; a note for the next step reaches exactly one
//! step, of the agent it names, once.

mod common;

use std::time::Duration;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

const GENERAL: &str = "Keep the handover short.";
const FOR_REVIEWER: &str = "Check floats too.";

#[test]
fn a_pause_holds_and_a_note_reaches_one_step() {
    let root = scratch("steering");
    let steps = calc_loop_ok().replacen("part = \"T001\"\n", "part = \"T001\"\nsleep_ms = 4000\n", 1);
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    wait_for(tx, id, "T001 running", |v| {
        v["steps"].as_object().unwrap().values().any(|s| s["part"] == "T001" && !s["started"].is_null())
    });
    ask(tx, Request::Pause { id }).unwrap();
    assert!(ask(tx, Request::Pause { id }).is_err(), "already paused");
    ask(tx, Request::Note { id, text: FOR_REVIEWER.into(), agent: Some("reviewer".into()) }).unwrap();
    ask(tx, Request::Note { id, text: GENERAL.into(), agent: None }).unwrap();
    assert!(ask(tx, Request::Note { id, text: "  ".into(), agent: None }).is_err(), "an empty note");

    // T001 ends; nothing starts while paused.
    let v = wait_for(tx, id, "T001's end", |v| {
        v["steps"].as_object().unwrap().values().any(|s| s["part"] == "T001" && !s["end"].is_null())
    });
    let count = v["steps"].as_object().unwrap().len();
    std::thread::sleep(Duration::from_secs(3));
    let v = ask(tx, Request::Get { id }).unwrap();
    assert_eq!(v["steps"].as_object().unwrap().len(), count, "nothing started while paused");
    assert_eq!(v["stage"], "paused");

    ask(tx, Request::GoOn { id }).unwrap();
    assert!(ask(tx, Request::GoOn { id }).is_err(), "not paused any more");
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");

    // Who got which note: from the instruction each step was given.
    let given = |s: &Value| {
        std::fs::read_to_string(
            root.join("data/runs").join(id.to_string()).join("steps").join(format!("{}.prompt", s["n"])),
        )
        .unwrap()
    };
    let mut steps: Vec<&Value> = v["steps"].as_object().unwrap().values().collect();
    steps.sort_by_key(|s| s["n"].as_u64());
    let general: Vec<String> = steps
        .iter()
        .filter(|s| given(s).contains(GENERAL))
        .map(|s| format!("{} {}", s["kind"], s["part"]))
        .collect();
    assert_eq!(general, vec!["\"handover\" \"phase-1\"".to_string()], "the first step after the note");
    let reviewer: Vec<String> = steps
        .iter()
        .filter(|s| given(s).contains(FOR_REVIEWER))
        .map(|s| format!("{} {} {}", s["kind"], s["part"], s["def"]))
        .collect();
    assert_eq!(reviewer, vec!["\"review\" \"US1\" \"reviewer\"".to_string()], "the reviewer's first step");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
