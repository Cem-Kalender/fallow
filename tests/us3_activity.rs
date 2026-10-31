//! 007 US3 (T054; FR-017, FR-018, SC-005): a loop's activity. Runtime events become feed entries
//! attributed to the agent and the task: narration, edits with their diff, commands with their
//! output and result (ok, failed, refused). Thinking is never shown. Fallow's own checks are in
//! the feed. Activity reaches a waiting page within 2 s (p95).

mod common;

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use common::*;
use fallow::run::activity::Filter;
use fallow::run::engine::{Msg, Reply, Request};
use serde_json::Value;

fn activity(tx: &Sender<Msg>, id: u64, f: Filter) -> Value {
    ask(tx, Request::LoopActivity { id, filter: f }).unwrap()
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn the_feed_shows_each_agents_work_and_fallows_checks() {
    let root = scratch("activity");
    let t2 = task_step("T002", &[("calc/__init__.py", CALC_INIT)], "")
        .replacen(
            "part = \"T002\"\n",
            "part = \"T002\"\nsay = \"I'll write add() with its type checks.\"\nthink = \"SECRET-THOUGHT\"\nrun = [{ command = \"python -m unittest\", output = \"Ran 2 tests\\nOK\" }, { command = \"python -m unittest tests.test_none\", output = \"FAILED (errors=1)\", ok = false }, { command = \"curl http://example.invalid\" }]\n",
            1,
        );
    let mut steps = calc_loop_ok();
    let at = steps
        .find("[[step]]\nkind = \"build\"\npart = \"T002\"")
        .unwrap();
    let end = steps[at + 1..]
        .find("[[step]]")
        .map_or(steps.len(), |e| at + 1 + e);
    steps.replace_range(at..end, &t2);
    // T001 talks for a while, so the latency of the feed can be measured on a live step.
    let steps = steps.replacen(
        "part = \"T001\"\n",
        "part = \"T001\"\nsay = \"Setting up the package.\"\nsleep_ms = 2500\n",
        1,
    );
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;

    // SC-005: each new line reaches a page waiting on the loop's scope within 2 s.
    let mut seen = 0usize;
    let mut delays: Vec<u64> = Vec::new();
    let mut mark = 0u64;
    let until = Instant::now() + Duration::from_secs(170);
    loop {
        let v = ask(tx, Request::Get { id }).unwrap();
        if v["stage"] == "ready" || Instant::now() > until {
            break;
        }
        let (rtx, rrx) = std::sync::mpsc::channel();
        tx.send(Msg::Request(
            Request::Events {
                scope: format!("loop:{id}"),
                after: mark,
            },
            rtx,
        ))
        .unwrap();
        let Ok(Reply::Ok(m)) = rrx.recv_timeout(Duration::from_secs(30)) else {
            continue;
        };
        let now = unix_ms();
        mark = m["seq"].as_u64().unwrap_or(mark);
        let a = activity(
            tx,
            id,
            Filter {
                after: seen,
                ..Filter::default()
            },
        );
        for en in a["entries"].as_array().unwrap() {
            if en["agent"] != "fallow" && en["agent"] != "you" {
                delays.push(now.saturating_sub(en["at"].as_u64().unwrap_or(now)));
            }
        }
        seen = a["next"].as_u64().unwrap() as usize;
    }
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    delays.sort_unstable();
    assert!(delays.len() >= 5, "lines seen live: {delays:?}");
    let p95 = delays[(delays.len() * 95 / 100).min(delays.len() - 1)];
    assert!(p95 <= 2000, "p95 {p95} ms over {delays:?}");

    let all = activity(tx, id, Filter::default());
    let entries = all["entries"].as_array().unwrap();
    assert!(
        !all.to_string().contains("SECRET-THOUGHT"),
        "no thinking is shown"
    );
    let find = |pred: &dyn Fn(&Value) -> bool| entries.iter().find(|e| pred(e)).cloned();
    let say = find(&|e| {
        e["kind"] == "say"
            && e["text"]
                .as_str()
                .unwrap()
                .contains("add() with its type checks")
    })
    .expect("the builder's narration");
    assert_eq!(say["agent"], "builder");
    assert_eq!(say["task"], "T002");
    let write = find(&|e| e["kind"] == "write" && e["task"] == "T002").expect("the write");
    assert!(write["text"].as_str().unwrap().contains("calc/__init__.py"));
    assert!(
        write["detail"].as_str().unwrap().contains("def add"),
        "{write}"
    );
    let ok =
        find(&|e| e["kind"] == "run" && e["text"] == "python -m unittest").expect("the test run");
    assert_eq!(ok["result"], "ok");
    assert!(ok["output"].as_str().unwrap().contains("Ran 2 tests"));
    let failed =
        find(&|e| e["kind"] == "run" && e["text"].as_str().unwrap().contains("tests.test_none"))
            .unwrap();
    assert_eq!(failed["result"], "failed");
    let refused =
        find(&|e| e["kind"] == "run" && e["text"].as_str().unwrap().contains("curl")).unwrap();
    assert_eq!(refused["result"], "refused");
    // Fallow's checks: the commit and the gates of T002, and the reviews.
    let commit = find(&|e| {
        e["agent"] == "fallow"
            && e["task"] == "T002"
            && e["text"].as_str().unwrap().contains("committed")
    })
    .unwrap();
    assert_eq!(commit["result"], "ok");
    assert!(
        find(
            &|e| e["agent"] == "fallow" && e["text"].as_str().unwrap().starts_with("gates on T002")
        )
        .is_some()
    );
    assert!(
        find(&|e| e["agent"] == "fallow" && e["text"].as_str().unwrap().contains("approved US1"))
            .is_some()
    );
    // In time order, each with its place.
    let ats: Vec<u64> = entries.iter().map(|e| e["at"].as_u64().unwrap()).collect();
    assert!(ats.windows(2).all(|w| w[0] <= w[1]));
    // Filters: by agent, by kind, by task, and after a place.
    let f = activity(
        tx,
        id,
        Filter {
            agent: Some("fallow".into()),
            ..Filter::default()
        },
    );
    assert!(
        f["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["agent"] == "fallow")
    );
    let f = activity(
        tx,
        id,
        Filter {
            kind: Some("run".into()),
            ..Filter::default()
        },
    );
    assert_eq!(f["entries"].as_array().unwrap().len(), 3);
    let f = activity(
        tx,
        id,
        Filter {
            task: Some("T002".into()),
            ..Filter::default()
        },
    );
    assert!(
        f["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["task"] == "T002")
    );
    let n = entries.len();
    let f = activity(
        tx,
        id,
        Filter {
            after: n - 2,
            ..Filter::default()
        },
    );
    assert_eq!(f["entries"].as_array().unwrap().len(), 2);

    // The loop's screen: its agents, checks and totals.
    let s = ask(tx, Request::Loop { id }).unwrap();
    let builder = s["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "builder")
        .unwrap()
        .clone();
    assert_eq!(builder["runtime"], "claude");
    assert_eq!(builder["where"], "fallow/001-calc/build");
    assert!(builder["cost_usd"].as_f64().unwrap() > 0.0);
    assert_eq!(s["used"]["tasks"], 4);
    assert_eq!(s["checks"]["commits"].as_array().unwrap().len(), 4);
    assert!(
        s["checks"]["gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["ok"] == true)
    );
    let us1 = s["checks"]["stories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "US1")
        .unwrap()
        .clone();
    assert!(
        us1["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["state"] == "passed"),
        "{us1}"
    );
    assert_eq!(s["stage"], "ready");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
