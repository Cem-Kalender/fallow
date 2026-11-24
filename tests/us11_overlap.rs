//! 011 US5 (quickstart Q6; FR-013 to FR-016c, SC-006): overlaps between unfinished loops of a
//! project are named at the lock and while running, with "Run after" and "Wait for". No network,
//! no model: the fake runtime plays every step.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use fallow::studio::Req;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

fn loop_view(tx: &Sender<Msg>, id: u64) -> Value {
    ask(tx, Request::Loop { id }).unwrap()
}

fn wait_view(tx: &Sender<Msg>, id: u64, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let v = loop_view(tx, id);
        if pred(&v) {
            return v;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {what}: stage {}, overlaps {}, waits_for {}, offer {}",
            v["stage"],
            v["overlaps"],
            v["waits_for"],
            v["offer"]
        );
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// A project with both features worked out (001-calc, 002-calc2); `squad` added to its squad
/// file first; `steps` the scenario's loop steps.
fn project(root: &Path, steps: &str, squad: &str) -> (fallow::service::ToolEngine, PathBuf) {
    let repo = repo(root);
    if !squad.is_empty() {
        let path = repo.join(".fallow").join("squad.toml");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str(squad);
        std::fs::write(&path, text).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "squad"]);
    }
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}{steps}", lead_writes_calc(), lead_writes_calc2()),
    )
    .unwrap();
    let e = start_studio(root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    prepare_feature_in(tx, "repo", "Calc", "001-calc");
    prepare_feature_in(tx, "repo", "Calc2", "002-calc2");
    (e, repo)
}

fn lock(tx: &Sender<Msg>, feature: &str, run_after: Option<u64>) -> u64 {
    lock_prepared(
        tx,
        "repo",
        feature,
        None,
        Some((5.0, 60)),
        None,
        LockExtra {
            update_limit: None,
            run_after,
        },
    )
}

/// 001-calc's first task writes calc/__init__.py, then its handover waits a while (running).
fn calc_running() -> String {
    let slow = handover_step(1, "sleep_ms = 30000");
    [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        slow,
        calc_loop_ok(),
    ]
    .concat()
}

