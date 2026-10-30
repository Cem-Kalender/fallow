//! 007 US2 (T053; research S-19): agent steps share a cap across loops and conversations. The
//! lead's turn in a chat goes ahead of loop steps, and a loop step above the cap waits with
//! "waiting for a slot", then goes on by itself.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;
use serde_json::Value;
use std::time::{Duration, Instant};

fn started(v: &Value, kind: &str, part: &str) -> bool {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .any(|s| s["kind"] == kind && s["part"] == part && !s["started"].is_null())
}

#[test]
fn a_chat_turn_goes_first_and_the_loop_waits_for_a_slot() {
    let root = scratch("slots");
    std::fs::write(root.join("settings.extra.toml"), "[defaults]\nmax_parallel = 1\n").unwrap();
    let mut steps = calc_loop_ok().replacen(
        "part = \"T001\"\n",
        "part = \"T001\"\nsleep_ms = 4000\n",
        1,
    );
    steps.push_str(
        "[[step]]\nkind = \"lead_turn\"\nwhen = \"project conversation\"\nsleep_ms = 9000\noutput = { reply = \"Thought it over.\", step = \"none\" }\n\n",
    );
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "T001 running", |v| started(v, "build", "T001"));
    let project = v["feature_loop"]["project"].as_str().unwrap().to_string();

    // The engineer asks the lead something while T001 holds the only slot: the turn starts.
    let pc = sreq(tx, Req::ProjectConversation { project }).unwrap();
    let conv = pc["conversation"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "How is it going?".into(),
        },
    )
    .unwrap();
    wait_conv(tx, &conv, "the turn under way", |c| c["busy"] == true);

    // T001 ends; the handover waits while the chat turn runs.
    let v = wait_for(tx, id, "waiting for a slot", |v| {
        v["waiting"].as_str().is_some_and(|w| w.starts_with("waiting for a slot"))
    });
    assert!(!started(&v, "handover", "phase-1"));
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        let c = sreq(tx, Req::Conversation { id: conv.clone() }).unwrap();
        if c["busy"] == false {
            break;
        }
        let v = ask(tx, Request::Get { id }).unwrap();
        assert!(!started(&v, "handover", "phase-1"), "no step past the cap");
        std::thread::sleep(Duration::from_millis(200));
    }

    // When the turn ends, the loop goes on by itself.
    turn_ended(tx, &conv, 1);
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert!(v["waiting"].is_null());
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_step_waits_for_a_free_slot_and_for_memory_room() {
    use fallow::run::engine::{STEP_MB, Slots};
    let s = Slots::default();
    // Two free slots and room for three steps: two start, the third waits for a slot.
    s.set(2, 3 * STEP_MB);
    assert!(s.take().is_ok());
    assert!(s.take().is_ok());
    let why = s.take().unwrap_err();
    assert!(why.starts_with("waiting for a slot") && why.contains("most agents"), "{why}");
    // A free slot but too little memory: it waits for room.
    s.set(4, STEP_MB - 1);
    let why = s.take().unwrap_err();
    assert!(why.starts_with("waiting for a slot") && why.contains("memory"), "{why}");
}
