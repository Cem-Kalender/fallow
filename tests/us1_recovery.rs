//! 006 US1 (T034, T040, FR-008): what Fallow does when a session cannot be resumed, when the
//! review gives no usable verdict, and when the repair cap is 0.

mod common;

use common::*;
use fallow::run::engine::Request;

#[test]
fn a_session_that_cannot_be_resumed_is_taken_over_by_a_new_one() {
    // The repair resumes the builder's session; the fake agent has no step for it (as when
    // Claude Code no longer knows the session), so it fails without any activity. A new session
    // gets the whole instruction, and the failed attempt costs no repair round.
    let root = scratch("lost");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(
        "[[step]]\nkind = \"build\"\noutput = { status = \"done\", commit = \"nope\", summary = \"x\" }\n",
    );
    sc.push_str(&build_step("repair", &good_files(), "when = \"Task: \""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["lost_sessions"].as_array().unwrap().len(), 1);
    assert_eq!(v["parts"]["main"]["repairs"], 1);
    let steps = v["steps"].as_object().unwrap();
    let repairs: Vec<_> = steps.values().filter(|s| s["kind"] == "repair").collect();
    assert_eq!(repairs.len(), 2);
    assert!(repairs[0]["resume"].is_string());
    assert!(
        repairs[1]["resume"].is_null(),
        "the new session starts fresh"
    );
    assert_eq!(v["interventions"], 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_engineer_can_have_the_review_tried_again() {
    let root = scratch("retry");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(&build_step("build", &good_files(), ""));
    for _ in 0..3 {
        sc.push_str("[[step]]\nkind = \"review\"\nexit = 3\n");
    }
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "the question", |v| v["needs"].is_object());
    assert!(
        v["needs"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "retry")
    );
    ask(&tx, Request::Retry { id }).unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["interventions"], 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn with_a_repair_cap_of_0_the_part_waits_for_the_engineer() {
    let root = scratch("cap0");
    let repo = repo(&root);
    std::fs::write(
        repo.join(".fallow/squad.toml"),
        "repair_cap = 0\n[[gate]]\nname = \"tests\"\ncommand = \"python -m unittest\"\nbaseline = true\n",
    )
    .unwrap();
    git(&repo, &["commit", "-q", "-am", "no repairs"]);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(
        "[[step]]\nkind = \"build\"\noutput = { status = \"done\", commit = \"nope\", summary = \"x\" }\n",
    );
    sc.push_str(&build_step("repair", &good_files(), ""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "the repair cap", |v| v["needs"].is_object());
    assert_eq!(v["parts"]["main"]["state"], "at_cap");
    assert!(
        !v["steps"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["kind"] == "repair"),
        "no repair before the engineer allows one"
    );
    ask(
        &tx,
        Request::Limits {
            id,
            usd: None,
            minutes: None,
            repair_cap: Some(1),
            once: false,
        },
    )
    .unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["parts"]["main"]["repairs"], 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failing_review_at_the_repair_cap_waits_for_the_engineer() {
    // Found in review: the run went back to building without a repair and looped on every pass.
    let root = scratch("review-cap");
    let repo = repo(&root);
    std::fs::write(
        repo.join(".fallow/squad.toml"),
        "repair_cap = 0\n[[gate]]\nname = \"tests\"\ncommand = \"python -m unittest\"\nbaseline = true\n",
    )
    .unwrap();
    git(&repo, &["commit", "-q", "-am", "no repairs"]);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(&build_step("build", &good_files(), ""));
    sc.push_str("[[step]]\nkind = \"review\"\noutput = { answers = [{ criterion = \"main/add\", ran = \"python -c add(2,3)\", result = \"6\", passed = false }, { criterion = \"main/bad\", ran = \"python -c add('a',1)\", result = \"TypeError\", passed = true, failing_case = \"add('a', 1) raises\" }] }\n");
    let fixed = format!(
        "{CALC}# checked again
"
    );
    sc.push_str(&build_step("repair", &[("calc.py", &fixed)], ""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "the repair cap", |v| v["needs"].is_object());
    assert_eq!(v["parts"]["main"]["state"], "at_cap");
    let seq = v["seq"].as_u64().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let v = ask(&tx, Request::Get { id }).unwrap();
    assert_eq!(
        v["seq"].as_u64().unwrap(),
        seq,
        "nothing happens while it waits"
    );
    assert!(v["needs"].is_object());
    ask(
        &tx,
        Request::Limits {
            id,
            usd: None,
            minutes: None,
            repair_cap: Some(1),
            once: false,
        },
    )
    .unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["parts"]["main"]["repairs"], 1);
    let _ = std::fs::remove_dir_all(&root);
}
