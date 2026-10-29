//! 006 US1 (T041, FR-038): the cost limit pauses the run before its next step; raising the
//! limit continues it.

mod common;

use common::*;
use fallow::run::engine::Request;

#[test]
fn the_cost_limit_pauses_before_the_next_step_and_raising_it_goes_on() {
    let root = scratch("limits");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(&build_step("build", &good_files(), "cost = 10.5"));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "the pause", |v| v["stage"] == "paused");
    assert!(v["limit_reached"].as_str().unwrap().starts_with("cost"));
    assert!(
        v["needs"]["why"]
            .as_str()
            .unwrap()
            .contains("limit is reached")
    );
    let reviews = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == "review" && !s["started"].is_null())
        .count();
    assert_eq!(reviews, 0, "no step starts past the limit");
    ask(
        &tx,
        Request::Limits {
            id,
            usd: Some(20.0),
            minutes: None,
            repair_cap: None,
            once: false,
        },
    )
    .unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["limits"]["usd"], 20.0);
    assert_eq!(v["interventions"], 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn continue_once_lets_one_more_step_run_past_the_limit() {
    let root = scratch("once");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(&build_step("build", &good_files(), "cost = 10.5"));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    wait_for(&tx, id, "the pause", |v| v["stage"] == "paused");
    ask(
        &tx,
        Request::Limits {
            id,
            usd: None,
            minutes: None,
            repair_cap: None,
            once: true,
        },
    )
    .unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["limits"]["usd"], 10.0, "the limit itself is unchanged");
    assert_eq!(v["allow_once"], false, "used by the review");
    let _ = std::fs::remove_dir_all(&root);
}
