//! 012 US3 (T028; quickstart Q3; FR-013, FR-014, FR-019 to FR-022; research R-4, R-5, R-12):
//! the session's changes with their turns, its copy as a version of the Files tab, the
//! engineer's edits committed as theirs, the gates on demand with honest labels, undo back to
//! before a turn, a failed turn commit and its two ways on, the base moving with a merge of the
//! starting branch, an undo finished after a restart, and an editor opened during a turn.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;

/// A project whose gate passes anywhere, with `a.txt` and `c.txt` committed.
fn project_with(root: &std::path::Path, scenario: &str) -> (fallow::service::ToolEngine, std::path::PathBuf, String) {
    let repo = repo(root);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.join("c.txt"), "gone soon\n").unwrap();
    std::fs::write(
        repo.join(".fallow/squad.toml"),
        "[[gate]]\nname = \"version\"\ncommand = \"git --version\"\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "a and c"]);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario).unwrap();
    let e = start_studio(root, &sc, None);
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project)
}

fn view(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, s: &str) -> Value {
    sreq(tx, Req::Conversation { id: s.to_string() }).unwrap()
}

fn file_of<'a>(changes: &'a Value, path: &str) -> &'a Value {
    changes["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("{path} not in {changes}"))
}

#[test]
fn changes_gates_and_undo_back_to_before_a_turn() {
    let root = scratch("us12-check");
    let (e, repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "The engineer undid these turns"
output = { reply = "I know what was undone." }

[[step]]
kind = "assistant_turn"
when = "turn one"
files = [{ path = "b.txt", text = "new\n" }]
output = { reply = "added b" }

[[step]]
kind = "assistant_turn"
when = "turn two"
files = [{ path = "a.txt", text = "two\n" }]
output = { reply = "changed a" }

[[step]]
kind = "assistant_turn"
when = "turn three"
delete = ["c.txt"]
output = { reply = "deleted c" }

[[step]]
kind = "assistant_turn"
when = "turn four"
output = { reply = "nothing" }

[[step]]
kind = "assistant_turn"
when = "turn five"
files = [{ path = "d.txt", text = "five\n" }]
linger_ms = 2500
output = { reply = "added d" }
"#,
    );
    let tx = &e.tx;
    let s = start_session(tx, &project, "turn one");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "turn two");
    wait_turn(tx, &s, 2);
    say_solo(tx, &s, "turn three");
    let v = wait_turn(tx, &s, 3);
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());

    // Changes: each file with the turn that changed it, and the notice.
    let ch = sreq(tx, Req::SoloChanges { session: s.clone() }).unwrap();
    assert_eq!(file_of(&ch, "b.txt")["mark"], "A");
    assert_eq!(file_of(&ch, "b.txt")["turns"], serde_json::json!([1]));
    assert_eq!(file_of(&ch, "a.txt")["turns"], serde_json::json!([2]));
    assert_eq!(file_of(&ch, "c.txt")["mark"], "D");
    assert_eq!(file_of(&ch, "c.txt")["turns"], serde_json::json!([3]));
    assert!(ch["notice"].as_str().unwrap().contains("only this session's working copy"));
    // The Files tab has the session's copy as a version.
    let vs = sreq(tx, Req::FileVersions { project: project.clone() }).unwrap();
    assert!(
        vs["versions"].as_array().unwrap().iter().any(|x| x["id"] == format!("solo:{s}")),
        "{vs}"
    );

    // The engineer's edit is committed as theirs before turn 4.
    std::fs::write(wt.join("a.txt"), "mine\n").unwrap();
    say_solo(tx, &s, "turn four");
    let v = wait_turn(tx, &s, 4);
    let edits = &v["session"]["engineer_edits"];
    assert_eq!(edits[0]["before"], "turn 4", "{edits}");
    assert_eq!(edits[0]["files"], serde_json::json!(["a.txt"]));
    assert!(git(&wt, &["log", "-1", "--format=%s"]).contains("the engineer's edits before turn 4"));
    let ch = sreq(tx, Req::SoloChanges { session: s.clone() }).unwrap();
    let by = file_of(&ch, "a.txt")["by"].as_array().unwrap().clone();
    assert!(by.iter().any(|b| b == "you") && by.iter().any(|b| b == "assistant"), "{by:?}");

    // The gates: not run, then passed on the head with no independent check, then stale.
    assert_eq!(v["session"]["gates"]["label"], "not_run");
    let r = sreq(tx, Req::SoloGates { session: s.clone() }).unwrap();
    assert_eq!(r["running"], true, "{r}");
    let v = wait_conv(tx, &s, "the gates", |v| v["session"]["gates"]["running"] == false && v["session"]["gates"]["label"] != "not_run");
    assert_eq!(v["session"]["gates"]["label"], "passed", "{}", v["session"]["gates"]);
    assert_eq!(v["session"]["gates"]["commit"], v["session"]["head"]);
    say_solo(tx, &s, "turn five");
    wait_conv(tx, &s, "turn five working", |v| v["busy"] == true);
    // Refused while a turn works.
    let err = sreq(tx, Req::SoloGates { session: s.clone() }).unwrap_err();
    assert!(err.contains("Wait until the turn ends"), "{err}");
    let v = wait_turn(tx, &s, 5);
    assert_eq!(v["session"]["gates"]["label"], "stale");

    // Undo turn 2: it names turns 2-5, the engineer's edits and what is not committed yet, and
    // needs the confirmation.
    std::fs::write(wt.join("e.txt"), "not committed\n").unwrap();
    let p = sreq(tx, Req::UndoPreview { session: s.clone(), turn: 2 }).unwrap();
    assert_eq!(p["uncommitted"], serde_json::json!(["e.txt"]), "{p}");
    assert_eq!(p["turns"], serde_json::json!([2, 3, 4, 5]), "{p}");
    assert_eq!(p["edits"][0]["files"], serde_json::json!(["a.txt"]), "{p}");
    assert_eq!(p["needs_confirm"], true);
    let err = sreq(tx, Req::Undo { session: s.clone(), turn: 2, confirmed: false }).unwrap_err();
    assert!(err.contains("confirmation"), "{err}");
    let turn1 = v["turns"][1]["head"].as_str().unwrap().to_string();
    sreq(tx, Req::Undo { session: s.clone(), turn: 2, confirmed: true }).unwrap();
    // The edit not committed was committed as the engineer's first, then undone with the rest.
    let head_before = git(&repo, &["rev-parse", &format!("refs/fallow/solo/{s}/undone/2")]);
    assert_eq!(git(&repo, &["show", &format!("{head_before}:e.txt")]), "not committed");
    assert!(!wt.join("e.txt").exists(), "nothing is left in the working copy");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), turn1, "the branch is at turn 1's commit");
    assert_eq!(std::fs::read_to_string(wt.join("a.txt")).unwrap(), "one\n");
    assert!(wt.join("c.txt").exists());
    assert!(
        git(&repo, &["log", "--format=%s", &head_before]).contains("turn 5"),
        "the undone commits are kept"
    );
    let v = view(tx, &s);
    assert_eq!(v["session"]["undone"], serde_json::json!([2, 3, 4, 5]));
    // The next turn's prompt names what was undone.
    say_solo(tx, &s, "what now");
    let v = wait_turn(tx, &s, 6);
    assert_eq!(v["turns"][5]["reply"], "I know what was undone.", "{}", v["turns"][5]);
}

