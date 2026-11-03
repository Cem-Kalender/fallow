//! 011 US2 (quickstart Q2 and Q4; FR-007, FR-009, FR-009a, FR-017): a target that moved without
//! a conflict is verified again too, merges into one target take turns, and updates overtaken
//! from outside Fallow count up to the lock's limit, then the engineer decides.
//! No network, no model: the fake runtime plays every step.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use fallow::run::events::Level;
use fallow::studio::Req;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

const MUL: &str = "def mul(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a * b\n";
const TEST_MUL: &str = "import unittest\nfrom calc.mul import mul\n\n\nclass Mul(unittest.TestCase):\n    def test_mul(self):\n        self.assertEqual(mul(2, 3), 6)\n";

/// `002-calc2`'s loop in its own file (calc/mul.py): nothing conflicts with `001-calc`.
fn mul_loop_ok() -> String {
    let handover = "[[step]]\nkind = \"handover\"\npart = \"phase-1\"\nfiles = [{ path = \"specs/002-calc2/handover/phase-1.md\", text = '''\n## Built\nPhase 1.\n\n## Decisions\nNone.\n\n## For the next phase\nGo on.\n''' }]\noutput = { status = \"done\", path = \"specs/002-calc2/handover/phase-1.md\", summary = \"handed over\" }\n\n";
    [
        task_step("T001", &[("calc/mul.py", MUL)], ""),
        task_step(
            "T002",
            &[("tests/__init__.py", ""), ("tests/test_mul.py", TEST_MUL)],
            "",
        ),
        handover.to_string(),
        check_step("US1", true, ""),
    ]
    .concat()
}

/// A check step that takes `ms` (an update's checks, slowed so the line can be seen).
fn slow_check(story: &str, ms: u64) -> String {
    check_step(story, true, &format!("sleep_ms = {ms}"))
}

/// A project with `002-calc2` (b) and `001-calc` (a) both ready and nothing merged; `after` are
/// the scenario's steps for what follows.
fn two_ready(
    root: &Path,
    after: &str,
    level: Option<Level>,
    extra_a: LockExtra,
) -> (fallow::service::ToolEngine, PathBuf, u64, u64) {
    let repo = repo(root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            "{}{}{}{}{after}",
            lead_writes_calc(),
            lead_writes_calc2(),
            mul_loop_ok(),
            calc_loop_ok()
        ),
    )
    .unwrap();
    let e = start_studio(root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    prepare_feature_in(tx, "repo", "Calc", "001-calc");
    prepare_feature_in(tx, "repo", "Calc2", "002-calc2");
    let b = lock_prepared(
        tx,
        "repo",
        "002-calc2",
        None,
        Some((5.0, 60)),
        level,
        LockExtra::default(),
    );
    wait_for(tx, b, "002-calc2 ready", |v| v["stage"] == "ready");
    let a = lock_prepared(
        tx,
        "repo",
        "001-calc",
        None,
        Some((5.0, 60)),
        level,
        extra_a,
    );
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    (e, repo, a, b)
}

fn loop_view(tx: &Sender<Msg>, id: u64) -> Value {
    ask(tx, Request::Loop { id }).unwrap()
}

fn merge(tx: &Sender<Msg>, id: u64) -> Result<Value, String> {
    ask(
        tx,
        Request::Merge {
            id,
            without_latest_check: false,
        },
    )
}