/// FR-013 (Q6): with 001-calc running and having changed calc/__init__.py, the overlap read for
/// 002-calc2 (whose tasks name that file) names the loop and the file; switched off, nothing.
#[test]
fn the_overlap_read_names_the_loop_and_the_file() {
    let root = scratch("o-read");
    let (e, _repo) = project(&root, &calc_running(), "");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    wait_for(tx, a, "calc/__init__.py touched", |v| {
        v["feature_loop"]["touched"]
            .to_string()
            .contains("calc/__init__.py")
    });
    let o = ask(
        tx,
        Request::Overlap {
            project: "repo".into(),
            feature: "002-calc2".into(),
        },
    )
    .unwrap();
    let list = o["overlaps"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{o}");
    assert_eq!(list[0]["run"], a);
    assert_eq!(list[0]["feature"], "001-calc");
    assert!(
        list[0]["files"].to_string().contains("calc/__init__.py"),
        "{o}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    let root = scratch("o-off");
    let (e, _repo) = project(&root, &calc_running(), "[updates]\noverlap = false\n");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    wait_for(tx, a, "calc/__init__.py touched", |v| {
        v["feature_loop"]["touched"]
            .to_string()
            .contains("calc/__init__.py")
    });
    let o = ask(
        tx,
        Request::Overlap {
            project: "repo".into(),
            feature: "002-calc2".into(),
        },
    )
    .unwrap();
    assert_eq!(o["overlaps"], serde_json::json!([]), "{o}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-014, SC-006: locked to run after 001-calc, 002-calc2 starts no step, shown waiting; it
/// starts by itself within 10 s of 001-calc being merged, from the target as it is then.
#[test]
fn run_after_waits_and_starts_when_the_other_is_merged() {
    let root = scratch("o-after");
    let steps = format!("{}{}", calc_loop_ok(), calc2_steps());
    let (e, repo) = project(&root, &steps, "");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    let b = lock(tx, "002-calc2", Some(a));
    let v = wait_view(tx, b, "waiting", |v| !v["waits_for"].is_null());
    assert_eq!(v["waits_for"]["feature"], "001-calc");
    std::thread::sleep(Duration::from_secs(3));
    let s = wait_for(tx, b, "no step", |_| true);
    assert!(
        s["steps"].as_object().is_none_or(|m| m.is_empty()),
        "no step while waiting: {}",
        s["steps"]
    );
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "001-calc merged", |v| v["stage"] == "done");
    let merged_at = Instant::now();
    let main = git(&repo, &["rev-parse", "main"]);
    wait_for(tx, b, "002-calc2 started", |v| {
        v["steps"].as_object().is_some_and(|m| !m.is_empty())
    });
    assert!(
        merged_at.elapsed() < Duration::from_secs(10),
        "SC-006: started {:?} after the merge",
        merged_at.elapsed()
    );
    let branch = git(&repo, &["rev-parse", "fallow/002-calc2/build"]);
    assert!(
        git(&repo, &["merge-base", "--is-ancestor", &main, &branch]).is_empty(),
        "started from the target as it is now"
    );
    let v = loop_view(tx, b);
    assert!(v["waits_for"].is_null());
    // Started from the target: its base moved, no update left in progress (review F1).
    assert!(v["update"].is_null(), "{}", v["update"]);
    assert_eq!(v["updates"], 0);
    let s = wait_for(tx, b, "b", |_| true);
    assert_eq!(
        s["feature_loop"]["base_commit"], main,
        "its base is the target it started from"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 002-calc2's steps, writing calc/__init__.py as well.
fn calc2_steps() -> String {
    [
        task_step("T001", &[("calc/__init__.py", CALC_INIT_SUB)], ""),
        task_step("T002", &[("tests/__init__.py", "")], ""),
        handover_step(1, "").replace("specs/001-calc/", "specs/002-calc2/"),
        check_step("US1", true, ""),
    ]
    .concat()
}

/// FR-015: two loops running change the same file: both say so and offer "Wait for"; "Wait
/// for" holds new steps until the other is stopped; a cycle is refused naming the loop; a restart
/// keeps the wait; "Stop waiting" lets it go on.
#[test]
fn running_loops_that_overlap_say_so_and_may_wait() {
    let root = scratch("o-run");
    // Both first tasks write calc/__init__.py; the second tasks wait a while (both running).
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, "sleep_ms = 20000"),
        handover_step(1, "sleep_ms = 20000").replace("specs/001-calc/", "specs/002-calc2/"),
    ]
    .concat();
    let (e, _repo) = project(&root, &steps, "");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    let b = lock(tx, "002-calc2", None);
    let va = wait_view(tx, a, "the overlap on a", |v| {
        v["overlaps"].as_array().is_some_and(|o| !o.is_empty())
    });
    let vb = wait_view(tx, b, "the overlap on b", |v| {
        v["overlaps"].as_array().is_some_and(|o| !o.is_empty())
    });
    assert_eq!(va["overlaps"][0]["run"], b);
    assert!(
        va["overlaps"][0]["files"]
            .to_string()
            .contains("calc/__init__.py")
    );
    assert_eq!(vb["overlaps"][0]["feature"], "001-calc");
    assert!(
        vb["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "wait_for"),
        "{}",
        vb["offer"]
    );
    ask(tx, Request::Wait { id: b, other: a }).unwrap();
    let why = ask(tx, Request::Wait { id: a, other: b }).unwrap_err();
    assert!(why.contains("002-calc2") && why.contains("cycle"), "{why}");
    let v = loop_view(tx, b);
    assert_eq!(v["waits_for"]["feature"], "001-calc");
    assert!(
        v["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "stop_waiting")
    );
    // A restart keeps the wait.
    let fallow::service::ToolEngine { tx: t, handle, .. } = e;
    t.send(Msg::Shutdown).unwrap();
    handle.join().unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let tx = &e.tx;
    let v = wait_view(tx, b, "still waiting after a restart", |v| {
        !v["waits_for"].is_null()
    });
    assert_eq!(v["waits_for"]["run"], a);
    ask(tx, Request::StopWaiting { id: b }).unwrap();
    assert!(loop_view(tx, b)["waits_for"].is_null());
    // Waiting again, then the other loop stopped: the wait ends by itself.
    ask(tx, Request::Wait { id: b, other: a }).unwrap();
    ask(
        tx,
        Request::Stop {
            id: a,
            reason: None,
        },
    )
    .unwrap();
    wait_view(tx, b, "the wait ended", |v| v["waits_for"].is_null());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

// 014 (T005): the HTTP transport is in the `automation` build only on Windows.
#[cfg(any(not(any(windows, target_os = "macos")), feature = "automation"))]
/// T034's page check: not a test. `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test
/// us11_overlap serve_the_011_overlap_demo -- --ignored --nocapture` serves a project where
/// 001-calc runs (slowly) having changed calc/__init__.py, and 002-calc2, whose tasks name that
/// file, is ready to lock; locked, its first task changes the same file. The address goes to
/// `<root>/url.txt`; it serves for `FALLOW_VISUAL_SECS` seconds (default 1800).
#[test]
#[ignore]
fn serve_the_011_overlap_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    // Each loop's first task changes calc/__init__.py; the next steps take their time.
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, "sleep_ms = 1800000"),
        task_step("T001", &[("calc/__init__.py", CALC_INIT_SUB)], ""),
        task_step("T002", &[("tests/__init__.py", "")], "sleep_ms = 1800000"),
    ]
    .concat();
    let (e, _repo) = project(&root, &steps, "");
    let a = lock(&e.tx, "001-calc", None);
    wait_for(&e.tx, a, "calc/__init__.py touched", |v| {
        v["feature_loop"]["touched"]
            .to_string()
            .contains("calc/__init__.py")
    });
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(Duration::from_secs(secs));
    drop(e);
}

/// FR-014 with FR-005 (phase 7 review 1, 3): the target changed this feature's documents while
/// it waited: it asks, and "Lock again" is taken; a loop stopped while waiting is left as it is.
#[test]
fn a_wait_that_ends_on_changed_documents_offers_lock_again() {
    let root = scratch("o-docs");
    let (e, repo) = project(&root, &calc_loop_ok(), "");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    let b = lock(tx, "002-calc2", Some(a));
    wait_view(tx, b, "waiting", |v| !v["waits_for"].is_null());
    commit_on(
        &repo,
        "main",
        "specs/002-calc2/spec.md",
        "# changed on main\n",
        "edit the spec",
    );
    // The other loop stopped: the wait ends, and the target moved with the documents' change.
    ask(
        tx,
        Request::Stop {
            id: a,
            reason: None,
        },
    )
    .unwrap();
    let v = wait_for(tx, b, "the documents' question", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("documents"))
    });
    assert!(
        v["steps"].as_object().is_none_or(|m| m.is_empty()),
        "nothing built"
    );
    let la = ask(tx, Request::LockAgain { id: b }).unwrap();
    assert!(
        la["message"]
            .as_str()
            .unwrap()
            .contains("specs/002-calc2/spec.md"),
        "{la}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// Phase 7 review 3: a loop stopped while it waits is not changed when the other is merged.
#[test]
fn a_loop_stopped_while_waiting_is_left_as_it_is() {
    let root = scratch("o-stopped");
    let (e, repo) = project(&root, &calc_loop_ok(), "");
    let tx = &e.tx;
    let a = lock(tx, "001-calc", None);
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    let b = lock(tx, "002-calc2", Some(a));
    wait_view(tx, b, "waiting", |v| !v["waits_for"].is_null());
    ask(
        tx,
        Request::Stop {
            id: b,
            reason: None,
        },
    )
    .unwrap();
    wait_for(tx, b, "stopped", |v| v["stage"] == "stopped");
    let branch = git(&repo, &["rev-parse", "fallow/002-calc2/build"]);
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "001-calc merged", |v| v["stage"] == "done");
    std::thread::sleep(Duration::from_secs(3));
    let v = wait_for(tx, b, "still stopped", |_| true);
    assert!(v["needs_engineer"].is_null(), "{}", v["needs_engineer"]);
    assert_eq!(git(&repo, &["rev-parse", "fallow/002-calc2/build"]), branch);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
