//! Long polling (006 research R-6; 007 research S-4): a loop's scope. A page that is behind gets
//! an answer at once; one that is up to date waits until the loop changes (an event or an
//! activity line).

mod common;

use std::time::{Duration, Instant};

use common::*;
use fallow::run::engine::{Msg, Reply, Request};

#[test]
fn a_wait_answers_at_once_when_behind_and_with_the_next_change_otherwise() {
    let root = scratch("wait");
    // The loop's first task works for 1.5 s after its first line.
    let steps = calc_loop_ok().replacen(
        "part = \"T001\"
",
        "part = \"T001\"
sleep_ms = 1500
",
        1,
    );
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = e.tx.clone();
    let wait = |after: u64| {
        let (rtx, rrx) = std::sync::mpsc::channel();
        tx.send(Msg::Request(
            Request::Events {
                scope: format!("loop:{id}"),
                after,
            },
            rtx,
        ))
        .unwrap();
        rrx
    };
    // Behind: answered at once.
    let Reply::Ok(m) = wait(0).recv_timeout(Duration::from_secs(2)).unwrap() else {
        panic!()
    };
    assert!(m["seq"].as_u64().unwrap() > 0);
    // Wait until the task's step has said its first line (it then works for 1.5 s).
    let v = wait_for(&tx, id, "the task running", |v| {
        v["steps"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["part"] == "T001" && s["started"].is_string())
    });
    let n = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .find(|s| s["part"] == "T001")
        .and_then(|s| s["n"].as_u64())
        .unwrap() as u32;
    // The step works for 1.5 s after its first line: wait for that line, then the next change
    // is the step's end.
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        let lines = ask(&tx, Request::Activity { id, n }).unwrap_or_default();
        if lines.as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(100));
    let Reply::Ok(now) = wait(0).recv_timeout(Duration::from_secs(2)).unwrap() else {
        panic!()
    };
    let mark = now["seq"].as_u64().unwrap();
    // Up to date: answered only when the loop changes.
    let start = Instant::now();
    let Reply::Ok(m) = wait(mark).recv_timeout(Duration::from_secs(30)).unwrap() else {
        panic!()
    };
    assert!(
        m["seq"].as_u64().unwrap() > mark,
        "answered with a change: {m}"
    );
    assert!(
        start.elapsed() > Duration::from_millis(300),
        "it waited for the change"
    );
    // A scope that does not exist is refused.
    let (rtx, rrx) = std::sync::mpsc::channel();
    tx.send(Msg::Request(
        Request::Events {
            scope: "loop:999".into(),
            after: 0,
        },
        rtx,
    ))
    .unwrap();
    assert!(matches!(
        rrx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Reply::Err(_)
    ));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