/// Waits until the loop's view says the target moved (the target watch read it).
fn moved(tx: &Sender<Msg>, id: u64) -> Value {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let v = loop_view(tx, id);
        if v["target"]["moved"] == true {
            return v;
        }
        assert!(
            std::time::Instant::now() < end,
            "never moved: {}",
            v["target"]
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Waits until the loop's view offers `what` (the target is read again after an update).
fn offered(tx: &Sender<Msg>, id: u64, what: &str) -> Value {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let v = loop_view(tx, id);
        if v["offer"]
            .as_array()
            .is_some_and(|o| o.iter().any(|o| o == what))
        {
            return v;
        }
        assert!(
            std::time::Instant::now() < end,
            "never offered {what}: {}",
            v["offer"]
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// "Merge without the latest check", once the remote was read (010).
fn merge_unchecked_when_read(tx: &Sender<Msg>, id: u64) -> Result<Value, String> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let r = ask(
            tx,
            Request::Merge {
                id,
                without_latest_check: true,
            },
        );
        match r {
            Err(e) if e.contains("reading the remote") && std::time::Instant::now() < end => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            r => return r,
        }
    }
}

fn parts(v: &Value, part: &str) -> usize {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["part"] == part)
        .count()
}

/// FR-009 (Q2): a target that moved without a conflict: no builder, the gates on the merge
/// commit and the story checked again before `merge` is offered; what merges is the new commit.
#[test]
fn a_target_that_moved_without_conflict_is_verified_again() {
    let root = scratch("l-clean");
    let (e, repo, a, b) = two_ready(
        &root,
        &check_step("US1", true, ""),
        None,
        LockExtra::default(),
    );
    let tx = &e.tx;
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "001-calc merged", |v| v["stage"] == "done");
    let v = moved(tx, b);
    assert_eq!(v["target"]["conflicts"], serde_json::json!([]));
    let offer = v["offer"].as_array().unwrap();
    assert!(offer.iter().any(|o| o == "bring_up_to_date"), "{offer:?}");
    assert!(!offer.iter().any(|o| o == "merge"), "{offer:?}");
    ask(tx, Request::Update { id: b }).unwrap();
    let r = wait_for(tx, b, "ready again", |v| {
        v["stage"] == "ready"
            && v["feature_loop"]["update"].is_null()
            && v["feature_loop"]["updates"] == 1
    });
    assert_eq!(parts(&r, "update-1"), 0, "no builder step");
    let gates = r["gate_runs"].to_string();
    assert!(
        gates.contains("update-1"),
        "the gates ran on the merge commit: {gates}"
    );
    let integrated = r["integrated"].as_str().unwrap().to_string();
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", &integrated]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "a merge commit: {parents}"
    );
    let v = offered(tx, b, "merge");
    assert!(
        v["offer"].as_array().unwrap().iter().any(|o| o == "merge"),
        "{}",
        v["offer"]
    );
    merge(tx, b).unwrap();
    wait_for(tx, b, "merged", |v| v["stage"] == "done");
    assert!(git(&repo, &["merge-base", "--is-ancestor", &integrated, "main"]).is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-009a, SC-007 (Q4): both asked to merge; the second waits in line while the first is
/// brought up to date for its merge and merges; then it is verified again once and merges.
#[test]
fn merges_into_one_target_take_turns() {
    let root = scratch("l-line");
    // a's update (both stories, the first slowed), then b's.
    let after = format!(
        "{}{}{}",
        slow_check("US1", 8000),
        check_step("US2", true, ""),
        check_step("US1", true, "")
    );
    let (e, repo, a, b) = two_ready(&root, &after, Some(Level::Free), LockExtra::default());
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "NOTES.md",
        "outside\n",
        "a note from outside Fallow",
    );
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "a brought up to date for its merge", |v| {
        !v["feature_loop"]["update"].is_null()
    });
    let reply = merge(tx, b).unwrap();
    assert_eq!(reply["in_line_after"], "001-calc", "{reply}");
    let v = loop_view(tx, b);
    assert_eq!(v["line"]["after"], "001-calc", "{}", v["line"]);
    wait_for(tx, a, "a merged", |v| v["stage"] == "done");
    let r = wait_for(tx, b, "b merged", |v| v["stage"] == "done");
    assert_eq!(
        r["feature_loop"]["updates"], 1,
        "verified again once (SC-007)"
    );
    assert_eq!(r["feature_loop"]["overtaken"], 0);
    let ra = wait_for(tx, a, "a", |_| true);
    assert_eq!(ra["feature_loop"]["updates"], 1);
    assert_eq!(
        ra["feature_loop"]["overtaken"], 0,
        "the move came before its turn"
    );
    let log = git(&repo, &["log", "--oneline", "main"]);
    assert!(log.contains("T001"), "{log}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-009a: a loop that waits for the engineer (here: paused) leaves the line; the one behind
/// goes on.
#[test]
fn a_loop_that_waits_leaves_the_line() {
    let root = scratch("l-leave");
    let after = format!(
        "{}{}{}",
        slow_check("US1", 8000),
        check_step("US2", true, ""),
        check_step("US1", true, "")
    );
    let (e, repo, a, b) = two_ready(&root, &after, Some(Level::Free), LockExtra::default());
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "NOTES.md",
        "outside\n",
        "a note from outside Fallow",
    );
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "a brought up to date for its merge", |v| {
        !v["feature_loop"]["update"].is_null()
    });
    assert_eq!(merge(tx, b).unwrap()["in_line_after"], "001-calc");
    ask(tx, Request::Pause { id: a }).unwrap();
    wait_for(tx, a, "a left the line", |v| {
        v["feature_loop"]["lined_at"].is_null()
    });
    wait_for(tx, b, "b merged", |v| v["stage"] == "done");
    assert_ne!(wait_for(tx, a, "a", |_| true)["stage"], "done");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007, US2 scenario 3 (Q4): with `update_limit = 1`, a commit from outside Fallow while the
