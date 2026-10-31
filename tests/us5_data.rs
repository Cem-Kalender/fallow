//! 007 US5 (T072; FR-003b, constitution V): the data view shows what Fallow keeps per project
//! (its loops, conversations and worktrees) with their sizes; a loop's or a conversation's data
//! can be deleted, but not a loop still running, nor the spec session of a feature not merged.

mod common;

use common::*;
use fallow::run::engine::Request;

#[test]
fn sizes_per_project_and_deleting_what_is_no_longer_needed() {
    let root = scratch("data");
    let steps = task_step("T001", &[("calc/__init__.py", "")], "sleep_ms = 30000");
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    wait_for(tx, id, "T001 running", |v| {
        v["busy"].as_str().is_some_and(|b| b.starts_with("step"))
    });

    let d = ask(tx, Request::Data).unwrap();
    let p = &d["projects"][0];
    assert_eq!(p["name"], "repo");
    let lp = &p["loops"][0];
    assert_eq!(lp["id"], id);
    assert_eq!(lp["feature"], "001-calc");
    assert_eq!(lp["running"], true);
    assert!(
        lp["bytes"].as_u64().unwrap() > 0 && lp["worktrees"].as_u64().unwrap() > 0,
        "{lp}"
    );
    let convs = p["conversations"].as_array().unwrap();
    assert_eq!(
        convs.len(),
        2,
        "the project conversation and the spec session: {convs:?}"
    );
    assert!(convs.iter().all(|c| c["bytes"].as_u64().unwrap() > 0));
    assert!(
        p["worktrees"].as_u64().unwrap() > 0,
        "the spec session's worktree"
    );
    let sum = lp["bytes"].as_u64().unwrap()
        + convs
            .iter()
            .map(|c| c["bytes"].as_u64().unwrap())
            .sum::<u64>()
        + p["worktrees"].as_u64().unwrap();
    assert_eq!(p["bytes"].as_u64().unwrap(), sum);
    assert_eq!(d["bytes"], p["bytes"]);

    // A running loop is refused; once stopped, its data goes, worktrees and all.
    let why = ask(
        tx,
        Request::DeleteData {
            kind: "loop".into(),
            id: id.to_string(),
        },
    )
    .unwrap_err();
    assert!(why.contains("still running"), "{why}");
    ask(tx, Request::Stop { id, reason: None }).unwrap();
    wait_for(tx, id, "stopped", |v| {
        v["stage"] == "stopped" && v["busy"] == ""
    });
    let dir = root.join("data/runs").join(id.to_string());
    assert!(dir.exists());
    ask(
        tx,
        Request::DeleteData {
            kind: "loop".into(),
            id: id.to_string(),
        },
    )
    .unwrap();
    assert!(!dir.exists());
    let d = ask(tx, Request::Data).unwrap();
    assert!(d["projects"][0]["loops"].as_array().unwrap().is_empty());
    assert!(ask(tx, Request::Get { id }).is_err(), "the loop is gone");
    let wts = git(&repo, &["worktree", "list"]);
    assert!(
        !wts.contains(&format!("runs/{id}/")) && !wts.contains(&format!("runs\\{id}\\")),
        "its worktrees are gone from git too: {wts}"
    );

    // The spec session of a feature not merged is what the lead answers from: refused. The
    // project conversation can go.
    let spec = convs.iter().find(|c| c["kind"] == "spec").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let project = convs.iter().find(|c| c["kind"] != "spec").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let why = ask(
        tx,
        Request::DeleteData {
            kind: "conv".into(),
            id: spec.clone(),
        },
    )
    .unwrap_err();
    assert!(why.contains("not merged"), "{why}");
    ask(
        tx,
        Request::DeleteData {
            kind: "conv".into(),
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(!root.join("data/conversations").join(&project).exists());
    let wts = git(&repo, &["worktree", "list"]);
    assert!(
        !wts.contains("_project"),
        "the conversation's worktree went with it: {wts}"
    );
    let d = ask(tx, Request::Data).unwrap();
    let left: Vec<&str> = d["projects"][0]["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(left, vec![spec.as_str()]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
