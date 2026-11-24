//! 011 US4 (quickstart Q5; FR-011, FR-011a, FR-012, FR-016b): a published loop whose pull
//! request conflicts with its target, or fell behind one that requires it, is brought up to date
//! against the target as the remote has it and pushed forward on its own branch: never forced,
//! never a second branch or pull request, the target untouched. A bare remote and the fake hosting
//! tool; no network, no model.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const ADDRESS: &str = "https://github.com/acme/calc.git";
const BRANCH: &str = "fallow/001-calc/build";
/// What main says on the remote: a change of calc/__init__.py, which 001-calc writes too.
const REMOTE_INIT: &str = "def add(a, b):\n    return a + b  # from the remote\n";

/// Waits until the loop's view satisfies `pred` (reading the loop also reads its pull request).
fn wait_loop(tx: &Sender<Msg>, id: u64, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(180);
    let mut last = Value::Null;
    while Instant::now() < end {
        if let Ok(v) = ask(tx, Request::Loop { id }) {
            if pred(&v) {
                return v;
            }
            last = v;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "timed out waiting for {what}: stage {}, target {}, offer {}, publish {}, needs {}",
        last["stage"], last["target"], last["offer"], last["publish"], last["needs"]
    );
}

fn set_pr(state: &Path, change: impl FnOnce(&mut Value)) {
    let mut s = host_state(state);
    change(&mut s["prs"][0]);
    set_host_state(state, &s);
}

/// A commit on main on the remote only: made on a branch of the repository, pushed to the
/// remote's main, the branch then removed (the engineer's own main is not moved).
fn remote_commit(repo: &Path, path: &str, text: &str, msg: &str) -> String {
    // On top of the remote's main (the test plays someone else on the remote).
    git(repo, &["fetch", "-q", "origin", "main"]);
    git(repo, &["checkout", "-q", "-b", "remote-side", "FETCH_HEAD"]);
    let file = repo.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", msg]);
    let sha = git(repo, &["rev-parse", "HEAD"]);
    git(repo, &["push", "-q", "origin", "remote-side:main"]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["branch", "-q", "-D", "remote-side"]);
    sha
}

/// What the engineer's repository says about the remote's target: its tracking ref and
/// FETCH_HEAD (a push updates the tracking ref of the branch pushed, as 010's push does).
fn refs_of(repo: &Path) -> (String, String) {
    (
        git(repo, &["for-each-ref", "refs/remotes/origin/main"]),
        std::fs::read_to_string(repo.join(".git").join("FETCH_HEAD")).unwrap_or_default(),
    )
}

/// 001-calc locked, built, published (a protected main); `after` are the scenario's steps for
/// what follows.
fn published(
    name: &str,
    after: &str,
    level: Option<fallow::run::events::Level>,
    host: Value,
) -> (fallow::service::ToolEngine, std::path::PathBuf, u64) {
    let root = scratch(name);
    let (e, repo, id) = locked_calc_prepared(
        &root,
        &format!("{}{after}", calc_loop_ok()),
        None,
        level,
        |root: &Path, repo: &Path| {
            bare_remote(root, repo, ADDRESS);
            fake_host(root, host);
        },
    );
    let tx = &e.tx;
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    wait_loop(tx, id, "the remote read", |v| {
        v["publish"]["remote"]["checking"] != true && v["publish"]["remote"]["state"] != "checking"
    });
    ask(
        tx,
        Request::Publish {
            id,
            without_latest_check: false,
        },
    )
    .unwrap();
    wait_loop(tx, id, "publishing", |v| v["stage"] == "publishing");
    let _ = repo;
    (e, root, id)
}

fn host() -> Value {
    json!({ "login": "octo", "protected": { "main": true } })
}

/// The repair of the conflict and both stories checked again.
fn repaired() -> String {
    format!(
        "{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true),
        check_step("US1", true, ""),
        check_step("US2", true, "")
    )
}

/// FR-011, FR-012 (Q5): a conflicting pull request is shown, "Bring up to date" is offered
/// (never "stop"), the update fetches the remote target by sha without writing a ref, resolves,
/// verifies again and pushes the same branch forward; the pull request, its number and the
/// target are unchanged otherwise.
#[test]
fn a_conflicting_pull_request_is_brought_up_to_date_and_pushed_forward() {
    let (e, root, id) = published("pu-conflict", &repaired(), None, host());
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    let published_at = remote_head(&root, BRANCH).unwrap();
    let remote_main = remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    let v = wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    assert_eq!(v["target"]["remote"], true);
    assert_eq!(
        v["offer"],
        json!(["bring_up_to_date"]),
        "never stop: {}",
        v["offer"]
    );
    // Normal asks under "Needs you", never offering "stop".
    let v = wait_loop(tx, id, "the question", |v| v["needs"].is_object());
    assert!(
        !v["needs"]["options"].to_string().contains("stop"),
        "{}",
        v["needs"]
    );
    let refs_before = refs_of(&repo);
    let local_main = git(&repo, &["rev-parse", "main"]);
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "pushed forward", |v| {
        v["update"].is_null() && v["updates"] == 1 && v["stage"] == "publishing"
    });
    let now = remote_head(&root, BRANCH).unwrap();
    assert_ne!(now, published_at, "the branch moved forward");
    assert!(
        git(&repo, &["merge-base", "--is-ancestor", &published_at, &now]).is_empty(),
        "forward, never forced"
    );
    assert!(
        git(&repo, &["merge-base", "--is-ancestor", &remote_main, &now]).is_empty(),
        "the remote target is underneath"
    );
    assert_eq!(v["publish"]["publication"]["pr_number"], 1);
    assert_eq!(v["publish"]["publication"]["commit"], now);
    assert!(
        remote_head(&root, &format!("{BRANCH}-2")).is_none(),
        "no second branch"
    );
    assert_eq!(
        remote_head(&root, "main").unwrap(),
        remote_main,
        "the target untouched"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "main"]),
        local_main,
        "the engineer's main untouched"
    );
    assert_eq!(
        refs_of(&repo),
        refs_before,
        "no ref of the repository written"
    );
    assert_eq!(
        host_state(&state)["prs"].as_array().unwrap().len(),
        1,
        "never a second pull request"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-012: someone else pushed to the loop's branch: nothing is pushed, it says so, no fallback.
#[test]
fn a_foreign_commit_on_the_branch_stops_the_push() {
    let (e, root, id) = published("pu-foreign", &repaired(), None, host());
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    // A colleague's commit on the loop's branch.
    git(
        &repo,
        &[
            "checkout",
            "-q",
            "-b",
            "colleague",
            &remote_head(&root, BRANCH).unwrap(),
        ],
    );
    std::fs::write(repo.join("NOTE.md"), "mine\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "a colleague's commit"]);
    let foreign = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["push", "-q", "origin", &format!("colleague:{BRANCH}")],
    );
    git(&repo, &["checkout", "-q", "main"]);
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "the refusal", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("someone else pushed"))
    });
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        foreign,
        "nothing pushed over it"
    );
    assert!(
        remote_head(&root, &format!("{BRANCH}-2")).is_none(),
        "no fallback branch"
    );
    assert_eq!(v["stage"], "publishing");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-011: behind a target that does not require it: shown, not urged; required: offered.