#[test]
fn a_failed_turn_commit_waits_and_is_committed_again_or_folded_into_the_next() {
    let root = scratch("us12-check-fail");
    let (e, _repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "make x"
files = [{ path = "x.txt", text = "x\n" }]
lock_index = true
output = { reply = "made x" }

[[step]]
kind = "assistant_turn"
when = "after the retry"
output = { reply = "ok" }

[[step]]
kind = "assistant_turn"
when = "make z"
files = [{ path = "z.txt", text = "z\n" }]
lock_index = true
output = { reply = "made z" }

[[step]]
kind = "assistant_turn"
when = "fold it"
files = [{ path = "w.txt", text = "w\n" }]
output = { reply = "folded" }
"#,
    );
    let tx = &e.tx;
    let s = start_session(tx, &project, "make x");
    let v = wait_turn(tx, &s, 1);
    assert_eq!(v["session"]["pending_commit"]["n"], 1, "{}", v["session"]);
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    let lock = {
        let link = std::fs::read_to_string(wt.join(".git")).unwrap();
        std::path::PathBuf::from(link.trim().trim_start_matches("gitdir: ").trim()).join("index.lock")
    };
    // Still locked: the next turn and an undo wait, saying why.
    let err = sreq(tx, Req::Say { conv: s.clone(), text: "after the retry".into() }).unwrap_err();
    assert!(err.contains("could not commit turn 1"), "{err}");
    let err = sreq(tx, Req::Undo { session: s.clone(), turn: 1, confirmed: true }).unwrap_err();
    assert!(err.contains("turn 1"), "{err}");
    // Unlocked, with an edit of the engineer's since: the retry keeps them apart.
    std::fs::remove_file(&lock).unwrap();
    std::fs::write(wt.join("y.txt"), "mine\n").unwrap();
    say_solo(tx, &s, "after the retry");
    let v = wait_turn(tx, &s, 2);
    assert!(v["session"]["pending_commit"].is_null(), "{}", v["session"]);
    let log = git(&wt, &["log", "--format=%s", "-3"]);
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines.iter().any(|l| l.contains("turn 1: committed again")), "{log}");
    assert!(lines.iter().any(|l| l.contains("the engineer's edits")), "{log}");
    assert_eq!(v["session"]["engineer_edits"][0]["files"], serde_json::json!(["y.txt"]));
    let shown = git(&wt, &["show", "--name-only", "--format=", "HEAD~1"]);
    assert!(shown.contains("x.txt") && !shown.contains("y.txt"), "the turn's commit holds only x: {shown}");

    // Another failure, fixed in the next turn: one commit for both turns.
    say_solo(tx, &s, "make z");
    let v = wait_turn(tx, &s, 3);
    assert_eq!(v["session"]["pending_commit"]["n"], 3);
    std::fs::remove_file(&lock).unwrap();
    sreq(
        tx,
        Req::SoloPending { session: s.clone(), choice: "fold_next".into(), confirmed: false },
    )
    .unwrap();
    say_solo(tx, &s, "fold it");
    let v = wait_turn(tx, &s, 4);
    assert!(v["session"]["pending_commit"].is_null(), "{}", v["session"]);
    let subject = git(&wt, &["log", "-1", "--format=%s"]);
    assert!(subject.contains("turns 3-4"), "{subject}");
    let shown = git(&wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(shown.contains("z.txt") && shown.contains("w.txt"), "{shown}");
}

