//! 006 research R-6: long polling. A page that is behind gets an answer at once; one that is up
//! to date waits until the run changes.

mod common;

use std::time::{Duration, Instant};

use common::*;
use fallow::run::engine::{Msg, Reply, Request};

#[test]
fn a_wait_answers_at_once_when_behind_and_with_the_next_change_otherwise() {
    let root = scratch("wait");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        "[[step]]\nkind = \"plan_turn\"\nsleep_ms = 1500\noutput = { reply = \"hello\", ready = false }\n",
    )
    .unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "t");
    let wait = |seq: u64, activity: u64| {
        let (rtx, rrx) = std::sync::mpsc::channel();
        tx.send(Msg::Request(Request::Wait { id, seq, activity }, rtx))
            .unwrap();
        rrx
    };
    // Behind: answered at once.
    let r = wait(0, 0).recv_timeout(Duration::from_secs(2)).unwrap();
    let Reply::Ok(m) = r else { panic!() };
    let seq = m["seq"].as_u64().unwrap();
    assert!(seq > 0);
    // Up to date (the turn is under way): answered only when it changes something.
    let v = wait_for(&tx, id, "the turn running", |v| {
        v["steps"]["1"]["started"].is_string()
    });
    let seq = v["seq"].as_u64().unwrap().max(seq);
    // The step has said its first line (it then works for 1.5 s).
    let mut activity = 0;
    let mut seq = seq;
    for _ in 0..100 {
        let Reply::Ok(now) = wait(0, 0).recv_timeout(Duration::from_secs(2)).unwrap() else {
            panic!()
        };
        activity = now["activity"].as_u64().unwrap();
        seq = now["seq"].as_u64().unwrap();
        if activity > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let start = Instant::now();
    let rx = wait(seq, activity);
    let Reply::Ok(m) = rx.recv_timeout(Duration::from_secs(30)).unwrap() else {
        panic!()
    };
    assert!(
        m["seq"].as_u64().unwrap() > seq || m["activity"].as_u64().unwrap() > activity,
        "answered with a change: {m}"
    );
    assert!(
        start.elapsed() > Duration::from_millis(500),
        "it waited for the change"
    );
    let _ = std::fs::remove_dir_all(&root);
}