#[test]
fn behind_is_shown_and_offered_only_when_required() {
    let (e, root, id) = published(
        "pu-behind",
        "",
        None,
        json!({ "login": "octo", "protected": { "main": true }, "strict": { "main": false } }),
    );
    let tx = &e.tx;
    let state = root.join("host.json");
    set_pr(&state, |pr| pr["merge_state"] = json!("BEHIND"));
    let v = wait_loop(tx, id, "behind shown", |v| {
        v["target"]["pr"]["behind"] == true
    });
    assert_eq!(v["target"]["pr"]["required"], false);
    assert_eq!(v["offer"], json!([]), "not urged");
    std::thread::sleep(Duration::from_secs(2));
    assert!(wait_loop(tx, id, "no question", |_| true)["needs"].is_null());
    let mut s = host_state(&state);
    s["strict"]["main"] = json!(true);
    set_host_state(&state, &s);
    let v = wait_loop(tx, id, "required", |v| {
        v["target"]["pr"]["required"] == true
    });
    assert!(
        v["offer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "bring_up_to_date")
            || v["needs"].is_object(),
        "{}",
        v["offer"]
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// The pull request merged on the remote during an update: the update stops, nothing is
/// pushed, the loop ends as merged (010).
#[test]
fn a_pull_request_merged_during_an_update_ends_it() {
    // The repair is slowed so the merge on the remote comes during it.
    let slow = conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true).replacen(
        "[[step]]\n",
        "[[step]]\nsleep_ms = 20000\n",
        1,
    );
    let (e, root, id) = published("pu-merged", &slow, None, host());
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    wait_loop(tx, id, "the update running", |v| !v["update"].is_null());
    set_pr(&state, |pr| {
        pr["state"] = json!("merged");
        pr["merged_at"] = json!("2026-11-03T12:00:00Z");
    });
    let v = wait_loop(tx, id, "merged", |v| v["stage"] == "done");
    assert!(v["update"].is_null());
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "nothing pushed"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-011a: a first publication when the remote target moved: nothing pushed; the loop asks
/// (Normal), is brought up to date against the remote target, then published.
#[test]
fn a_first_publication_checks_the_remote_target_first() {
    let root = scratch("pu-first");
    let after = format!(
        "{}{}",
        check_step("US1", true, ""),
        check_step("US2", true, "")
    );
    let (e, repo, id) = locked_calc_prepared(
        &root,
        &format!("{}{after}", calc_loop_ok()),
        None,
        None,
        |root: &Path, repo: &Path| {
            bare_remote(root, repo, ADDRESS);
            fake_host(root, host());
        },
    );
    let tx = &e.tx;
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    wait_loop(tx, id, "the remote read", |v| {
        v["publish"]["remote"]["checking"] != true && v["publish"]["remote"]["state"] != "checking"
    });
    // A change on the remote's main that does not conflict.
    let remote_main = remote_commit(&repo, "NOTES.md", "remote\n", "a note on the remote");
    ask(
        tx,
        Request::Publish {
            id,
            without_latest_check: false,
        },
    )
    .unwrap();
    // Nothing pushed; at Normal it asks before the update (FR-010).
    wait_loop(tx, id, "the question", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("nothing was pushed"))
    });
    assert!(remote_head(&root, BRANCH).is_none(), "nothing pushed");
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "published after its update", |v| {
        v["stage"] == "publishing"
    });
    assert_eq!(v["updates"], 1, "brought up to date first");
    let pushed = remote_head(&root, BRANCH).unwrap();
    assert!(
        git(
            &repo,
            &["merge-base", "--is-ancestor", &remote_main, &pushed]
        )
        .is_empty(),
        "the remote target is underneath what was pushed"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007, FR-009: with `update_limit = 1`, the remote target moving again during the update
/// stops the push; at the limit the pull request may be left as it is, recorded with the target's
/// commits it was not verified with, and nothing is pushed.
#[test]
fn at_the_limit_the_pull_request_may_be_left_as_it_is() {
    // The update's first check is slowed so the remote moves again before the push.
    let after = format!(
        "{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true),
        check_step("US1", true, "sleep_ms = 8000"),
        check_step("US2", true, "")
    );
    let root = scratch("pu-limit");
    let repo = repo(&root);
    bare_remote(&root, &repo, ADDRESS);
    let state = fake_host(&root, host());
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}{after}", lead_writes_calc(), calc_loop_ok()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, fallow::studio::Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    git(&repo, &["push", "-q", "origin", "main"]);
    let id = lock_feature_with(
        tx,
        "repo",
        "Calc",
        "001-calc",
        None,
        Some((5.0, 60)),
        None,
        LockExtra {
            update_limit: Some(1),
            run_after: None,
        },
    );
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    wait_loop(tx, id, "the remote read", |v| {
        v["publish"]["remote"]["checking"] != true && v["publish"]["remote"]["state"] != "checking"
    });
    ask(
        tx,
        Request::Publish {
            id,
            without_latest_check: false,
        },
    )
    .unwrap();
    wait_loop(tx, id, "publishing", |v| v["stage"] == "publishing");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    wait_loop(tx, id, "the update merged", |v| {
        v["update"]["merged"] == true
    });
    let late = remote_commit(
        &repo,
        "NOTES.md",
        "later
",
        "another change on the remote",
    );
    let v = wait_loop(tx, id, "the limit", |v| {
        v["needs"]["options"]
            .to_string()
            .contains("leave the pull request as it is")
    });
    assert_eq!(v["overtaken"], 1);
    assert!(
        !v["needs"]["options"].to_string().contains("stop"),
        "{}",
        v["needs"]
    );
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "nothing pushed"
    );
    ask(tx, Request::LeavePr { id }).unwrap();
    let r = wait_for(tx, id, "left as it is", |v| {
        v["feature_loop"]["update"].is_null() && v["feature_loop"]["unchecked"].is_array()
    });
    let unchecked = r["feature_loop"]["unchecked"].to_string();
    assert!(unchecked.contains(&late), "{unchecked}");
    assert_eq!(r["stage"], "publishing");
    assert_eq!(remote_head(&root, BRANCH).unwrap(), published_at);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T031, FR-016b: the remote target changed the feature's documents while the pull request is
/// open: nothing pushed, neither "lock again" nor "stop" offered, the page says to close the pull
/// request first; once closed on the remote, the loop is ready again (010).
#[test]
fn documents_changed_on_the_remote_while_the_pull_request_is_open() {
    let (e, root, id) = published(
        "pu-docs",
        "",
        None,
        json!({ "login": "octo", "protected": { "main": true }, "strict": { "main": true } }),
    );
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(
        &repo,
        "specs/001-calc/spec.md",
        "# changed on the remote\n",
        "edit the spec",
    );
    set_pr(&state, |pr| pr["merge_state"] = json!("BEHIND"));
    wait_loop(tx, id, "required and behind", |v| {
        v["target"]["pr"]["required"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "held", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("Close the pull request on the remote first"))
    });
    let offered = format!("{} {}", v["offer"], v["needs"]["options"]);
    assert!(
        !offered.contains("lock") && !offered.contains("stop"),
        "{offered}"
    );
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "nothing pushed"
    );
    set_pr(&state, |pr| pr["state"] = json!("closed"));
    let v = wait_loop(tx, id, "ready again", |v| v["stage"] == "ready");
    assert!(v["update"].is_null(), "the update dropped with the close");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007: at the limit of a published loop, "Verify again on the latest <target>" brings it up
/// to date again and pushes forward (phase 6 review 4).
#[test]
fn at_the_limit_verify_again_starts_a_new_update() {
    // The update's first check is slowed so the remote moves again before the push.
    let after = format!(
        "{}{}{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true),
        check_step("US1", true, "sleep_ms = 8000"),
        check_step("US2", true, ""),
        // "Verify again": a clean update, both stories again.
        check_step("US1", true, ""),
        check_step("US2", true, "")
    );
    let root = scratch("pu-again");
    let repo = repo(&root);
    bare_remote(&root, &repo, ADDRESS);
    let state = fake_host(&root, host());
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}{after}", lead_writes_calc(), calc_loop_ok()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    sreq(tx, fallow::studio::Req::AddProject { path: repo.clone() }).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    git(&repo, &["push", "-q", "origin", "main"]);
    let id = lock_feature_with(
        tx,
        "repo",
        "Calc",
        "001-calc",
        None,
        Some((5.0, 60)),
        None,
        LockExtra {
            update_limit: Some(1),
            run_after: None,
        },
    );
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    wait_loop(tx, id, "the remote read", |v| {
        v["publish"]["remote"]["checking"] != true && v["publish"]["remote"]["state"] != "checking"
    });
    ask(
        tx,
        Request::Publish {
            id,
            without_latest_check: false,
        },
    )
    .unwrap();
    wait_loop(tx, id, "publishing", |v| v["stage"] == "publishing");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    wait_loop(tx, id, "the update merged", |v| {
        v["update"]["merged"] == true
    });
    let late = remote_commit(
        &repo,
        "NOTES.md",
        "later
",
        "another change on the remote",
    );
    let v = wait_loop(tx, id, "the limit", |v| {
        v["needs"]["options"]
            .to_string()
            .contains("leave the pull request as it is")
    });
    assert_eq!(v["overtaken"], 1);
    assert!(
        !v["needs"]["options"].to_string().contains("stop"),
        "{}",
        v["needs"]
    );
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "nothing pushed"
    );
    // Review 4: "Verify again" starts another update, the count starting again.
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "pushed forward after verifying again", |v| {
        v["update"].is_null() && v["updates"] == 2 && v["overtaken"] == 0
    });
    assert_ne!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "pushed forward"
    );
    assert!(
        git(
            &repo,
            &[
                "merge-base",
                "--is-ancestor",
                &late,
                &remote_head(&root, BRANCH).unwrap()
            ]
        )
        .is_empty(),
        "the latest target is underneath"
    );
    assert_eq!(v["stage"], "publishing");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-012 (phase 6 review F1): a push forward that failed is said; once the engineer brings the