#[test]
fn the_base_moves_with_a_merge_of_the_starting_branch_and_back_on_undo() {
    let root = scratch("us12-check-base");
    let (e, repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "start"
files = [{ path = "s.txt", text = "s\n" }]
output = { reply = "started" }

[[step]]
kind = "assistant_turn"
when = "bring it up to date"
exec = ["git merge -q --no-edit main"]
output = { reply = "merged" }

[[step]]
kind = "assistant_turn"
when = "publish it on main"
exec = ["git commit -q --allow-empty -m mine", "git branch -f main HEAD"]
output = { reply = "main has it" }
"#,
    );
    let tx = &e.tx;
    let s = start_session(tx, &project, "start");
    let v = wait_turn(tx, &s, 1);
    let start = v["session"]["base"]["gates"].as_str().unwrap().to_string();
    // The starting branch moves on.
    std::fs::write(repo.join("m.txt"), "main\n").unwrap();
    git(&repo, &["add", "m.txt"]);
    git(&repo, &["commit", "-q", "-m", "on main"]);
    let main = git(&repo, &["rev-parse", "main"]);
    let v = view(tx, &s);
    assert_eq!(v["session"]["start_moved"]["commits"], 1, "{}", v["session"]);
    say_solo(tx, &s, "bring it up to date");
    let w = wait_conv(tx, &s, "the merge asked", |v| !v["pending_approval"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id,
            answer: fallow::conversation::events::ApprovalAnswer::Once,
            prefix: None,
        },
    )
    .unwrap();
    let v = wait_turn(tx, &s, 2);
    assert_eq!(v["session"]["base"]["gates"], main.as_str(), "{}", v["session"]);
    assert!(v["session"]["start_moved"].is_null(), "{}", v["session"]);
    let p = sreq(tx, Req::UndoPreview { session: s.clone(), turn: 2 }).unwrap();
    assert_eq!(p["base_moves"], true, "{p}");
    sreq(tx, Req::Undo { session: s.clone(), turn: 2, confirmed: false }).unwrap();
    let v = view(tx, &s);
    assert_eq!(v["session"]["base"]["gates"], start.as_str(), "the base is back");
    // The session's own commit put on main moves nothing (R-5: a merge into the starting branch).
    say_solo(tx, &s, "publish it on main");
    let mut asked = 0;
    let v = loop {
        let w = wait_conv(tx, &s, "a command or the end", |v| {
            !v["pending_approval"].is_null() || v["turns"].as_array().is_some_and(|t| t.len() >= 3 && !t[2]["reply"].is_null())
        });
        if w["pending_approval"].is_null() {
            break w;
        }
        asked += 1;
        assert!(asked < 4, "{w}");
        let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
        sreq(
            tx,
            Req::Approve {
                conv: s.clone(),
                id,
                answer: fallow::conversation::events::ApprovalAnswer::Once,
                prefix: None,
            },
        )
        .unwrap();
    };
    assert_eq!(v["session"]["base"]["gates"], start.as_str(), "{}", v["session"]);
}

