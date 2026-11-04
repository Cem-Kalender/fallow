//! 011 US1 (quickstart Q1; FR-001 to FR-008): a loop whose target moved is brought up to date in
//! its own working copy, a conflict goes to a builder, and only a newly verified commit merges.
//! No network, no model: the fake runtime plays every step.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use fallow::run::events::Level;
use fallow::studio::Req;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Phase 1's checkpoint: two features of one project, both locked, each with its own loop.
#[test]
fn two_features_lock_in_one_project() {
    let root = scratch("u-two");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}", lead_writes_calc(), lead_writes_calc2()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let a = lock_calc_in(&e.tx, &repo, None, Some((5.0, 60)), None);
    // A project is named after its folder.
    let b = lock_feature_in(
        &e.tx,
        "repo",
        "Calc2",
        "002-calc2",
        None,
        Some((5.0, 60)),
        None,
    );
    assert_ne!(a, b);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

pub const CALC_INIT_MUL: &str = "def mul(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a * b\n";
pub const TEST_MUL: &str = "import unittest\nfrom calc import mul\n\n\nclass Mul(unittest.TestCase):\n    def test_mul(self):\n        self.assertEqual(mul(2, 3), 6)\n";
/// Both features' work in one file: what a builder's resolution of the conflict writes.
pub fn calc_resolved() -> String {
    format!("{CALC_INIT_SUB}\n\n{CALC_INIT_MUL}")
}