/// loop up to date again, the verified commit reaches the pull request's branch.
#[test]
fn a_failed_push_forward_is_pushed_once_asked_again() {
    // The update's first check is slowed so the branch can be taken away before the push.
    let after = format!(
        "{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true),
        check_step("US1", true, "sleep_ms = 8000"),
        check_step("US2", true, "")
    );
    let (e, root, id) = published("pu-pushfail", &after, None, host());
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    wait_loop(tx, id, "the update merged", |v| {
        v["update"]["merged"] == true
    });
    // The branch is gone on the remote for a moment: the push forward fails, nothing pushed.
    git(
        &repo,
        &["push", "-q", "origin", &format!(":refs/heads/{BRANCH}")],
    );
    wait_loop(tx, id, "the failure said", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("Nothing was pushed"))
    });
    git(
        &repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{published_at}:refs/heads/{BRANCH}"),
        ],
    );
    ask(tx, Request::Update { id }).unwrap();
    let v = wait_loop(tx, id, "pushed forward", |v| {
        v["publish"]["publication"]["commit"] != published_at.as_str()
    });
    let now = remote_head(&root, BRANCH).unwrap();
    assert_eq!(v["publish"]["publication"]["commit"], now);
    assert!(
        git(&repo, &["merge-base", "--is-ancestor", &published_at, &now]).is_empty(),
        "forward from what Fallow pushed"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// FR-007, FR-010 (phase 6 review N1): a move of the remote target found by a later push of the
/// verified commit (its update gone) is a fresh move: at Normal it asks, never counted as
/// overtaken, never an update by itself.
#[test]
fn a_move_found_after_a_failed_push_is_asked_about_not_counted() {
    let after = format!(
        "{}{}{}",
        conflict_repair_step(1, "calc/__init__.py", CALC_INIT_SUB, true),
        check_step("US1", true, "sleep_ms = 8000"),
        check_step("US2", true, "")
    );
    let (e, root, id) = published("pu-fresh", &after, None, host());
    let tx = &e.tx;
    let repo = root.join("repo");
    let state = root.join("host.json");
    let published_at = remote_head(&root, BRANCH).unwrap();
    remote_commit(&repo, "calc/__init__.py", REMOTE_INIT, "on the remote");
    set_pr(&state, |pr| pr["mergeable"] = json!("CONFLICTING"));
    wait_loop(tx, id, "the conflict shown", |v| {
        v["target"]["pr"]["conflicting"] == true
    });
    ask(tx, Request::Update { id }).unwrap();
    wait_loop(tx, id, "the update merged", |v| {
        v["update"]["merged"] == true
    });
    git(
        &repo,
        &["push", "-q", "origin", &format!(":refs/heads/{BRANCH}")],
    );
    wait_loop(tx, id, "the failure said", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("Nothing was pushed"))
    });
    git(
        &repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{published_at}:refs/heads/{BRANCH}"),
        ],
    );
    remote_commit(
        &repo,
        "NOTES.md",
        "meanwhile\n",
        "a change while the loop waited",
    );
    // A note answers nothing, but clears the question.
    ask(
        tx,
        Request::Note {
            id,
            text: "looking at it".into(),
            agent: None,
        },
    )
    .unwrap();
    let v = wait_loop(tx, id, "asked about the move", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("moved on the remote"))
    });
    assert_eq!(v["overtaken"], 0, "a move while waiting is no overtaking");
    assert!(v["update"].is_null(), "nothing started at Normal");
    assert_eq!(
        remote_head(&root, BRANCH).unwrap(),
        published_at,
        "nothing pushed"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