#[test]
fn an_undo_stopped_halfway_is_finished_on_restart_and_an_editor_during_a_turn_is_said() {
    let root = scratch("us12-check-restart");
    let scenario = r#"
[[step]]
kind = "assistant_turn"
when = "first"
files = [{ path = "f.txt", text = "1\n" }]
output = { reply = "1" }

[[step]]
kind = "assistant_turn"
when = "second"
files = [{ path = "f.txt", text = "2\n" }]
output = { reply = "2" }

[[step]]
kind = "assistant_turn"
when = "slow"
linger_ms = 2500
output = { reply = "3" }
"#;
    let (s, wt, head_after, project);
    {
        let (e, _repo, p) = project_with(&root, scenario);
        project = p;
        let tx = &e.tx;
        s = start_session(tx, &project, "first");
        wait_turn(tx, &s, 1);
        say_solo(tx, &s, "second");
        let v = wait_turn(tx, &s, 2);
        wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
        head_after = v["turns"][1]["head"].as_str().unwrap().to_string();
        drop(e);
    }
    // Fallow stopped right after writing `undo_started`.
    let log = root.join("data").join("conversations").join(&s).join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let last: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    let seq = last["seq"].as_u64().unwrap() + 1;
    let head_before = git(&wt, &["rev-parse", "HEAD"]);
    let line = serde_json::json!({
        "seq": seq, "at": "2026-11-05T12:00:00Z", "type": "undo_started",
        "from": 2, "head_before": head_before, "head_after": head_after,
    });
    std::fs::write(&log, format!("{text}{line}\n")).unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let tx = &e.tx;
    let v = view(tx, &s);
    assert_eq!(v["session"]["undone"], serde_json::json!([2]), "{}", v["session"]);
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head_after);
    assert_eq!(std::fs::read_to_string(wt.join("f.txt")).unwrap(), "1\n");
    assert!(!git(&wt, &["rev-parse", &format!("refs/fallow/solo/{s}/undone/2")]).is_empty());

    // An editor opened on the session's copy during a turn is said.
    say_solo(tx, &s, "slow");
    wait_conv(tx, &s, "the slow turn", |v| v["busy"] == true);
    sreq(
        tx,
        Req::OpenFile {
            project: project.clone(),
            version: format!("solo:{s}"),
            path: ".".into(),
            editor: Some(serde_json::from_value(serde_json::json!({ "program": "git", "args": ["--version"] })).unwrap()),
            command: None,
        },
    )
    .unwrap();
    let v = view(tx, &s);
    assert_eq!(v["session"]["editor_during_turn"], true, "{}", v["session"]);
    let v = wait_turn(tx, &s, 3);
    assert_eq!(v["session"]["editor_during_turn"], false);
}

/// 012 T033: the pages' demo. Not a test: run it by hand with `FALLOW_VISUAL_ROOT=<empty folder>
/// cargo test --test us12_check serve_the_012_check_demo -- --ignored --nocapture`; it writes the
/// page's address to `<root>/url.txt`, then `<project>\n<feature>` to `<root>/projects.txt`, and
/// serves for `FALLOW_VISUAL_SECS` seconds (default 1800). The session has three turns and a
/// gate run that passed; the feature's spec session has its worktree.
#[test]
#[ignore]
fn serve_the_012_check_demo() {
    let root = std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let (e, _repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "Add a picker test"
files = [{ path = "b.txt", text = "new\n" }]
output = { reply = "Added **b.txt** with the test." }

[[step]]
kind = "assistant_turn"
when = "Fix the week"
files = [{ path = "a.txt", text = "two\n" }]
output = { reply = "Changed `a.txt`: weeks start on Monday." }

[[step]]
kind = "assistant_turn"
when = "Remove the old notes"
delete = ["c.txt"]
output = { reply = "Deleted `c.txt`." }
"#,
    );
    let tx = &e.tx;
    let s = start_session(tx, &project, "Add a picker test");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "Fix the week");
    wait_turn(tx, &s, 2);
    say_solo(tx, &s, "Remove the old notes");
    wait_turn(tx, &s, 3);
    sreq(tx, Req::SoloGates { session: s.clone() }).unwrap();
    wait_conv(tx, &s, "the gates", |v| v["session"]["gates"]["label"] == "passed");
    sreq(tx, Req::NewFeature { project: project.clone(), title: "Charts".into(), lead: None }).unwrap();
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), format!("{project}\n001-charts")).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
