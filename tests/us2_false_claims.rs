//! 007 US2 (T039; CI gate for constitution I; FR-009, FR-010a, FR-012, FR-015): the loop
//! accepts nothing on an agent's word. An edit to the locked documents (undone by Fallow), a
//! wrong task id, files claimed but not changed, a missing handover and an incomplete verdict are
//! each refused, and the loop still reaches ready through repairs and fresh asks; a pipeline
//! without reviews ends as done but not verified.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

fn repair_step(task: &str, files: &[(&str, &str)]) -> String {
    task_step(task, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
}

fn refused(v: &Value, pred: impl Fn(&Value) -> bool) -> Vec<String> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| pred(s))
        .filter(|s| s["verified"][0] == false)
        .flat_map(|s| {
            s["verified"][1]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn false_claims_are_refused_and_the_loop_goes_on_through_repairs() {
    let root = scratch("claims");
    let steps = [
        // T001 also edits the locked spec.
        task_step("T001", &[("calc/__init__.py", ""), ("specs/001-calc/spec.md", "hacked\n")], ""),
        repair_step("T001", &[("calc/__init__.py", "# calc\n")]),
        // A handover that writes nothing, then a good one.
        "[[step]]\nkind = \"handover\"\npart = \"phase-1\"\noutput = { status = \"done\", path = \"specs/001-calc/handover/phase-1.md\", summary = \"x\" }\n\n".to_string(),
        handover_step(1, ""),
        // T002 reports another task.
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], "").replace("task = \"T002\"", "task = \"T009\""),
        repair_step("T002", &[("calc/__init__.py", &format!("{CALC_INIT}# repaired\n"))]),
        // T003 claims a file it did not write.
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], "")
            .replace("files = [\"tests/__init__.py\", \"tests/test_add.py\"]", "files = [\"tests/__init__.py\", \"tests/test_add.py\", \"tests/ghost.py\"]"),
        repair_step("T003", &[("tests/test_more.py", "")]),
        handover_step(2, ""),
        // An incomplete verdict (no failing case shown), then a complete one.
        check_step("US1", true, "").replace(", failing_case = \"add('a', 1) is refused\"", ""),
        check_step("US1", true, ""),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");

    let t1 = refused(&v, |s| s["task"] == "T001");
    assert!(
        t1.iter().any(
            |r| r.contains("locked documents were changed (Fallow undid it)")
                && r.contains("spec.md")
        ),
        "{t1:?}"
    );
    let spec_now = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/spec.md"],
    );
    assert!(
        spec_now.contains("User Story 1"),
        "the locked spec is intact: {spec_now}"
    );
    let t2 = refused(&v, |s| s["task"] == "T002");
    assert!(
        t2.iter().any(|r| r.contains("given T002 but reports T009")),
        "{t2:?}"
    );
    let t3 = refused(&v, |s| s["task"] == "T003");
    assert!(
        t3.iter()
            .any(|r| r.contains("claimed but not changed: tests/ghost.py")),
        "{t3:?}"
    );
    let h = refused(&v, |s| s["kind"] == "handover");
    assert!(h.iter().any(|r| r.contains("missing or empty")), "{h:?}");
    let c = refused(&v, |s| s["kind"] == "review");
    assert!(
        c.iter()
            .any(|r| r.contains("incomplete verdict") && r.contains("the case that must fail")),
        "{c:?}"
    );
    // The incomplete verdict asked the checker again, in its session; no repair of the story.
    let steps: Vec<&Value> = v["steps"].as_object().unwrap().values().collect();
    let reviews: Vec<&&Value> = steps
        .iter()
        .filter(|s| s["kind"] == "review" && s["part"] == "US1")
        .collect();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[1]["resume"], reviews[0]["session"]);
    assert!(
        !steps
            .iter()
            .any(|s| s["kind"] == "repair" && s["part"] == "US1")
    );
    // Each task needed exactly one repair round.
    for t in ["T001", "T002", "T003"] {
        assert_eq!(v["feature_loop"]["tasks"][t]["repairs"], 1, "{t}");
    }
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pipeline_without_reviews_ends_done_but_not_verified() {
    let root = scratch("solo");
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
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("solo"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "the end", |v| v["stage"] == "ready");
    assert_eq!(
        v["feature_loop"]["outcome"],
        serde_json::json!([false, ["no review at checkpoints"]])
    );
    assert!(
        v["verdicts"].as_array().unwrap().is_empty(),
        "nobody reviewed"
    );
    let d = ask(tx, Request::Dashboard).unwrap();
    let f = d["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "001-calc")
        .unwrap()
        .clone();
    assert_eq!(
        f["stage"], "done_unverified",
        "never shown as ready to merge"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_agents_own_commits_and_a_checker_that_edits_count_for_nothing() {
    let root = scratch("claims-git");
    let steps = [
        // T001 commits an edit to the locked spec itself.
        task_step(
            "T001",
            &[
                ("calc/__init__.py", ""),
                ("specs/001-calc/spec.md", "hacked\n"),
            ],
            "commit = \"sneaky\"",
        ),
        repair_step("T001", &[("calc/__init__.py", "# calc\n")]),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        // The reviewer changes a file in its worktree: its verdict does not count.
        check_step(
            "US1",
            true,
            "files = [{ path = \"calc/__init__.py\", text = \"x\" }]",
        ),
        check_step("US1", true, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let t1 = refused(&v, |s| s["task"] == "T001");
    assert!(
        t1.iter()
            .any(|r| r.contains("locked documents were changed")),
        "{t1:?}"
    );
    let spec_now = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/spec.md"],
    );
    assert!(
        spec_now.contains("User Story 1"),
        "the committed edit did not survive: {spec_now}"
    );
    let log = git(&repo, &["log", "--format=%s", "fallow/001-calc/build"]);
    assert!(
        !log.lines().any(|l| l == "sneaky"),
        "the agent's own commit is not kept: {log}"
    );
    let c = refused(&v, |s| s["kind"] == "review");
    assert!(c.iter().any(|r| r.contains("worktree changed")), "{c:?}");
    // What is merged is what was gated: only handovers and ticks follow the last gate run.
    let gated = v["gate_runs"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|g| g["kind"] == "part")
        .unwrap()["result"]["commit"]
        .as_str()
        .unwrap()
        .to_string();
    let integrated = v["integrated"].as_str().unwrap();
    let after = git(&repo, &["diff", "--name-only", &gated, integrated]);
    assert!(
        after
            .lines()
            .all(|f| f.starts_with("specs/001-calc/handover/") || f == "specs/001-calc/tasks.md"),
        "{after}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_agents_own_commit_is_undone_even_when_its_step_fails() {
    let root = scratch("claims-failed");
    let steps = [
        // T001 commits an edit to the locked spec, then reports that it failed.
        task_step(
            "T001",
            &[
                ("calc/__init__.py", ""),
                ("specs/001-calc/spec.md", "hacked\n"),
            ],
            "commit = \"sneaky\"",
        )
        .replace("status = \"done\"", "status = \"failed\""),
        // The repair changes nothing more and says it is done.
        repair_step("T001", &[("calc/__init__.py", "# calc\n")]),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", true, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let t1 = refused(&v, |s| s["task"] == "T001");
    assert!(
        t1.iter()
            .any(|r| r.contains("locked documents were changed")),
        "{t1:?}"
    );
    let spec_now = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/spec.md"],
    );
    assert!(
        spec_now.contains("User Story 1"),
        "the locked spec is intact: {spec_now}"
    );
    let log = git(&repo, &["log", "--format=%s", "fallow/001-calc/build"]);
    assert!(!log.lines().any(|l| l == "sneaky"), "{log}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
