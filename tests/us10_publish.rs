//! 010 US2 and US3 (quickstart Q2 to Q5; FR-005 to FR-021): a finished loop becomes a pull
//! request (the only way with a protected target), on its own when verified if chosen at the lock,
//! and is followed to its merge on the remote. Never a push to the target. No network: the
//! remote is a bare repository behind a github.com address, `gh` is `fallow-fake-host`.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use fallow::run::events::Level;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const ADDRESS: &str = "https://github.com/acme/calc.git";
const BRANCH: &str = "fallow/001-calc/build";

/// Waits until the loop's view satisfies `pred` (reading the loop also reads its pull request).
fn wait_loop(tx: &Sender<Msg>, id: u64, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(120);
    let mut last = Value::Null;
    while Instant::now() < end {
        if let Ok(v) = ask(tx, Request::Loop { id }) {
            if pred(&v) {
                return v;
            }
            last = v;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("timed out waiting for {what}: stage {}, publish {}", last["stage"], last["publish"]);
}

/// The remote's protection is read before anything is offered.
fn settled(tx: &Sender<Msg>, id: u64) -> Value {
    wait_loop(tx, id, "the remote read", |v| {
        v["publish"]["remote"]["checking"] != true && v["publish"]["remote"]["state"] != "checking"
    })
}

fn hosted(state: Value) -> impl FnOnce(&Path, &Path) {
    move |root: &Path, repo: &Path| {
        bare_remote(root, repo, ADDRESS);
        fake_host(root, state);
    }
}

fn ready(tx: &Sender<Msg>, id: u64) -> Value {
    wait_for(tx, id, "ready", |v| v["stage"] == "ready")
}

fn pr_creates(root: &Path) -> Vec<Vec<String>> {
    fake_host_log(&root.join("host.json"))
        .into_iter()
        .filter(|a| a.len() > 1 && a[0] == "pr" && a[1] == "create")
        .collect()
}

fn arg<'a>(line: &'a [String], name: &str) -> Option<&'a str> {
    line.iter().position(|a| a == name).and_then(|i| line.get(i + 1)).map(String::as_str)
}

