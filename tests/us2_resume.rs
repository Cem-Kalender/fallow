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

/// 018 US3 AS12 (T031; FR-020, research R-10): Fallow stops while three builders of a group of
/// [P] tasks run; after the restart each task resumes in its own session and working copy, no
/// task is merged twice and no accepted task is built again.
#[test]
fn a_restart_while_builders_run_resumes_each_task() {
    let root = scratch("resume-builders");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("builders.toml"), pipeline_with(3, "")).unwrap();
    let f = parallel_tasks_feature(&root, 4, &[]);
    let mut steps = f.phase_1();
    for t in &f.parallel[..3] {
        // At work when Fallow stops: each has said something, so its program surely ran.
        steps.push_str(&f.build(t, &format!("say = \"Working\"\n{SLOW}")));
        steps.push_str(&f.build(t, AGAIN));
    }
    steps.push_str(&f.build("T007", ""));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(&root, &scenario, None);
    let id = lock_calc_in(&e.tx, &f.repo, Some("builders"), Some((5.0, 60)), None);
    for t in ["T004", "T005", "T006"] {
        working(&e.tx, id, "build", t);
    }
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    // A working copy gone while Fallow was stopped is made again on its branch, with the
    // interrupted work committed there or not (research R-10).
    let wt = root.join("data/runs").join(id.to_string()).join("wt");
    std::fs::remove_dir_all(wt.join("task-T005")).unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    for t in ["T004", "T005", "T006"] {
        let mut s = steps_of(&v, "build", t);
        s.sort_by_key(|x| x["n"].as_u64());
        assert_eq!(s.len(), 2, "{t}: the interrupted build and its resumption");
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
            "{t} resumed in its session"
        );
        for s in s {
            let wt = s["worktree"].as_str().unwrap().replace('\\', "/");
            assert!(wt.ends_with(&format!("/task-{t}")), "{t}: {wt}");
        }
    }
    // T007 and T008: built once.
    for t in ["T007", "T008"] {
        assert_eq!(steps_of(&v, "build", t).len(), 1, "{t}");
    }
    let log = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("events.jsonl"),
    )
    .unwrap();
    let merged: Vec<String> = log
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e["type"] == "task_merged")
        .map(|e| e["task"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006", "T007"], "each merged once");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 AS12 (FR-020; review 1): Fallow stops after two tasks of a group were merged, while the
/// third builds; after the restart the merged tasks are neither built nor merged again, the third
/// resumes in its session and working copy, and the group ends once.
#[test]
fn a_restart_after_some_group_tasks_merged_merges_none_twice() {
    let root = scratch("resume-merged");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("builders.toml"), pipeline_with(3, "")).unwrap();
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    steps.push_str(&f.build("T004", ""));
    steps.push_str(&f.build("T005", ""));
    steps.push_str(&f.build("T006", &format!("say = \"Working\"\n{SLOW}")));
    steps.push_str(&f.build("T006", AGAIN));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(&root, &scenario, None);
    let id = lock_calc_in(&e.tx, &f.repo, Some("builders"), Some((5.0, 60)), None);
    let merged_of = |root: &Path| -> Vec<String> {
        std::fs::read_to_string(
            root.join("data/runs")
                .join(id.to_string())
                .join("events.jsonl"),
        )
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e["type"] == "task_merged")
        .map(|e| e["task"].as_str().unwrap().to_string())
        .collect()
    };
    working(&e.tx, id, "build", "T006");
    let end = Instant::now() + Duration::from_secs(60);
    while merged_of(&root) != ["T004", "T005"] && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(merged_of(&root), ["T004", "T005"], "merged before the stop");
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    for t in ["T004", "T005"] {
        assert_eq!(steps_of(&v, "build", t).len(), 1, "{t} built once");
    }
    let mut s = steps_of(&v, "build", "T006");
    s.sort_by_key(|x| x["n"].as_u64());
    assert_eq!(s.len(), 2, "the interrupted build and its resumption");
    assert_eq!(s[1]["resume"], s[0]["session"], "resumed in its session");
    assert!(
        s[1]["worktree"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/task-T006")
    );
    assert_eq!(
        merged_of(&root),
        ["T004", "T005", "T006"],
        "each merged once"
    );
    let log = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("events.jsonl"),
    )
    .unwrap();
    assert_eq!(log.matches("\"type\":\"group_started\"").count(), 1);
    assert_eq!(log.matches("\"type\":\"group_done\"").count(), 1);
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