/// `002-calc2`'s loop: mul() in calc/__init__.py (the file 001-calc writes too), its tests, the
/// handover, the check.
fn calc2_loop_ok() -> String {
    let handover = "[[step]]\nkind = \"handover\"\npart = \"phase-1\"\nfiles = [{ path = \"specs/002-calc2/handover/phase-1.md\", text = '''\n## Built\nPhase 1.\n\n## Decisions\nNone.\n\n## For the next phase\nGo on.\n''' }]\noutput = { status = \"done\", path = \"specs/002-calc2/handover/phase-1.md\", summary = \"handed over\" }\n\n";
    [
        task_step("T001", &[("calc/__init__.py", CALC_INIT_MUL)], ""),
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

/// A project with `002-calc2` locked and ready first, then `001-calc` locked, ready and merged:
/// the second loop's target moved under it with a conflict in calc/__init__.py. `after` are the
/// scenario's steps for what follows (the update's repair, the checks again).
fn conflicting(root: &Path, after: &str) -> (fallow::service::ToolEngine, PathBuf, u64, u64) {
    conflicting_at(root, after, None, 5.0, "")
}

/// `conflicting`, with 002-calc2 locked at `level` with a budget of `usd`, and `squad` added to
/// the project's squad file before the locks.
fn conflicting_at(
    root: &Path,
    after: &str,
    level: Option<Level>,
    usd: f64,
    squad: &str,
) -> (fallow::service::ToolEngine, PathBuf, u64, u64) {
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
        format!(
            "{}{}{}{}{after}",
            lead_writes_calc(),
            lead_writes_calc2(),
            calc2_loop_ok(),
            calc_loop_ok()
        ),
    )
    .unwrap();
    let e = start_studio(root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    // Both worked out first (numbers follow creation), then 002 locked and built before 001.
    prepare_feature_in(tx, "repo", "Calc", "001-calc");
    prepare_feature_in(tx, "repo", "Calc2", "002-calc2");
    let b = lock_prepared(
        tx,
        "repo",
        "002-calc2",
        None,
        Some((usd, 60)),
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
        None,
        LockExtra::default(),
    );
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "001-calc merged", |v| v["stage"] == "done");
    (e, repo, a, b)
}

fn loop_view(tx: &Sender<Msg>, id: u64) -> Value {
    ask(tx, Request::Loop { id }).unwrap()
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
            "the target never showed as moved: {}",
            v["target"]
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Q1: the conflict is brought up to date by a builder and verified again; only the new
/// verified commit merges, and the target and the engineer's folder never held a conflict.
#[test]
fn a_conflicting_loop_is_brought_up_to_date_verified_again_and_merged() {
    let root = scratch("u-conflict");
    let after = format!(
        "{}{}",
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), false),
        check_step("US1", true, "")
    );
    let (e, repo, a, b) = conflicting(&root, &after);
    let tx = &e.tx;
    let main_before = git(&repo, &["rev-parse", "main"]);
    let folder_before = std::fs::read_to_string(repo.join("calc/__init__.py")).unwrap();
    // FR-001 to FR-003: the target moved, it would conflict, no merge is offered.
    let v = moved(tx, b);
    let t = &v["target"];
    assert_eq!(
        t["conflicts"],
        serde_json::json!(["calc/__init__.py"]),
        "{t}"
    );
    assert!(
        t["commits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["from"] == "001-calc"),
        "the first loop's merge is named (FR-002): {t}"
    );
    let offer = v["offer"].as_array().unwrap();
    assert!(offer.iter().any(|o| o == "bring_up_to_date"), "{offer:?}");
    assert!(!offer.iter().any(|o| o == "merge"), "{offer:?}");
    // A merge asked for anyway is refused, nothing touched (FR-003).
    assert!(
        ask(
            tx,
            Request::Merge {
                id: b,
                without_latest_check: false
            }
        )
        .is_err()
    );
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    // Bring it up to date: a repair on update-1 naming the file; while it runs, not ready.
    ask(tx, Request::Update { id: b }).unwrap();
    let s = wait_for(tx, b, "the update's repair", |v| {
        v["steps"]
            .as_object()
            .is_some_and(|m| m.values().any(|s| s["part"] == "update-1"))
    });
    assert_ne!(s["stage"], "ready", "FR-003a: not ready during an update");
    assert!(
        ask(
            tx,
            Request::Merge {
                id: b,
                without_latest_check: false
            }
        )
        .is_err(),
        "the old commit is not merged"
    );
    let prompt = std::fs::read_dir(
        root.join("data")
            .join("runs")
            .join(b.to_string())
            .join("steps"),
    )
    .unwrap()
    .flatten()
    .filter(|f| f.path().extension().is_some_and(|x| x == "prompt"))
    .map(|f| std::fs::read_to_string(f.path()).unwrap_or_default())
    .find(|t| t.contains("update-1"))
    .expect("the update's prompt");
    assert!(prompt.contains("calc/__init__.py"), "{prompt}");
    // Verified again and ready with a merge commit of the old head and the target (FR-006).
    let r = wait_for(tx, b, "ready again", |v| {
        v["stage"] == "ready"
            && v["feature_loop"]["update"].is_null()
            && v["feature_loop"]["updates"] == 1
    });
    let new = r["integrated"].as_str().unwrap().to_string();
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", &new]);
    assert!(
        parents.contains(&main_before),
        "the target is a parent of the result: {parents}"
    );
    let reviews = r["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == "review" && s["part"] == "US1")
        .count();
    assert_eq!(reviews, 2, "the story was checked again after the update");
    // The target and the engineer's folder never changed during the update (SC-003).
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    assert_eq!(
        std::fs::read_to_string(repo.join("calc/__init__.py")).unwrap(),
        folder_before
    );
    // The new verified commit merges without conflict.
    merge_when_read(tx, b).unwrap();
    wait_for(tx, b, "002-calc2 merged", |v| v["stage"] == "done");
    let merged = git(&repo, &["show", "main:calc/__init__.py"]);
    assert!(
        merged.contains("def sub") && merged.contains("def mul"),
        "{merged}"
    );
    assert!(!merged.contains("<<<<<<<"));
    let _ = a;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A file still holding a marker is refused and repaired again; the builder's own commit counts
/// like any (Fallow takes the tree).
#[test]
fn a_marker_left_is_refused_and_the_builder_may_commit_itself() {
    let root = scratch("u-marker");
    let marked = format!("<<<<<<< ours\n{CALC_INIT_MUL}=======\n{CALC_INIT_SUB}>>>>>>> theirs\n");
    let after = format!(
        "{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", &marked, false),
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true),
        check_step("US1", true, "")
    );
    let (e, repo, _a, b) = conflicting(&root, &after);
    let tx = &e.tx;
    moved(tx, b);
    ask(tx, Request::Update { id: b }).unwrap();
    let r = wait_for(tx, b, "ready again", |v| {
        v["stage"] == "ready"
            && v["feature_loop"]["update"].is_null()
            && v["feature_loop"]["updates"] == 1
    });
    let tries = r["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["part"] == "update-1")
        .count();
    assert_eq!(tries, 2, "the first resolution was refused: {}", r["steps"]);
    let steps = r["feature_loop"]["update_steps"].to_string();
    assert!(steps.contains("merged in by Fallow"), "{steps}");
    merge_when_read(tx, b).unwrap();
    wait_for(tx, b, "merged", |v| v["stage"] == "done");
    assert!(!git(&repo, &["show", "main:calc/__init__.py"]).contains("<<<<<<<"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007: the update's repairs count against the cap; at the cap the engineer decides and
/// nothing is merged.
#[test]
fn the_repair_cap_stops_an_update_for_the_engineer() {
    let root = scratch("u-cap");
    let marked = format!("<<<<<<< ours\n{CALC_INIT_MUL}=======\n{CALC_INIT_SUB}>>>>>>> theirs\n");
    let after = [
        conflict_repair_step(1, "calc/__init__.py", &marked, false),
        conflict_repair_step(1, "calc/__init__.py", &marked, false),
        conflict_repair_step(1, "calc/__init__.py", &marked, false),
        conflict_repair_step(1, "calc/__init__.py", &marked, false),
    ]
    .concat();
    let (e, repo, _a, b) = conflicting(&root, &after);
    let tx = &e.tx;
    let main_before = git(&repo, &["rev-parse", "main"]);
    moved(tx, b);
    ask(tx, Request::Update { id: b }).unwrap();
    let v = wait_for(tx, b, "the cap", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("still not resolved"))
    });
    assert_ne!(v["stage"], "ready");
    assert!(
        ask(
            tx,
            Request::Merge {
                id: b,
                without_latest_check: false
            }
        )
        .is_err()
    );
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-005: the target changed this feature's documents: nothing merged, no builder, the engineer
/// decides (Lock again or Stop).
#[test]
fn a_target_that_changed_the_features_documents_stops_the_update() {
    let root = scratch("u-docs");
    let (e, repo, _a, b) = conflicting(&root, "");
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "specs/002-calc2/spec.md",
        "# changed on main\n",
        "edit the spec",
    );
    let v = moved(tx, b);
    assert_eq!(
        v["target"]["docs_changed"],
        serde_json::json!(["specs/002-calc2/spec.md"]),
        "{}",
        v["target"]
    );
    assert!(
        v["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "lock_again"),
        "{}",
        v["offer"]
    );
    ask(tx, Request::Update { id: b }).unwrap();
    let s = wait_for(tx, b, "the documents' stop", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("documents"))
            && v["feature_loop"]["update"]["docs_changed"].is_array()
    });
    assert!(
        !s["steps"]
            .as_object()
            .unwrap()
            .values()
            .any(|x| x["part"] == "update-1"),
        "no builder"
    );
    assert_eq!(
        s["feature_loop"]["update"]["docs_changed"][0],
        "specs/002-calc2/spec.md"
    );
    assert_ne!(s["stage"], "ready");
    // Held: Lock again is offered; another "Bring up to date" is refused and records nothing,
    // so the question stays (review 1, 2).
    let held = loop_view(tx, b);
    assert_eq!(
        held["offer"],
        serde_json::json!(["lock_again", "stop"]),
        "{}",
        held["offer"]
    );
    let steered = held["steering"].as_array().map_or(0, Vec::len);
    assert!(ask(tx, Request::Update { id: b }).is_err());
    assert_eq!(
        loop_view(tx, b)["steering"].as_array().map_or(0, Vec::len),
        steered
    );
    wait_for(tx, b, "the question kept", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("documents"))
    });
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A target branch that is gone: nothing merged, the page says so and offers stop.
#[test]
fn a_gone_target_is_said_and_nothing_merges() {
    let root = scratch("u-gone");
    let (e, repo, _a, b) = conflicting(&root, "");
    let tx = &e.tx;
    git(&repo, &["branch", "-m", "main", "trunk"]);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let v = loop {
        let v = loop_view(tx, b);
        if v["target"]["gone"] == true {
            break v;
        }
        assert!(
            std::time::Instant::now() < end,
            "never gone: {}",
            v["target"]
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    assert_eq!(v["offer"], serde_json::json!(["stop"]));
    let why = ask(
        tx,
        Request::Merge {
            id: b,
            without_latest_check: false,
        },
    )
    .unwrap_err();
    assert!(why.contains("is gone"), "{why}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A conflict without markers (a file deleted on one side, changed on the other) needs the
/// builder's `resolutions`; without them it is refused, with them it merges.
#[test]
fn a_non_text_conflict_needs_resolutions() {
    let root = scratch("u-nontext");
    let after = format!(
        "{}{}{}",
        // First: the text resolved, the deletion kept, but no word on it.
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), false),
        nontext_repair_step(
            1,
            "README.md",
            None,
            "kept the deletion: the README moved to docs"
        ),
        check_step("US1", true, "")
    );
    let (e, repo, _a, b) = conflicting_readme(&root, &after);
    let tx = &e.tx;
    let v = moved(tx, b);
    let conflicts = v["target"]["conflicts"].as_array().unwrap().clone();
    assert!(conflicts.iter().any(|c| c == "README.md"), "{conflicts:?}");
    ask(tx, Request::Update { id: b }).unwrap();
    let r = wait_for(tx, b, "ready again", |v| {
        v["stage"] == "ready"
            && v["feature_loop"]["update"].is_null()
            && v["feature_loop"]["updates"] == 1
    });
    let mut tries: Vec<(u64, String)> = r["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["part"] == "update-1")
        .map(|s| (s["n"].as_u64().unwrap_or(0), s["verified"].to_string()))
        .collect();
    tries.sort();
    assert_eq!(
        tries.len(),
        2,
        "the first, without resolutions, was not taken: {tries:?}"
    );
    let second = std::fs::read_to_string(
        root.join("data")
            .join("runs")
            .join(b.to_string())
            .join("steps")
            .join(format!("{}.prompt", tries[1].0)),
    )
    .unwrap();
    assert!(
        second.contains("README.md had a conflict without markers"),
        "{second}"
    );
    let _ = repo;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// `conflicting`, where 002-calc2's T002 also deletes README.md and main changes it after
/// 001-calc is merged: a modify/delete conflict besides the text one.
fn conflicting_readme(
    root: &Path,
    after: &str,
) -> (fallow::service::ToolEngine, PathBuf, u64, u64) {
    let repo = repo(root);
    let scenario = root.join("scenario.toml");
    let calc2 = calc2_loop_ok().replacen(
        "part = \"T002\"\n",
        "part = \"T002\"\ndelete = [\"README.md\"]\n",
        1,
    );
    std::fs::write(
        &scenario,
        format!(
            "{}{}{calc2}{}{after}",
            lead_writes_calc(),
            lead_writes_calc2(),
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
        None,
        LockExtra::default(),
    );
    wait_for(tx, b, "002-calc2 ready", |v| v["stage"] == "ready");
    let a = lock_prepared(
        tx,
        "repo",
        "001-calc",
        None,
        Some((5.0, 60)),
        None,
        LockExtra::default(),
    );
    wait_for(tx, a, "001-calc ready", |v| v["stage"] == "ready");
    merge_when_read(tx, a).unwrap();
    wait_for(tx, a, "001-calc merged", |v| v["stage"] == "done");
    commit_on(&repo, "main", "README.md", "calc, documented\n", "readme");
    (e, repo, a, b)
}

/// FR-003, FR-009a: the target is checked again right before merging, whatever the page last
/// read: a merge asked for after the target moved merges nothing; the loop is brought up to
/// date first (the update made right before the merge asked for).
#[test]
fn a_merge_right_after_the_target_moved_is_not_tried() {
    let root = scratch("u-recheck");
    let (e, repo, _a, b) = conflicting(&root, "");
    let tx = &e.tx;
    let main_before = git(&repo, &["rev-parse", "main"]);
    match ask(
        tx,
        Request::Merge {
            id: b,
            without_latest_check: false,
        },
    ) {
        // The watch had read the move and its conflict.
        Err(why) => assert!(why.contains("moved"), "{why}"),
        // Asked before the watch read it: in its line, brought up to date first; at Normal it
        // asks before the update (FR-010).
        Ok(_) => {
            wait_for(tx, b, "brought up to date first, or asked", |v| {
                v["feature_loop"]["updates"] == 1
                    || v["needs_engineer"]
                        .as_str()
                        .is_some_and(|n| n.contains("Bring it up to date"))
            });
        }
    }
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-005, research R-2: "Lock again" unlocks the feature and gives the spec session a message
/// with the target's version; keeping the loop's own version and locking again, the loop goes
/// on and the same change is not flagged again.
#[test]
fn lock_again_reconciles_and_the_same_change_is_not_flagged_again() {
    let root = scratch("u-lockagain");
    let (e, repo, _a, b) = conflicting(&root, LEAD_ANALYZES_AGAIN);
    let tx = &e.tx;
    commit_on(
        &repo,
        "main",
        "specs/002-calc2/spec.md",
        "# changed on main\n",
        "edit the spec",
    );
    let v = moved(tx, b);
    assert!(
        v["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "lock_again")
    );
    let la = ask(tx, Request::LockAgain { id: b }).unwrap();
    let msg = la["message"].as_str().unwrap();
    assert!(
        msg.contains("specs/002-calc2/spec.md") && msg.contains("changed on main"),
        "{msg}"
    );
    let s = wait_for(tx, b, "unlocked", |v| v["feature_loop"]["unlocked"] == true);
    assert!(s["feature_loop"]["unlocked"] == true);
    // The engineer keeps the loop's version: analyze again, lock again.
    analyze_again(tx, "repo", "002-calc2");
    let l = ask(
        tx,
        Request::Lock {
            project: "repo".into(),
            feature: "002-calc2".into(),
            pipeline: None,
            level: None,
            budget_usd: None,
            budget_minutes: None,
            when_verified: None,
            update_limit: None,
            run_after: None,
        },
    )
    .unwrap();
    assert_eq!(l["loop"], b, "the same loop goes on");
    let r = wait_for(tx, b, "ready after the new lock", |v| v["stage"] == "ready");
    assert!(
        r["feature_loop"]["reconciled_target"].is_string(),
        "{}",
        r["feature_loop"]
    );
    // The target still moved (001-calc's merge), but the documents' change is not flagged again.
    let v = moved(tx, b);
    assert_eq!(
        v["target"]["docs_changed"],
        serde_json::json!([]),
        "{}",
        v["target"]
    );
    assert!(
        v["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "bring_up_to_date"),
        "{}",
        v["offer"]
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T019's page check: not a test. `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test
/// us11_update serve_the_011_demo -- --ignored --nocapture` serves a project whose `002-calc2`
/// loop is ready while `001-calc`, merged first, conflicts with it (text in calc/__init__.py, a
/// deleted README.md changed on main); its update's repair and checks are slowed so the page can
/// be seen during the update. The page's address goes to `<root>/url.txt`; it serves for
/// `FALLOW_VISUAL_SECS` seconds (default 1800).
#[test]
#[ignore]
fn serve_the_011_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let slow = |step: String, ms: u64| {
        step.replacen("[[step]]\n", &format!("[[step]]\nsleep_ms = {ms}\n"), 1)
    };
    let repair = format!(
        "[[step]]\nkind = \"repair\"\npart = \"update-1\"\nfiles = [{{ path = \"calc/__init__.py\", text = {} }}]\ndelete = [\"README.md\"]\noutput = {{ status = \"done\", files = [\"calc/__init__.py\", \"README.md\"], summary = \"kept sub() and mul()\", resolutions = [{{ path = \"README.md\", how = \"kept the deletion: this feature moved the README to docs/\" }}] }}\n\n",
        lit(&calc_resolved())
    );
    let after = format!(
        "{}{}",
        slow(repair, 30_000),
        slow(check_step("US1", true, ""), 15_000)
    );
    let (e, _repo, _a, _b) = conflicting_readme(&root, &after);
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}

/// US3 scenario 1 (Q3): a loop locked Free whose target moved brings itself up to date, verifies
/// again and is ready, with no action of the engineer; each step is recorded.
#[test]
fn a_free_loop_brings_itself_up_to_date() {
    let root = scratch("u-free");
    let after = format!(
        "{}{}",
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true),
        check_step("US1", true, "")
    );
    let (e, _repo, _a, b) = conflicting_at(&root, &after, Some(Level::Free), 5.0, "");
    let tx = &e.tx;
    let r = wait_for(tx, b, "ready again by itself", |v| {
        v["stage"] == "ready"
            && v["feature_loop"]["update"].is_null()
            && v["feature_loop"]["updates"] == 1
    });
    assert!(
        !r["steering"].to_string().contains("\"update\""),
        "no action of the engineer: {}",
        r["steering"]
    );
    assert!(
        r["feature_loop"]["update_steps"]
            .to_string()
            .contains("bringing the loop up to date")
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// US3 scenario 2 (Q3): Normal, Strict, and Free with `[updates] auto = false` wait for the
/// engineer, shown under "Needs you" as `update`; nothing starts.
#[test]
fn normal_strict_and_auto_off_ask_first() {
    for (name, level, squad) in [
        ("u-normal", Level::Normal, ""),
        ("u-strict", Level::Strict, ""),
        ("u-autooff", Level::Free, "[updates]\nauto = false\n"),
    ] {
        let root = scratch(name);
        let (e, _repo, _a, b) = conflicting_at(&root, "", Some(level), 5.0, squad);
        let tx = &e.tx;
        wait_for(tx, b, "the question", |v| {
            v["needs_engineer"]
                .as_str()
                .is_some_and(|n| n.contains("Bring it up to date"))
        });
        let d = ask(tx, Request::Dashboard).unwrap().to_string();
        assert!(d.contains("\"kind\":\"update\""), "{name}: {d}");
        std::thread::sleep(std::time::Duration::from_secs(2));
        let v = wait_for(tx, b, "still waiting", |_| true);
        assert_eq!(v["feature_loop"]["updates"], 0, "{name}: nothing started");
        drop(e);
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// US3 scenario 3, FR-010: a Free loop whose budget is used up asks, naming the limit.
/// The fake runtime's steps cost $0.01 each: 002-calc2's four reach $0.04 at ready.
#[test]
fn a_free_loop_over_its_budget_asks() {
    let root = scratch("u-budget");
    let (e, _repo, _a, b) = conflicting_at(&root, "", Some(Level::Free), 0.04, "");
    let tx = &e.tx;
    let v = wait_for(tx, b, "the question", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains("limit is used up"))
    });
    assert_eq!(v["feature_loop"]["updates"], 0, "nothing started");
    // "Needs you" names the limit, never "Ready to merge" (FR-010).
    let d = ask(tx, Request::Dashboard).unwrap().to_string();
    assert!(d.contains("stopped at a limit"), "{d}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007: the budget used up during an update stops it there; the loop asks, nothing merged.
/// $0.04 at ready, under the $0.045 budget: the update starts; its repair ($0.01) uses it up.
#[test]
fn a_budget_used_up_during_an_update_stops_it() {
    let root = scratch("u-budget2");
    let after = format!(
        "{}{}",
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true),
        check_step("US1", true, "")
    );
    let (e, repo, _a, b) = conflicting_at(&root, &after, Some(Level::Free), 0.045, "");
    let tx = &e.tx;
    let main_before = git(&repo, &["rev-parse", "main"]);
    let v = wait_for(tx, b, "stopped at the limit", |v| {
        v["feature_loop"]["updates"] == 1 && v["limit_reached"].is_string()
    });
    assert_ne!(v["stage"], "ready");
    assert!(v["needs_engineer"].is_string(), "{}", v["needs_engineer"]);
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// Stops the engine and starts it again on the same folders (constitution VII).
fn restart(e: fallow::service::ToolEngine, root: &Path) -> fallow::service::ToolEngine {
    let fallow::service::ToolEngine { tx: t, handle, .. } = e;
    t.send(Msg::Shutdown).unwrap();
    handle.join().unwrap();
    start_studio(root, &root.join("scenario.toml"), None)
}

/// Merge commits (two parents) on the loop's branch since its lock.
fn merges_on(repo: &Path, branch: &str, since: &str) -> usize {
    git(
        repo,
        &[
            "rev-list",
            "--merges",
            "--count",
            &format!("{since}..{branch}"),
        ],
    )
    .parse()
    .unwrap_or(0)
}

/// T036, FR-017: a restart right after an update started (its merge being computed): the
/// computation is made again, one merge, the loop verified again and ready.
#[test]
fn a_restart_while_the_update_is_computed_resumes_it_once() {
    let root = scratch("u-restart-a");
    let after = format!(
        "{}{}",
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true),
        check_step("US1", true, "")
    );
    let (e, repo, _a, b) = conflicting(&root, &after);
    moved(&e.tx, b);
    let lock = git(&repo, &["rev-parse", "main"]);
    ask(&e.tx, Request::Update { id: b }).unwrap();
    let e = restart(e, &root);
    let tx = &e.tx;
    let r = wait_for(tx, b, "ready again after the restart", |v| {
        v["stage"] == "ready" && v["feature_loop"]["update"].is_null()
    });
    assert_eq!(r["feature_loop"]["updates"], 1, "one update");
    assert_eq!(
        merges_on(&repo, "fallow/002-calc2/build", &lock),
        1,
        "one merge of the target"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T036, FR-017: a restart while the update's repair runs: the repair goes on in its session,
/// the resolution is not lost, one merge commit.
#[test]
fn a_restart_during_the_update_repair_keeps_the_resolution() {
    let root = scratch("u-restart-b");
    // The repair is slow, so the restart lands during it; a second one answers the step resumed.
    let slow = conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true).replacen(
        "[[step]]\n",
        "[[step]]\nsleep_ms = 15000\n",
        1,
    );
    let after = format!(
        "{slow}{}{}",
        conflict_repair_step(1, "calc/__init__.py", &calc_resolved(), true),
        check_step("US1", true, "")
    );
    let (e, repo, _a, b) = conflicting(&root, &after);
    moved(&e.tx, b);
    let lock = git(&repo, &["rev-parse", "main"]);
    ask(&e.tx, Request::Update { id: b }).unwrap();
    wait_for(&e.tx, b, "the repair running", |v| {
        v["steps"]
            .as_object()
            .is_some_and(|m| m.values().any(|s| s["part"] == "update-1"))
    });
    let e = restart(e, &root);
    let tx = &e.tx;
    let r = wait_for(tx, b, "ready again after the restart", |v| {
        v["stage"] == "ready" && v["feature_loop"]["update"].is_null()
    });
    assert_eq!(r["feature_loop"]["updates"], 1);
    assert_eq!(merges_on(&repo, "fallow/002-calc2/build", &lock), 1);
    let file = git(&repo, &["show", "fallow/002-calc2/build:calc/__init__.py"]);
    assert!(
        !file.contains("<<<<<<<") && file.contains("def mul"),
        "{file}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
