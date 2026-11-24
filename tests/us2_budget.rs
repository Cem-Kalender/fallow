//! 007 US2 (T043; FR-016a): a loop's budget is suggested from its tasks; the loop pauses at the
//! limit and asks, offering "continue for one step" and "raise the limit"; paused time and time
//! waiting for the engineer do not count.

mod common;

use common::*;
use fallow::config::Autonomy;
use fallow::run::engine::Request;
use fallow::run::events::{Event, Limits, Logged, Mode, SteeringKind};
use fallow::run::state::RunState;
use fallow::run::time::rfc3339;
use serde_json::Value;

fn started(v: &Value, kind: &str, part: &str) -> usize {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part && !s["started"].is_null())
        .count()
}

fn limits(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    id: u64,
    usd: Option<f64>,
    once: bool,
) {
    ask(
        tx,
        Request::Limits {
            id,
            usd,
            minutes: None,
            repair_cap: None,
            once,
        },
    )
    .unwrap();
}

#[test]
fn the_suggested_budget_pauses_the_loop_and_the_engineer_lets_it_go_on() {
    let root = scratch("budget");
    let cost = "cost = 0.8";
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], cost),
        handover_step(1, cost),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], cost),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            cost,
        ),
        handover_step(2, cost),
        check_step("US1", true, cost),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            cost,
        ),
        handover_step(3, cost),
        check_step("US2", true, cost),
    ]
    .concat();
    // No budget given at the lock: the suggestion, 4 tasks at $0.50 and 10 minutes each.
    let (e, _repo, id) = locked_calc_with(&root, &steps, None, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the pause", |v| v["stage"] == "paused");
    assert_eq!(v["feature_loop"]["budget_usd"], 2.0);
    assert_eq!(v["feature_loop"]["budget_minutes"], 40);
    assert_eq!(v["limits"]["usd"], 2.0);
    // Three steps at $0.80 went over $2.00: the pause comes before the fourth.
    assert!(
        v["limit_reached"].as_str().unwrap().starts_with("cost"),
        "{}",
        v["limit_reached"]
    );
    let options: Vec<&str> = v["needs"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_str().unwrap())
        .collect();
    assert!(
        options.contains(&"continue for one step") && options.contains(&"raise the limit"),
        "{options:?}"
    );
    assert_eq!(started(&v, "build", "T002"), 1);
    assert_eq!(
        started(&v, "build", "T003"),
        0,
        "no step starts past the limit"
    );

    // Continue for one step: T003 runs, then the loop pauses again.
    limits(tx, id, None, true);
    let v = wait_for(tx, id, "the second pause", |v| {
        v["stage"] == "paused" && started(v, "build", "T003") == 1
    });
    assert_eq!(started(&v, "handover", "phase-2"), 0);
    assert_eq!(v["limits"]["usd"], 2.0, "the limit itself is unchanged");

    // Raise the limit: the loop goes on to the end.
    limits(tx, id, Some(10.0), false);
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    assert!((v["cost_usd"].as_f64().unwrap() - 7.2).abs() < 1e-6);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

fn created() -> Event {
    Event::RunCreated {
        repo: "C:/r".into(),
        target: "main".into(),
        start_commit: "a".repeat(40),
        task: "t".into(),
        mode: Mode::Solo,
        autonomy: Autonomy::Medium,
        limits: Limits {
            usd: 10.0,
            minutes: 60,
            step_minutes: 30,
        },
        roles: Default::default(),
        repair_cap: 2,
        gates: Vec::new(),
        allow: Default::default(),
        test_patterns: Vec::new(),
        recorded: false,
        left_out: Vec::new(),
    }
}

#[test]
fn time_waiting_for_the_engineer_or_paused_does_not_count() {
    let t0 = 1_800_000_000;
    let ev = |seq: u64, s: u64, event: Event| Logged {
        seq,
        at: rfc3339(t0 + s),
        event,
    };
    let stage = |to| Event::Stage {
        to,
        why: String::new(),
    };
    use fallow::run::events::Stage;
    let mut events = vec![
        ev(1, 0, created()),
        ev(2, 0, stage(Stage::Building)),
        // 10 minutes of work, then a question to the engineer.
        ev(
            3,
            600,
            Event::NeedsEngineer {
                why: "which one?".into(),
                options: vec![],
            },
        ),
    ];
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(s.minutes_used(t0 + 3000), 10, "the wait so far is left out");
    // An hour later the engineer answers.
    events.push(ev(
        4,
        4200,
        Event::Steering {
            kind: SteeringKind::Note,
            target: None,
            detail: Some("this one".into()),
        },
    ));
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(s.minutes_used(t0 + 4800), 20);
    assert!(
        s.limit_hit(t0 + 4800).is_none(),
        "an hour of waiting is not an hour of work"
    );
    // A pause with its question counts once, not twice.
    events.push(ev(5, 4800, stage(Stage::Paused)));
    events.push(ev(
        6,
        4800,
        Event::NeedsEngineer {
            why: "limit".into(),
            options: vec![],
        },
    ));
    events.push(ev(7, 6000, stage(Stage::Building)));
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(s.minutes_used(t0 + 6600), 30);
    let (which, used) = s.limit_hit(t0 + 6600 + 1800).unwrap();
    assert_eq!(which, "time");
    assert!(used.starts_with("60 of 60"), "{used}");
}
