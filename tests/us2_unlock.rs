//! 007 US2 (T042; FR-009, research S-17): unlocking halts the loop after its current step; the
//! relock preview names the affected stories; after the new lock only those are repaired and
//! reviewed again, on a new baseline, and the loop still ends in one merge.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;
use serde_json::Value;
use std::time::Duration;

fn steps_of(v: &Value, kind: &str, part: &str) -> Vec<Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .cloned()
        .collect()
}

#[test]
fn a_new_lock_reopens_only_the_affected_story() {
    let root = scratch("unlock");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", true, ""),
        // Unlocked while T004 runs.
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "sleep_ms = 6000",
        ),
        // After the new lock: US1 is brought in line, in a fresh session, and checked again.
        task_step(
            "US1",
            &[("calc/__init__.py", &format!("{CALC_INIT_SUB}# ints only\n"))],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
        check_step("US1", true, ""),
        handover_step(3, ""),
        check_step("US2", true, ""),
        LEAD_ANALYZES_AGAIN.to_string(),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "T004 running", |v| {
        steps_of(v, "build", "T004")
            .iter()
            .any(|s| !s["started"].is_null())
    });
    let project = v["feature_loop"]["project"].as_str().unwrap().to_string();
    let feature = "001-calc".to_string();

    // Unlocking: the step under way ends, and nothing starts after it.
    let u = ask(
        tx,
        Request::Unlock {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(u["loop"], id);
    let v = wait_for(tx, id, "T004's end", |v| {
        steps_of(v, "build", "T004")
            .iter()
            .all(|s| !s["end"].is_null())
    });
    let seq = v["seq"].clone();
    std::thread::sleep(Duration::from_secs(3));
    let v = ask(tx, Request::Get { id }).unwrap();
    assert_eq!(v["seq"], seq, "nothing ran while unlocked");
    assert_eq!(v["feature_loop"]["unlocked"], true);
    let f = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(f["unlocked"], true, "the spec session is open again");

    // The engineer changes User Story 1's section.
    let path = "specs/001-calc/spec.md".to_string();
    let doc = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: path.clone(),
        },
    )
    .unwrap();
    let text = doc["text"]
        .as_str()
        .unwrap()
        .replace("the result is 5.", "the result is the int 5.");
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path,
            text,
            version: doc["version"].as_str().unwrap().to_string(),
        },
    )
    .unwrap();
    let p = ask(
        tx,
        Request::RelockPreview {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(p["affected"], serde_json::json!(["US1"]), "{p}");
    assert!(
        p["why"][0].as_str().unwrap().contains("section of spec.md"),
        "{p}"
    );

    // The documents changed: analyze runs again, then the new lock.
    analyze_again(tx, &project, &feature);
    let l = ask(
        tx,
        Request::Lock {
            project: project.clone(),
            feature: feature.clone(),
            pipeline: None,
            level: None,
            budget_usd: None,
            budget_minutes: None,
        },
    )
    .unwrap();
    assert_eq!(l["loop"], id, "the same loop goes on");
    assert_eq!(l["lock"], 2);
    assert_eq!(l["affected"], serde_json::json!(["US1"]));
    let new_lock = l["commit"].as_str().unwrap().to_string();

    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let lp = &v["feature_loop"];
    assert_eq!(lp["lock"], 2);
    assert_eq!(lp["outcome"][0], true);
    // US1: one repair with the change, in a fresh session, then reviewed again.
    let repairs = steps_of(&v, "repair", "US1");
    assert_eq!(repairs.len(), 1);
    assert!(repairs[0]["resume"].is_null(), "a fresh session");
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", repairs[0]["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("the int 5"),
        "the change goes to the builder: {prompt}"
    );
    assert_eq!(steps_of(&v, "review", "US1").len(), 2, "US1 reviewed again");
    // US2 and the tasks: not repeated.
    assert_eq!(steps_of(&v, "review", "US2").len(), 1);
    for t in ["T001", "T002", "T003", "T004"] {
        assert_eq!(steps_of(&v, "build", t).len(), 1, "{t} built once");
    }
    // A new baseline, on the new lock commit.
    let baselines: Vec<&Value> = v["gate_runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["kind"] == "baseline")
        .collect();
    assert_eq!(baselines.len(), 2);
    assert_eq!(baselines[1]["result"]["commit"], new_lock.as_str());
    // The build branch has the new lock's spec, and every task still ticked.
    let spec = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/spec.md"],
    );
    assert!(spec.contains("the int 5"));
    let tasks = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/tasks.md"],
    );
    for t in ["T001", "T002", "T003", "T004"] {
        assert!(tasks.contains(&format!("- [X] {t}")), "{t} ticked: {tasks}");
    }
    ask(tx, Request::Merge { id }).unwrap();
    wait_for(tx, id, "merged", |v| v["stage"] == "done");
    assert!(git(&repo, &["show", "main:calc/__init__.py"]).contains("# ints only"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_lock_while_paused_at_ready_is_built_and_checked_before_any_merge() {
    let root = scratch("unlock-paused");
    let steps = [
        calc_loop_ok(),
        task_step(
            "US1",
            &[("calc/__init__.py", &format!("{CALC_INIT_SUB}# ints only\n"))],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
        check_step("US1", true, ""),
        LEAD_ANALYZES_AGAIN.to_string(),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let project = v["feature_loop"]["project"].as_str().unwrap().to_string();
    let feature = "001-calc".to_string();
    // Paused at ready, unlocked, changed and locked again.
    ask(tx, Request::Pause { id }).unwrap();
    wait_for(tx, id, "paused", |v| v["stage"] == "paused");
    ask(
        tx,
        Request::Unlock {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    let path = "specs/001-calc/spec.md".to_string();
    let doc = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: path.clone(),
        },
    )
    .unwrap();
    let text = doc["text"]
        .as_str()
        .unwrap()
        .replace("the result is 5.", "the result is the int 5.");
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path,
            text,
            version: doc["version"].as_str().unwrap().to_string(),
        },
    )
    .unwrap();
    analyze_again(tx, &project, &feature);
    ask(
        tx,
        Request::Lock {
            project,
            feature,
            pipeline: None,
            level: None,
            budget_usd: None,
            budget_minutes: None,
        },
    )
    .unwrap();
    // Nothing merges what the new lock has not been built and checked against.
    let why = ask(tx, Request::Merge { id }).unwrap_err();
    assert!(
        why.contains("not ready") || why.contains("not finished"),
        "{why}"
    );
    ask(tx, Request::GoOn { id }).unwrap();
    let v = wait_for(tx, id, "ready again, judged", |v| {
        v["stage"] == "ready" && v["feature_loop"]["outcome"][0] == true
    });
    assert_eq!(
        steps_of(&v, "repair", "US1").len(),
        1,
        "the change was built"
    );
    assert_eq!(steps_of(&v, "review", "US1").len(), 2, "and checked again");
    ask(tx, Request::Merge { id }).unwrap();
    wait_for(tx, id, "merged", |v| v["stage"] == "done");
    assert!(git(&repo, &["show", "main:calc/__init__.py"]).contains("# ints only"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
