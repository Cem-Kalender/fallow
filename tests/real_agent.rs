//! A solo run with the real Claude Code on a tiny task (006 quickstart V1, automated). Ignored by
//! default: it needs `claude` signed in and costs a little. Run it by hand:
//!
//!     cargo test --test real_agent -- --ignored --nocapture

mod common;

use common::*;
use fallow::config::Settings;
use fallow::run::engine::Request;

const LONG: std::time::Duration = std::time::Duration::from_secs(20 * 60);

#[test]
#[ignore = "uses the real Claude Code and costs a little"]
fn a_real_solo_run_on_a_tiny_task() {
    let root = scratch("real");
    let repo = repo(&root);
    let mut n = Vec::new();
    let settings = Settings::parse(
        "settings.toml",
        "[defaults]\nautonomy = \"high\"\nlimit_usd = 3\nstep_min = 10\n[roles.planner]\nmodel = \"haiku\"\n[roles.builder]\nmodel = \"haiku\"\n[roles.reviewer]\nmodel = \"haiku\"\n",
        &mut n,
    );
    assert!(n.is_empty(), "{n:?}");
    let (tx, _h) = fallow::service::engine_for_tests(root.join("data"), settings).unwrap();
    let id = create(
        &tx,
        &repo,
        "Add calc.py with add(a, b) that returns a + b for ints and raises TypeError for anything else, with unittest tests in tests/test_calc.py (and tests/__init__.py).",
    );
    let mut turns = 0;
    loop {
        let v = wait_up_to(&tx, id, "the planner's turn", LONG, |v| {
            v["plan_approvable"] == true
                || (v["busy"] == ""
                    && v["steps"]
                        .as_object()
                        .is_some_and(|s| s.values().all(|x| !x["end"].is_null())))
        });
        for t in v["conversation"].as_array().unwrap() {
            println!("{}: {}", t["who"], t["text"]);
        }
        if v["plan_approvable"] == true {
            break;
        }
        println!("problems: {}", v["plan_problems"]);
        turns += 1;
        assert!(turns < 4, "no approvable plan after {turns} turns");
        ask(
            &tx,
            Request::Say {
                id,
                text: "No questions from me: write the plan with one builder and its criteria now, commit it, and set ready.".into(),
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_up_to(&tx, id, "ready or stuck", LONG, |v| {
        v["stage"] == "ready" || v["needs"].is_object() || v["stage"] == "paused"
    });
    for s in v["steps"].as_object().unwrap().values() {
        println!(
            "step {} {} {}: cost {} verified {}",
            s["n"], s["kind"], s["model"], s["cost_usd"], s["verified"]
        );
    }
    println!("gates: {}", v["gate_runs"]);
    println!("verdicts: {}", v["verdicts"]);
    println!("cost: {}", v["cost_usd"]);
    assert_eq!(v["stage"], "ready", "needs: {}", v["needs"]);
    ask(&tx, Request::Merge { id }).unwrap();
    wait_up_to(&tx, id, "done", LONG, |v| v["stage"] == "done");
    println!(
        "merged: {}",
        git(&repo, &["log", "--oneline", "-3", "main"])
    );
}
