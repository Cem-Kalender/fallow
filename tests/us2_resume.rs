//! 007 US2 (T040; CI gate for constitution VII): a loop stopped mid-task, mid-gates,
//! mid-handover, mid-checkpoint and at ready resumes when Fallow starts again, in the right
//! session, without repeating a completed task, gate run or review.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use serde_json::Value;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const AGAIN: &str = "when = \"You were interrupted\"";
const SLOW: &str = "sleep_ms = 30000";
/// A test that takes a while, so Fallow can be stopped while the gates run.
const TEST_SLOW: &str = "import time\nimport unittest\n\n\nclass Slow(unittest.TestCase):\n    def test_slow(self):\n        time.sleep(6)\n";

fn steps_of(v: &Value, kind: &str, part: &str) -> Vec<Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .cloned()
        .collect()
}

/// Waits until the running step of `kind` for `part` has said something.
fn working(tx: &Sender<Msg>, id: u64, kind: &str, part: &str) {
    let v = wait_for(tx, id, kind, |v| {
        steps_of(v, kind, part)
            .iter()
            .any(|s| !s["started"].is_null() && s["end"].is_null())
    });
    let n = steps_of(&v, kind, part)
        .iter()
        .find(|s| s["end"].is_null())
        .and_then(|s| s["n"].as_u64())
        .unwrap() as u32;
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if ask(tx, Request::Activity { id, n })
            .ok()
            .and_then(|a| a.as_array().map(|a| !a.is_empty()))
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("step {n} never got to work");
}

fn restart(e: fallow::service::ToolEngine, root: &Path) -> fallow::service::ToolEngine {
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    start_studio(root, &root.join("scenario.toml"), None)
}

/// The interrupted step and its resumption in the same session.
fn resumed_in_session(v: &Value, kind: &str, part: &str) {
    let s = steps_of(v, kind, part);
    assert_eq!(
        s.len(),
        2,
        "{kind} {part}: the interrupted step and its resumption"
    );
    assert!(
        s[0]["end"]["why"]
            .as_str()
            .unwrap_or("")
            .starts_with("interrupted"),
        "{}",
        s[0]
    );
    assert_eq!(
        s[1]["resume"], s[0]["session"],
        "{kind} {part} resumed in its session"
    );
}

#[test]
fn a_loop_survives_stops_at_every_kind_of_step_and_at_ready() {
    let root = scratch("loop-resume");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], SLOW),
        task_step("T001", &[("calc/__init__.py", "")], AGAIN),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[
                ("tests/__init__.py", ""),
                ("tests/test_add.py", TEST_ADD),
                ("tests/test_slow.py", TEST_SLOW),
            ],
            "",
        ),
        handover_step(2, SLOW),
        handover_step(2, AGAIN),
        check_step("US1", true, SLOW),
        check_step("US1", true, AGAIN),
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

    // Mid-task.
    working(&e.tx, id, "build", "T001");
    let e = restart(e, &root);
    // Mid-gates: T003's gate run is under way.
    wait_for(&e.tx, id, "T003's gates", |v| {
        v["busy"] == "gates"
            && steps_of(v, "build", "T003")
                .iter()
                .any(|s| !s["end"].is_null())
    });
    let e = restart(e, &root);
    // Mid-handover.
    working(&e.tx, id, "handover", "phase-2");
    let e = restart(e, &root);
    // Mid-checkpoint.
    working(&e.tx, id, "review", "US1");
    let e = restart(e, &root);
    let v = wait_for(&e.tx, id, "ready to merge", |v| v["stage"] == "ready");
    // At ready.
    let e = restart(e, &root);
    let after = wait_for(&e.tx, id, "ready again", |v| v["stage"] == "ready");
    assert_eq!(
        after["seq"], v["seq"],
        "nothing ran again after the restart at ready"
    );

    let v = after;
    resumed_in_session(&v, "build", "T001");
    resumed_in_session(&v, "handover", "phase-2");
    resumed_in_session(&v, "review", "US1");
    // No completed task, gate run or review repeated.
    for t in ["T002", "T003", "T004"] {
        assert_eq!(steps_of(&v, "build", t).len(), 1, "{t} built once");
    }
    let gates = v["gate_runs"].as_array().unwrap();
    for t in ["T001", "T002", "T003", "T004"] {
        let runs: Vec<&Value> = gates
            .iter()
            .filter(|g| g["kind"] == "part" && g["part"] == t)
            .collect();
        assert_eq!(runs.len(), 1, "one gate run for {t}");
    }
    assert_eq!(gates.iter().filter(|g| g["kind"] == "baseline").count(), 1);
    assert_eq!(steps_of(&v, "review", "US2").len(), 1);
    assert_eq!(v["verdicts"].as_array().unwrap().len(), 2);
    assert_eq!(v["feature_loop"]["outcome"][0], true);

    // The merge still goes through after all that.
    merge_when_read(&e.tx, id).unwrap();
    wait_for(&e.tx, id, "merged", |v| v["stage"] == "done");
    assert!(git(&repo, &["show", "main:calc/__init__.py"]).contains("def sub"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
