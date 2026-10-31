//! 007 US3 (T055; SC-004): a finished task's whole story from one request: the agent and its
//! steps, the diff of its commit, the commands it ran with their results, the commit, and its
//! gate result. After Fallow starts again, a page gets the whole history.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};

#[test]
fn a_tasks_story_is_one_request_away_even_after_a_restart() {
    let root = scratch("task-detail");
    let steps = calc_loop_ok().replacen(
        "part = \"T003\"\n",
        "part = \"T003\"\nrun = [{ command = \"python -m unittest\", output = \"Ran 2 tests\\nOK\" }]\n",
        1,
    );
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    wait_for(&e.tx, id, "ready to merge", |v| v["stage"] == "ready");
    let before = ask(
        &e.tx,
        Request::Task {
            id,
            task: "T003".into(),
        },
    )
    .unwrap();

    // Fallow starts again: the same story.
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let d = ask(
        &e.tx,
        Request::Task {
            id,
            task: "T003".into(),
        },
    )
    .unwrap();
    assert_eq!(d, before, "a reopened page gets the whole history");

    assert_eq!(d["state"], "accepted");
    let steps = d["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["agent"], "builder");
    assert_eq!(steps[0]["verified"][0], true);
    let commit = d["commit"].as_str().unwrap();
    assert_eq!(steps[0]["commit"], commit);
    let diff = d["diff"].as_str().unwrap();
    assert!(
        diff.contains("tests/test_add.py") && diff.contains("+class Add"),
        "{diff}"
    );
    assert!(
        diff.contains("- [X] T003") || diff.contains("+- [X] T003"),
        "the tick is in the task's commit: {diff}"
    );
    let cmds = d["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0]["text"], "python -m unittest");
    assert_eq!(cmds[0]["result"], "ok");
    let gates = d["gates"].as_array().unwrap();
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0]["result"]["commit"], commit);
    assert_eq!(gates[0]["class"], "pass");
    // A task that does not exist is refused.
    assert!(
        ask(
            &e.tx,
            Request::Task {
                id,
                task: "T999".into()
            }
        )
        .is_err()
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