/// loop holds its turn: overtaken, the engineer decides; merging without the latest check is
/// recorded with the commit it was not verified with, never as verified; the count survives a
/// restart (FR-017).
#[test]
fn an_overtaken_update_at_the_limit_asks_and_may_merge_unchecked() {
    let root = scratch("l-limit");
    let after = format!("{}{}", slow_check("US1", 8000), check_step("US2", true, ""));
    let extra = LockExtra {
        update_limit: Some(1),
        run_after: None,
    };
    let (e, repo, a, _b) = two_ready(&root, &after, Some(Level::Free), extra);
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "NOTES.md",
        "outside\n",
        "a note from outside Fallow",
    );
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "a brought up to date for its merge", |v| {
        !v["feature_loop"]["update"].is_null()
    });
    let late = commit_on(&repo, "main", "NOTES2.md", "later\n", "another note");
    let v = wait_for(tx, a, "the limit", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("overtaken"))
    });
    assert_eq!(v["feature_loop"]["overtaken"], 1);
    // "Needs you" says the limit was reached (contracts/web-api.md).
    let d = ask(tx, Request::Dashboard).unwrap().to_string();
    assert!(d.contains("update_limit"), "{d}");
    let options = v["needs"]["options"].to_string();
    assert!(
        options.contains("verify again on the latest main")
            && options.contains("merge without the latest check")
            && options.contains("stop"),
        "{options}"
    );
    // A restart keeps the count and the question.
    let fallow::service::ToolEngine { tx: t, handle, .. } = e;
    t.send(Msg::Shutdown).unwrap();
    handle.join().unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let tx = &e.tx;
    let v = wait_for(tx, a, "after the restart", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["overtaken"], 1);
    assert!(v["needs_engineer"].is_string(), "{}", v["needs_engineer"]);
    let main_before = git(&repo, &["rev-parse", "main"]);
    merge_unchecked_when_read(tx, a).unwrap();
    let r = wait_for(tx, a, "merged unchecked", |v| v["stage"] == "done");
    assert_eq!(r["feature_loop"]["unchecked"], serde_json::json!([late]));
    let lv = loop_view(tx, a);
    assert_eq!(lv["unchecked"], serde_json::json!([late]));
    assert_ne!(git(&repo, &["rev-parse", "main"]), main_before);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// "Verify again on the latest <target>" starts the count again: another update, then the merge.
#[test]
fn verify_again_resets_the_count_and_merges_after() {
    let root = scratch("l-again");
    let after = format!(
        "{}{}{}{}",
        slow_check("US1", 8000),
        check_step("US2", true, ""),
        check_step("US1", true, ""),
        check_step("US2", true, "")
    );
    let extra = LockExtra {
        update_limit: Some(1),
        run_after: None,
    };
    let (e, repo, a, _b) = two_ready(&root, &after, Some(Level::Free), extra);
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "NOTES.md",
        "outside\n",
        "a note from outside Fallow",
    );
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "a brought up to date for its merge", |v| {
        !v["feature_loop"]["update"].is_null()
    });
    commit_on(&repo, "main", "NOTES2.md", "later\n", "another note");
    wait_for(tx, a, "the limit", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("overtaken"))
    });
    ask(tx, Request::Update { id: a }).unwrap();
    let r = wait_for(tx, a, "merged", |v| v["stage"] == "done");
    assert_eq!(r["feature_loop"]["overtaken"], 0);
    assert_eq!(r["feature_loop"]["updates"], 2);
    assert_eq!(r["feature_loop"]["unchecked"], serde_json::json!([]));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// `[updates] line = false`: no line forms; every move found at the merge's recheck counts.
#[test]
fn without_the_line_every_move_at_the_recheck_counts() {
    let root = scratch("l-noline");
    let extra = LockExtra {
        update_limit: Some(1),
        run_after: None,
    };
    // The switch is read from the squad file at the lock commit.
    let (e, repo, a, b) = two_ready_with_squad(&root, "", extra, "[updates]\nline = false\n");
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "NOTES.md",
        "outside\n",
        "a note from outside Fallow",
    );
    let reply = merge_when_read(tx, a).unwrap();
    assert!(reply["in_line_after"].is_null(), "{reply}");
    wait_for(tx, a, "the limit", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("overtaken"))
    });
    // b is not held behind a.
    assert!(merge(tx, b).unwrap()["in_line_after"].is_null());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// `two_ready`, with `squad` appended to the project's `.fallow/squad.toml` before the locks.
fn two_ready_with_squad(
    root: &Path,
    after: &str,
    extra_a: LockExtra,
    squad: &str,
) -> (fallow::service::ToolEngine, PathBuf, u64, u64) {
    let repo = repo(root);
    let path = repo.join(".fallow").join("squad.toml");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(squad);
    std::fs::write(&path, text).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "squad"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            "{}{}{}{}{after}",
            lead_writes_calc(),
            lead_writes_calc2(),
            mul_loop_ok(),
            calc_loop_ok()
        ),
    )
    .unwrap();
    let e = start_studio(root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    prepare_feature_in(tx, "repo", "Calc", "001-calc");
    prepare_feature_in(tx, "repo", "Calc2", "002-calc2");
    let b = lock_prepared(
        tx,
        "repo",
        "002-calc2",
        None,
        Some((5.0, 60)),
        Some(Level::Free),
        LockExtra::default(),
    );
    wait_for(tx, b, "002-calc2 ready", |v| v["stage"] == "ready");
    let a = lock_prepared(
        tx,
        "repo",
        "001-calc",
        None,
        Some((5.0, 60)),
        Some(Level::Free),
        extra_a,
    );
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    (e, repo, a, b)
}
