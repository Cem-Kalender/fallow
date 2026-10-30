//! 007 US2 (T041; FR-013): a task whose gates fail is repaired in its phase's session, and after
//! the cap the loop asks the engineer; a story whose review fails is repaired, gated, and
//! checked again.

mod common;

use common::*;
use serde_json::Value;

const ADD_WRONG: &str = "def add(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a - b\n";

fn repair_step(part: &str, files: &[(&str, &str)]) -> String {
    task_step(part, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
}

fn steps_of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect()
}

#[test]
fn failing_gates_are_repaired_in_the_session_and_the_cap_asks_the_engineer() {
    let root = scratch("repairs");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        // T002 ships a wrong add() with a test that catches it: the gates fail.
        task_step("T002", &[("calc/__init__.py", ADD_WRONG), ("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        repair_step("T002", &[("calc/__init__.py", CALC_INIT)]),
        // T003 keeps failing: its build and three repairs all leave a failing test.
        task_step("T003", &[("tests/test_more.py", "import unittest\n\n\nclass More(unittest.TestCase):\n    def test_x(self):\n        self.assertEqual(1, 2)\n")], ""),
        repair_step("T003", &[("tests/note1.txt", "1")]),
        repair_step("T003", &[("tests/note2.txt", "2")]),
        repair_step("T003", &[("tests/note3.txt", "3")]),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the engineer asked", |v| !v["needs"].is_null());
    let lp = &v["feature_loop"];
    // T002: one repair, resumed in the phase's session, then accepted.
    assert_eq!(lp["tasks"]["T002"]["state"], "accepted");
    assert_eq!(lp["tasks"]["T002"]["repairs"], 1);
    let b = steps_of(&v, "build", "T002");
    let r = steps_of(&v, "repair", "T002");
    assert_eq!(r[0]["resume"], b[0]["session"], "the repair resumed the phase's session");
    let prompt = std::fs::read_to_string(
        root.join("data/runs").join(id.to_string()).join("steps").join(format!("{}.prompt", r[0]["n"])),
    )
    .unwrap();
    assert!(prompt.contains("gate tests"), "the reasons go to the builder: {prompt}");
    // T003: three repairs, then the engineer.
    assert_eq!(steps_of(&v, "repair", "T003").len(), 3);
    assert_eq!(lp["tasks"]["T003"]["state"], "failed");
    let why = v["needs"]["why"].as_str().unwrap();
    assert!(why.contains("T003 still fails after 3 repair round(s)"), "{why}");
    assert_eq!(v["needs_engineer"].as_str().unwrap(), why);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failing_review_is_repaired_gated_and_checked_again() {
    let root = scratch("review-repair");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
        check_step("US1", false, ""),
        repair_step("US1", &[("calc/__init__.py", &format!("{CALC_INIT}# fixed\n"))]),
        check_step("US1", true, ""),
        task_step("T004", &[("calc/__init__.py", &format!("{CALC_INIT_SUB}# fixed\n")), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let reviews = steps_of(&v, "review", "US1");
    assert_eq!(reviews.len(), 2, "checked again after the repair");
    assert_eq!(reviews[0]["verified"][0], false);
    assert_eq!(reviews[1]["verified"][0], true);
    let repairs = steps_of(&v, "repair", "US1");
    assert_eq!(repairs.len(), 1);
    let phase2 = steps_of(&v, "build", "T002")[0]["session"].clone();
    assert_eq!(repairs[0]["resume"], phase2, "in the phase's session");
    let gates = v["gate_runs"].as_array().unwrap();
    assert!(gates.iter().any(|g| g["part"] == "US1"), "the story's repair was gated");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
