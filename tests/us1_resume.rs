//! 006 US1 (T040, constitution VII, SC-003): a run interrupted mid-step, and at "ready", resumes
//! when Fallow starts again, without repeating a completed step and with its history intact.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use serde_json::Value;

fn steps_of(v: &Value, kind: &str) -> Vec<Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind)
        .cloned()
        .collect()
}

fn running(v: &Value, kind: &str) -> bool {
    steps_of(v, kind)
        .iter()
        .any(|s| !s["started"].is_null() && s["end"].is_null())
}

/// Waits until the running step of `kind` has said something: its agent is really at work.
fn working(tx: &std::sync::mpsc::Sender<Msg>, id: u64, kind: &str) {
    let v = wait_for(tx, id, kind, |v| running(v, kind));
    let n = steps_of(&v, kind)
        .iter()
        .find(|s| s["end"].is_null())
        .and_then(|s| s["n"].as_u64())
        .unwrap() as u32;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < end {
        if ask(tx, Request::Activity { id, n })
            .ok()
            .and_then(|a| a.as_array().map(|a| !a.is_empty()))
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("step {n} never got to work");
}

#[test]
fn a_run_survives_stops_mid_build_mid_review_and_at_ready() {
    let root = scratch("resume");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    // Stopped while it works: nothing written yet.
    sc.push_str("[[step]]\nkind = \"build\"\nsleep_ms = 20000\noutput = { status = \"done\", commit = \"$HEAD\", summary = \"too late\" }\n");
    sc.push_str(&build_step(
        "build",
        &good_files(),
        "when = \"You were interrupted\"",
    ));
    sc.push_str(&review_ok("sleep_ms = 20000"));
    sc.push_str(&review_ok("when = \"You were interrupted\""));
    std::fs::write(&scenario, &sc).unwrap();

    // Fallow 1: plan, approve, stop during the build.
    let (tx, h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    working(&tx, id, "build");
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();

    // Fallow 2: the build resumes in its session; stop during the review.
    let (tx, h) = start_engine(&root, &scenario);
    working(&tx, id, "review");
    let v = ask(&tx, Request::Get { id }).unwrap();
    let builds = steps_of(&v, "build");
    assert_eq!(builds.len(), 2, "the interrupted build and its resumption");
    assert!(
        builds
            .iter()
            .any(|b| b["reason"] == "resumed after Fallow stopped")
    );
    let resumed = builds
        .iter()
        .find(|b| b["reason"] == "resumed after Fallow stopped")
        .unwrap();
    let first = builds
        .iter()
        .find(|b| b["reason"] != "resumed after Fallow stopped")
        .unwrap();
    assert_eq!(
        resumed["session"], first["session"],
        "resumed in the same session"
    );
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();

    // Fallow 3: the review resumes; stop at ready.
    let (tx, h) = start_engine(&root, &scenario);
    wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();

    // Fallow 4: still ready; the merge goes through.
    let (tx, h) = start_engine(&root, &scenario);
    let v = wait_for(&tx, id, "ready after the restart", |v| {
        v["stage"] == "ready"
    });
    assert_eq!(
        steps_of(&v, "plan_turn").len(),
        1,
        "planning never ran again"
    );
    let baselines = v["gate_runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["kind"] == "baseline")
        .count();
    assert_eq!(baselines, 1, "the baseline never ran again");
    ask(&tx, Request::Merge { id }).unwrap();
    let v = wait_for(&tx, id, "done", |v| v["stage"] == "done");
    assert_eq!(v["steering"].as_array().unwrap().len(), 2);
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_run_survives_stops_mid_planning_and_mid_gates() {
    let root = scratch("resume-plan");
    let repo = repo(&root);
    // A gate that is slow while the marker exists, so Fallow can be stopped while it runs.
    let marker = root.join("slow");
    std::fs::write(
        repo.join("gate.py"),
        format!(
            "import os, subprocess, sys, time\nif os.path.exists({:?}):\n    time.sleep(60)\nsys.exit(subprocess.call([sys.executable, '-m', 'unittest']))\n",
            marker.display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        repo.join(".fallow/squad.toml"),
        "repair_cap = 2\n[[gate]]\nname = \"tests\"\ncommand = \"python gate.py\"\nbaseline = true\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "slow gate"]);
    let scenario = root.join("scenario.toml");
    let mut sc = String::from(
        "[[step]]\nkind = \"plan_turn\"\nsleep_ms = 20000\noutput = { reply = \"late\", ready = false }\n",
    );
    sc.push_str(&plan_step(PLAN).replace(
        "kind = \"plan_turn\"\n",
        "kind = \"plan_turn\"\nwhen = \"You were interrupted\"\n",
    ));
    sc.push_str(&build_step("build", &good_files(), ""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();

    // Fallow 1: stop during the first planning turn.
    let (tx, h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    working(&tx, id, "plan_turn");
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();

    // Fallow 2: the planning turn resumes in its session; stop while the baseline runs.
    let (tx, h) = start_engine(&root, &scenario);
    let v = wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    let turns = steps_of(&v, "plan_turn");
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0]["session"], turns[1]["session"]);
    std::fs::write(&marker, "").unwrap();
    ask(&tx, Request::Approve { id }).unwrap();
    wait_for(&tx, id, "the gates", |v| v["busy"] == "gates");
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();
    std::fs::remove_file(&marker).unwrap();

    // Fallow 3: the stopped gate left no result and runs again; the run goes on.
    let (tx, h) = start_engine(&root, &scenario);
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    let baselines: Vec<&Value> = v["gate_runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["kind"] == "baseline")
        .collect();
    assert_eq!(baselines.len(), 1, "only the finished baseline is recorded");
    assert_eq!(steps_of(&v, "plan_turn").len(), 2);
    tx.send(Msg::Shutdown).unwrap();
    h.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