#[test]
fn a_protected_target_takes_a_pull_request_of_exactly_the_verified_commit() {
    let root = scratch("p-protected");
    let (e, repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        hosted(json!({ "login": "octo", "rules": { "main": ["pull_request"] }, "protected": { "main": false } })),
    );
    let tx = &e.tx;
    let v = ready(tx, id);
    let commit = v["integrated"].as_str().unwrap().to_string();
    let main_before = remote_head(&root, "main");
    let l = settled(tx, id);
    assert_eq!(l["publish"]["offer"], json!(["pull_request"]), "{}", l["publish"]);
    assert!(l["publish"]["why"].as_str().unwrap().contains("protected"), "{}", l["publish"]);
    assert_eq!(l["publish"]["account"], "octo");
    assert_eq!(l["publish"]["when_verified"], "wait", "Normal waits: {}", l["publish"]);
    // No local merge onto a protected target.
    let e2 = ask(tx, Request::Merge { id }).unwrap_err();
    assert!(e2.contains("protected"), "{e2}");
    ask(tx, Request::Publish { id }).unwrap();
    let l = wait_loop(tx, id, "publishing", |v| v["stage"] == "publishing");
    let pb = &l["publish"]["publication"];
    assert_eq!(pb["branch"], BRANCH, "{pb}");
    assert_eq!(pb["commit"], commit.as_str());
    assert_eq!(pb["by"], "engineer");
    assert_eq!(pb["pr_url"], "https://github.com/acme/calc/pull/1");
    // Exactly the verified commit on the loop's branch; the target untouched.
    assert_eq!(remote_head(&root, BRANCH).as_deref(), Some(commit.as_str()));
    assert_eq!(remote_head(&root, "main"), main_before, "the target never moves");
    let creates = pr_creates(&root);
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert_eq!(arg(&creates[0], "--base"), Some("main"));
    assert_eq!(arg(&creates[0], "--head"), Some(BRANCH));
    let body = host_state(&root.join("host.json"))["prs"][0]["body"].as_str().unwrap().to_string();
    for want in ["US1", "Gates", "Verified by Fallow. The merge is the engineer's."] {
        assert!(body.contains(want), "{want} in the body:\n{body}");
    }
    // Never a token, a merge or an approval asked of the tool.
    for line in fake_host_log(&root.join("host.json")) {
        for bad in ["token", "--show-token", "merge", "review", "--approve", "close", "edit"] {
            assert!(!line.iter().any(|a| a == bad), "{line:?}");
        }
    }
    // A second publication while it is open is refused, showing the open one.
    let e3 = ask(tx, Request::Publish { id }).unwrap_err();
    assert!(e3.contains("open"), "{e3}");
    assert_eq!(l["publish"]["offer"], json!([]), "nothing more to offer while open");
    // The open pull request is something the engineer has to do.
    let d = ask(tx, Request::Dashboard).unwrap();
    assert!(d["needs_you"].to_string().contains("pr_open"), "{}", d["needs_you"]);
    let _ = repo;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_open_target_offers_both_and_the_local_merge_pushes_nothing() {
    let root = scratch("p-open");
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        hosted(json!({ "login": "octo", "protected": { "main": false } })),
    );
    let tx = &e.tx;
    ready(tx, id);
    let l = settled(tx, id);
    assert_eq!(l["publish"]["offer"], json!(["pull_request", "local_merge"]), "{}", l["publish"]);
    let main_before = remote_head(&root, "main");
    ask(tx, Request::Merge { id }).unwrap();
    wait_for(tx, id, "merged", |v| v["stage"] == "done");
    assert_eq!(remote_head(&root, "main"), main_before, "a local merge pushes nothing");
    assert!(pr_creates(&root).is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_target_not_on_the_remote_offers_only_the_local_merge() {
    let root = scratch("p-missing");
    // The fake knows no `main` (a 404): the target was never pushed there.
    let (e, _repo, id) = locked_calc_prepared(&root, &calc_loop_ok(), None, None, hosted(json!({ "login": "octo" })));
    let tx = &e.tx;
    ready(tx, id);
    let l = settled(tx, id);
    assert_eq!(l["publish"]["offer"], json!(["local_merge"]), "{}", l["publish"]);
    assert!(l["publish"]["why"].as_str().unwrap().contains("main is not on acme/calc yet"), "{}", l["publish"]);
    assert!(ask(tx, Request::Publish { id }).unwrap_err().contains("no pull request here"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_taken_branch_is_never_overwritten_and_a_failed_pull_request_can_be_tried_again() {
    let root = scratch("p-taken");
    let (e, repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        |root: &Path, repo: &Path| {
            bare_remote(root, repo, ADDRESS);
            // Someone's branch of the same name, at another commit.
            git(repo, &["push", "-q", "origin", &format!("HEAD:refs/heads/{BRANCH}")]);
            fake_host(
                root,
                json!({ "login": "octo", "protected": { "main": true }, "fail": { "pr create": "GraphQL: something went wrong (createPullRequest)" } }),
            );
        },
    );
    let tx = &e.tx;
    let theirs = remote_head(&root, BRANCH).unwrap();
    let v = ready(tx, id);
    let commit = v["integrated"].as_str().unwrap().to_string();
    settled(tx, id);
    ask(tx, Request::Publish { id }).unwrap();
    let l = wait_loop(tx, id, "the failure", |v| !v["publish"]["failed"].is_null());
    let f = &l["publish"]["failed"];
    assert_eq!(f["step"], "pull_request", "{f}");
    assert!(f["why"].as_str().unwrap().contains("something went wrong"), "{f}");
    assert_eq!(f["pushed"], json!([format!("{BRANCH}-2"), commit]), "{f}");
    assert_eq!(l["stage"], "ready");
    assert_eq!(remote_head(&root, BRANCH).as_deref(), Some(theirs.as_str()), "theirs untouched");
    assert_eq!(remote_head(&root, &format!("{BRANCH}-2")).as_deref(), Some(commit.as_str()));
    // Tried again once the remote works: one pull request, from the branch already pushed.
    let state = root.join("host.json");
    let mut s = host_state(&state);
    s["fail"] = json!({});
    set_host_state(&state, &s);
    ask(tx, Request::Publish { id }).unwrap();
    let l = wait_loop(tx, id, "publishing", |v| v["stage"] == "publishing");
    assert_eq!(l["publish"]["publication"]["branch"], format!("{BRANCH}-2"));
    assert_eq!(host_state(&state)["prs"].as_array().unwrap().len(), 1);
    assert!(l["publish"]["failed"].is_null(), "{}", l["publish"]);
    let _ = repo;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_loop_locked_free_publishes_itself_only_when_verified_and_readable() {
    // Free, verified, protection read: published without a request.
    let root = scratch("p-free");
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        Some(Level::Free),
        hosted(json!({ "login": "octo", "protected": { "main": true } })),
    );
    let tx = &e.tx;
    let l = wait_loop(tx, id, "published on its own", |v| v["stage"] == "publishing");
    assert_eq!(l["publish"]["when_verified"], "pull_request");
    assert_eq!(l["publish"]["publication"]["by"], "when_verified");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // Free, but the loop ends not verified (a pipeline without reviews): it waits.
    let root = scratch("p-free-unverified");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &steps,
        Some("solo"),
        Some(Level::Free),
        hosted(json!({ "login": "octo", "protected": { "main": true } })),
    );
    let tx = &e.tx;
    ready(tx, id);
    let l = wait_loop(tx, id, "why it waits", |v| v["publish"]["waits"].is_string());
    assert!(l["publish"]["waits"].as_str().unwrap().contains("not verified"), "said at once: {}", l["publish"]);
    std::thread::sleep(Duration::from_secs(2));
    assert!(pr_creates(&root).is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // Free, verified, but not signed in: the protection cannot be read, so it waits.
    let root = scratch("p-free-signed-out");
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        Some(Level::Free),
        hosted(json!({ "signed_in": false, "protected": { "main": true } })),
    );
    let tx = &e.tx;
    ready(tx, id);
    let l = wait_loop(tx, id, "why it waits", |v| {
        v["publish"]["waits"].as_str().is_some_and(|w| w.contains("could not be read"))
    });
    assert!(l["publish"]["publication"].is_null());
    assert!(pr_creates(&root).is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_publication_cut_short_by_a_restart_never_opens_a_second_pull_request() {
    // Fallow stops while `gh pr create` runs; the pull request is opened all the same.
    let root = scratch("p-restart");
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        hosted(json!({ "login": "octo", "protected": { "main": true }, "sleep_ms": { "pr create": 3000 } })),
    );
    let tx = &e.tx;
    ready(tx, id);
    settled(tx, id);
    ask(tx, Request::Publish { id }).unwrap();
    let state = root.join("host.json");
    let end = Instant::now() + Duration::from_secs(60);
    while pr_creates(&root).is_empty() {
        assert!(Instant::now() < end, "pr create never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let fallow::service::ToolEngine { tx: t, handle, .. } = e;
    t.send(Msg::Shutdown).unwrap();
    handle.join().unwrap();
    while host_state(&state)["prs"].as_array().is_none_or(|a| a.is_empty()) {
        assert!(Instant::now() < end, "the pull request never came");
        std::thread::sleep(Duration::from_millis(100));
    }
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let l = wait_loop(&e.tx, id, "reconciled", |v| v["stage"] == "publishing");
    assert_eq!(l["publish"]["publication"]["pr_number"], 1);
    assert_eq!(host_state(&state)["prs"].as_array().unwrap().len(), 1, "never a second");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // The same, but the pull request was never opened: back to ready, saying so.
    let root = scratch("p-restart-none");
    let (e, _repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        hosted(json!({ "login": "octo", "protected": { "main": true }, "sleep_ms": { "pr create": 3000 }, "fail": { "pr create": "HTTP 502" } })),
    );
    let tx = &e.tx;
    ready(tx, id);
    settled(tx, id);
    ask(tx, Request::Publish { id }).unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    while pr_creates(&root).is_empty() {
        assert!(Instant::now() < end, "pr create never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let fallow::service::ToolEngine { tx: t, handle, .. } = e;
    t.send(Msg::Shutdown).unwrap();
    handle.join().unwrap();
    std::thread::sleep(Duration::from_secs(4));
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let l = wait_loop(&e.tx, id, "reconciled", |v| !v["publish"]["failed"].is_null());
    assert_eq!(l["stage"], "ready");
    assert!(l["publish"]["failed"]["why"].as_str().unwrap().contains("interrupted"), "{}", l["publish"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// Publishes a loop on a protected remote; its engine, repository and the fake host's state.
fn published(name: &str) -> (fallow::service::ToolEngine, std::path::PathBuf, u64, std::path::PathBuf) {
    let root = scratch(name);
    let (e, repo, id) = locked_calc_prepared(
        &root,
        &calc_loop_ok(),
        None,
        None,
        hosted(json!({ "login": "octo", "protected": { "main": true } })),
    );
    ready(&e.tx, id);
    settled(&e.tx, id);
    ask(&e.tx, Request::Publish { id }).unwrap();
    wait_loop(&e.tx, id, "publishing", |v| v["stage"] == "publishing");
    let _ = repo;
    (e, root.clone(), id, root.join("host.json"))
}

fn set_pr(state: &Path, change: impl FnOnce(&mut Value)) {
    let mut s = host_state(state);
    change(&mut s["prs"][0]);
    set_host_state(state, &s);
}

fn check_lines(tx: &Sender<Msg>, id: u64) -> Vec<String> {
    let a = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: fallow::run::activity::Filter::default(),
        },
    )
    .unwrap();
    a["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["text"].as_str())
        .filter(|t| t.starts_with("Pull request #1"))
        .map(str::to_string)
        .collect()
}

#[test]
fn the_pull_request_is_followed_to_its_merge_on_the_remote() {
    let (e, root, id, state) = published("p-follow");
    let tx = &e.tx;
    let repo = root.join("repo");
    let local_main = git(&repo, &["rev-parse", "main"]);
    // Checks appear on the remote: shown as they are, recorded once per change.
    set_pr(&state, |pr| {
        pr["checks"] = json!([
            { "name": "test", "status": "IN_PROGRESS", "conclusion": "", "detailsUrl": "https://github.com/acme/calc/actions/runs/1" },
            { "name": "lint", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "" }
        ])
    });
    let l = wait_loop(tx, id, "the checks", |v| {
        v["publish"]["publication"]["checks"].as_array().is_some_and(|c| c.len() == 2)
    });
    let checks = &l["publish"]["publication"]["checks"];
    assert_eq!(checks[0], json!({ "name": "test", "state": "waiting", "url": "https://github.com/acme/calc/actions/runs/1" }));
    assert_eq!(checks[1]["state"], "failed", "a failing check is shown, nothing more: {checks}");
    assert_eq!(l["stage"], "publishing", "a failed check starts nothing");
    // Read again and again without a change: no new record.
    std::thread::sleep(Duration::from_secs(12));
    loop_view_twice(tx, id);
    assert_eq!(check_lines(tx, id).len(), 1, "{:?}", check_lines(tx, id));
    // The remote cannot be read for a moment: the last state stays, saying so.
    let mut s = host_state(&state);
    s["fail"] = json!({ "pr view": "gh: could not resolve host api.github.com" });
    set_host_state(&state, &s);
    let l = wait_loop(tx, id, "the failed read", |v| v["publish"]["pr_read"]["failed"].is_string());
    assert_eq!(l["publish"]["publication"]["state"], "open");
    assert!(l["publish"]["pr_read"]["ok_at"].is_string(), "{}", l["publish"]);
    s["fail"] = json!({});
    set_host_state(&state, &s);
    // Merged on the remote: the loop ends merged; the local target is not touched.
    set_pr(&state, |pr| {
        pr["state"] = json!("merged");
        pr["merged_at"] = json!("2026-11-03T08:00:00Z");
    });
    let l = wait_loop(tx, id, "merged", |v| v["stage"] == "done");
    assert_eq!(l["publish"]["publication"]["state"], "merged");
    assert_eq!(l["publish"]["publication"]["merged_at"], "2026-11-03T08:00:00Z");
    let v = wait_for(tx, id, "the record", |v| !v["merged"].is_null());
    assert_eq!(v["merged"], l["publish"]["publication"]["commit"]);
    assert_eq!(git(&repo, &["rev-parse", "main"]), local_main, "Fallow pulls nothing");
    let d = ask(tx, Request::Dashboard).unwrap();
    let f = d["features"].as_array().unwrap().iter().find(|f| f["name"] == "001-calc").unwrap().clone();
    assert_eq!(f["stage"], "merged", "{f}");
    assert!(!d["needs_you"].to_string().contains("pr_open"), "nothing left to do: {}", d["needs_you"]);
    assert!(check_lines(tx, id).iter().any(|l| l.contains("was merged on acme/calc")));
    // One notice for the merge, kept as seen.
    let seen = std::fs::read_to_string(root.join("data/notified.json")).unwrap_or_default();
    assert!(seen.contains(&format!("{id}:pr:1:merged")), "{seen}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

fn loop_view_twice(tx: &Sender<Msg>, id: u64) {
    for _ in 0..2 {
        ask(tx, Request::Loop { id }).unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn a_pull_request_closed_without_merging_puts_the_loop_back_to_ready() {
    let (e, root, id, state) = published("p-closed");
    let tx = &e.tx;
    set_pr(&state, |pr| pr["state"] = json!("closed"));
    let l = wait_loop(tx, id, "closed", |v| v["stage"] == "ready");
    assert_eq!(l["publish"]["publication"]["state"], "closed");
    assert_eq!(l["publish"]["offer"], json!(["pull_request"]), "publish again: {}", l["publish"]);
    // Published again: a new pull request from the same branch, at the same commit.
    ask(tx, Request::Publish { id }).unwrap();
    let l = wait_loop(tx, id, "published again", |v| {
        v["stage"] == "publishing" && v["publish"]["publication"]["pr_number"] == 2
    });
    assert_eq!(l["publish"]["publication"]["branch"], BRANCH);
    assert_eq!(host_state(&state)["prs"].as_array().unwrap().len(), 2);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
