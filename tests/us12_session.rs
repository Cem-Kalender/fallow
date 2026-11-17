//! 012 US1 (T012; quickstart Q1; FR-001, FR-004, FR-006, FR-012, FR-013, FR-015; research R-2,
//! R-3): a session made from its first message works in a working copy and on a branch of its
//! own; each turn that changed files is one commit by Fallow, which runs no hook and signs
//! nothing; a turn that changed nothing makes no commit; a stopped turn is committed as cut
//! short; the engineer's folder and branch never change.

mod common;

use common::*;
use fallow::studio::Req;

const SCENARIO: &str = r#"
[[step]]
kind = "assistant_turn"
when = "fix the Sunday bug"
say = "Looking at picker.js."
files = [{ path = "picker.js", text = "const firstDay = 1;\n" }]
output = { reply = "Fixed: weeks start on Monday now." }

[[step]]
kind = "assistant_turn"
when = "what does it do"
output = { reply = "It draws the calendar." }

[[step]]
kind = "assistant_turn"
when = "slow change"
files = [{ path = "slow.txt", text = "half done\n" }]
linger_ms = 20000
output = { reply = "never reached" }

[[step]]
kind = "assistant_turn"
when = "fix the Sunday bug"
output = { reply = "Second session." }
"#;

#[test]
fn a_session_commits_each_turn_on_its_own_branch_and_never_touches_the_folder() {
    let root = scratch("us12-session");
    let repo = repo_with_markers(&root);
    std::fs::write(repo.join("picker.js"), "const firstDay = 0;\n").unwrap();
    std::fs::write(
        repo.join(".gitattributes"),
        "*.psd filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "picker"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, SCENARIO).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    // The engineer has an uncommitted change, and asks for signed commits.
    std::fs::write(repo.join("notes.txt"), "mine\n").unwrap();
    git(&repo, &["config", "commit.gpgsign", "true"]);
    let _ = std::fs::remove_file(root.join("hooks.log"));
    let folder_head = git(&repo, &["rev-parse", "HEAD"]);
    let folder_status = git(&repo, &["status", "--porcelain"]);

    // The first message makes the session and starts its first turn.
    let s = start_session(tx, &project, "fix the Sunday bug");
    let v = wait_turn(tx, &s, 1);
    let sess = &v["session"];
    assert_eq!(sess["branch"], "fallow/solo/fix-the-sunday-bug", "{sess}");
    assert_eq!(sess["name"], "fix the Sunday bug");
    assert_eq!(sess["start"]["branch"], "main");
    assert_eq!(
        sess["start"]["dirty"], true,
        "the folder's own change is not in it"
    );
    assert_eq!(sess["lfs"], true);
    assert_eq!(v["turns"][0]["reply"], "Fixed: weeks start on Monday now.");
    let wt = std::path::PathBuf::from(sess["worktree"].as_str().unwrap());
    assert!(
        !wt.join("notes.txt").exists(),
        "the engineer's uncommitted change is not in it"
    );
    let log = git(&wt, &["log", "--format=%s", "main..HEAD"]);
    assert_eq!(log, format!("Fallow solo {s}, turn 1: fix the Sunday bug"));
    let commit = v["turns"][0]["commit"].as_str().unwrap().to_string();
    let raw = git(&wt, &["cat-file", "-p", &commit]);
    assert!(!raw.contains("gpgsig"), "not signed: {raw}");
    assert!(
        hooks_ran(&root).is_empty(),
        "no hook ran: {:?}",
        hooks_ran(&root)
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("picker.js")).unwrap(),
        "const firstDay = 1;\n"
    );
    assert!(
        v["activity"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "write" || a["kind"] == "edit"),
        "the activity shows the edit: {}",
        v["activity"]
    );

    // A turn that changes nothing makes no commit.
    say_solo(tx, &s, "what does it do");
    let v = wait_turn(tx, &s, 2);
    assert!(v["turns"][1]["commit"].is_null(), "{}", v["turns"][1]);
    assert_eq!(git(&wt, &["rev-list", "--count", "main..HEAD"]), "1");

    // A stopped turn: what it changed is committed, marked as cut short.
    say_solo(tx, &s, "slow change");
    wait_conv(tx, &s, "the slow turn writing", |_| {
        wt.join("slow.txt").exists()
    });
    sreq(tx, Req::Cancel { conv: s.clone() }).unwrap();
    let v = wait_turn(tx, &s, 3);
    assert_eq!(v["turns"][2]["cut"], true, "{}", v["turns"][2]);
    let subject = git(&wt, &["log", "-1", "--format=%s"]);
    assert!(subject.ends_with("(cut short)"), "{subject}");
    assert!(git(&wt, &["show", "--name-only", "--format=", "HEAD"]).contains("slow.txt"));

    // The engineer's folder, branch and status are as they were.
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), folder_head);
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo, &["status", "--porcelain"]), folder_status);
    assert!(hooks_ran(&root).is_empty(), "{:?}", hooks_ran(&root));

    // A second session with the same first words gets another branch name (FR-015).
    let s2 = start_session(tx, &project, "fix the Sunday bug");
    let v = wait_turn(tx, &s2, 1);
    assert_eq!(v["session"]["branch"], "fallow/solo/fix-the-sunday-bug-2");
}

#[test]
fn the_assistant_gets_its_rules_keeps_its_own_commits_and_a_detached_start_is_said() {
    let root = scratch("us12-rules");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    // Matched only when Fallow's standing rules reached the assistant (research R-7).
    std::fs::write(
        &scenario,
        r#"
[[step]]
kind = "assistant_turn"
system = "one squashed commit"
files = [{ path = "a.txt", text = "committed by the assistant\n" }]
commit = "assistant: a"
output = { reply = "I committed a.txt myself." }
"#,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    git(&repo, &["checkout", "-q", "--detach"]);
    let s = start_session(tx, &project, "make a");
    let v = wait_turn(tx, &s, 1);
    assert_eq!(v["turns"][0]["reply"], "I committed a.txt myself.", "{v}");
    assert!(
        v["session"]["start"]["branch"].is_null(),
        "started from a commit: {}",
        v["session"]
    );
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    // The assistant's own commit stays; Fallow had nothing left to commit.
    assert_eq!(git(&wt, &["log", "-1", "--format=%s"]), "assistant: a");
    let commits = v["session"]["commits"]["1"]["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 1, "{}", v["session"]["commits"]);
}

#[test]
fn an_assistant_whose_runtime_is_missing_is_refused_with_the_reason() {
    let root = scratch("us12-missing");
    let repo = repo(&root);
    assistant_on_fake(&repo);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    // No `[agents.fake]` profile: the assistant's runtime is not configured.
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    let err = sreq(
        tx,
        Req::NewSession {
            project,
            agent: None,
            text: "hello".into(),
        },
    )
    .unwrap_err();
    assert!(err.contains("the assistant cannot start"), "{err}");
    assert!(err.contains("fake"), "names the runtime: {err}");
}

#[test]
fn the_gates_run_unasked_and_a_branch_deletion_is_asked_with_its_reason() {
    use fallow::conversation::events::ApprovalAnswer;
    let root = scratch("us12-ask");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    // The gate (`PYTHON -m unittest`, from the repository's squad file) and then a deletion of
    // the engineer's branch, both through Claude's permission rules as the fake plays them.
    std::fs::write(
        &scenario,
        format!(
            r#"
[[step]]
kind = "assistant_turn"
exec = ["{PYTHON} -m unittest", "git branch -D main"]
output = {{ reply = "done" }}
"#
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "run the tests then tidy up");
    let w = wait_conv(tx, &s, "a command waiting", |v| {
        !v["pending_approval"].is_null()
    });
    assert_eq!(
        w["pending_approval"]["command"], "git branch -D main",
        "{w}"
    );
    let asked = &w["turns"][0]["approvals"][0];
    assert_eq!(
        asked["command"], "git branch -D main",
        "the gate was not asked: {}",
        w["turns"][0]
    );
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    // "Always" is refused for a git prefix in a session (R-6).
    let refused = sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id: id.clone(),
            answer: ApprovalAnswer::Always,
            prefix: Some("git branch".into()),
        },
    )
    .unwrap_err();
    assert!(!refused.is_empty());
    sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id,
            answer: ApprovalAnswer::Deny,
            prefix: None,
        },
    )
    .unwrap();
    let v = wait_turn(tx, &s, 1);
    assert_eq!(
        v["turns"][0]["approvals"][0]["answer"], "deny",
        "{}",
        v["turns"][0]
    );
    assert!(
        git(&repo, &["branch", "--list", "main"]).contains("main"),
        "main is still there"
    );
}

// 014 (T005): the HTTP transport is in the `automation` build only on Windows.
#[cfg(any(not(windows), feature = "automation"))]
/// 012 T018: the page check's demo. Not a test: run it by hand with `FALLOW_VISUAL_ROOT=<empty
/// folder> cargo test --test us12_session serve_the_012_demo -- --ignored --nocapture`; it writes
/// the page's address to `<root>/url.txt` and the projects to `<root>/projects.txt`, and serves
/// for `FALLOW_VISUAL_SECS` seconds (default 1800). `shop` has a session with a finished turn and
/// a command waiting for the engineer; `notes` has none (the draft).
#[test]
#[ignore]
fn serve_the_012_demo() {
    let root =
        std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let shop = repo_named(&root, "shop");
    let notes = repo_named(&root, "notes");
    std::fs::write(shop.join("picker.js"), "const firstDay = 0;\n").unwrap();
    git(&shop, &["add", "."]);
    git(&shop, &["commit", "-q", "-m", "picker"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r#"
[[step]]
kind = "assistant_turn"
when = "wrong week on Sundays"
say = "Looking at picker.js."
files = [{ path = "picker.js", text = "const firstDay = 1; // weeks start on Monday\n" }]
output = { reply = "Fixed: the picker counted weeks from Sunday. Weeks start on **Monday** now, in `picker.js`." }

[[step]]
kind = "assistant_turn"
when = "Sunday test"
files = [{ path = "picker_test.js", text = "test('sunday', () => {});\n" }]
output = { reply = "Added a test." }

[[step]]
kind = "assistant_turn"
when = "tidy"
exec = ["git branch -D main"]
output = { reply = "Tidied up." }
"#,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, Some(std::time::Duration::from_secs(secs)));
    let tx = &e.tx;
    let mut ids = Vec::new();
    for r in [&shop, &notes] {
        let v = sreq(tx, Req::AddProject { path: r.clone() }).unwrap();
        git(r, &["add", "."]);
        git(r, &["commit", "-q", "-m", "spec kit"]);
        ids.push(v["project"].as_str().unwrap().to_string());
    }
    std::fs::write(shop.join("notes.txt"), "mine\n").unwrap();
    let s = start_session(
        tx,
        &ids[0],
        "the date picker shows the wrong week on Sundays, fix it",
    );
    wait_turn(tx, &s, 1);
    // A second session, finished, for the dropdown.
    let s2 = start_session(tx, &ids[0], "add a Sunday test for the picker");
    wait_turn(tx, &s2, 1);
    sreq(tx, Req::OpenSession { session: s.clone() }).unwrap();
    say_solo(tx, &s, "now tidy up the old branches");
    wait_conv(tx, &s, "a command waiting", |v| {
        !v["pending_approval"].is_null()
    });
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), ids.join("\n")).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}

#[test]
fn the_projects_assistant_must_be_an_assistant() {
    let root = scratch("us12-default");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    let patch = |v: serde_json::Value| {
        sreq(
            tx,
            Req::PatchProject {
                id: project.clone(),
                patch: v,
            },
        )
    };
    let e = patch(serde_json::json!({ "assistant": "lead" })).unwrap_err();
    assert!(e.contains("lead is not an assistant"), "{e}");
    patch(serde_json::json!({ "assistant": "assistant" })).unwrap();
}

#[test]
fn a_restart_during_a_stop_commits_the_turn_as_cut_short_and_messages_keep_their_order() {
    let root = scratch("us12-restart");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r#"
[[step]]
kind = "assistant_turn"
when = "slow change"
files = [{ path = "slow.txt", text = "half done\n" }]
linger_ms = 60000
output = { reply = "never reached" }
"#,
    )
    .unwrap();
    let wt;
    let s;
    {
        let e = start_studio(&root, &scenario, None);
        let tx = &e.tx;
        let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
        let project = added["project"].as_str().unwrap().to_string();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "spec kit"]);
        s = start_session(tx, &project, "slow change");
        let v = wait_conv(tx, &s, "the turn writing", |v| {
            v["session"]["worktree"]
                .as_str()
                .is_some_and(|w| std::path::Path::new(w).join("slow.txt").exists())
        });
        wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
        // Messages typed during the turn wait, in the order typed (FR-011).
        assert!(say_solo(tx, &s, "first after").get("queued").is_some());
        assert!(say_solo(tx, &s, "second after").get("queued").is_some());
        let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
        let q: Vec<&str> = v["session"]["queue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["text"].as_str().unwrap())
            .collect();
        assert_eq!(q, ["first after", "second after"]);
        // Stop, and Fallow goes down before the turn's end comes back.
        sreq(tx, Req::Cancel { conv: s.clone() }).unwrap();
        drop(e);
    }
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
    let t = &v["turns"][0];
    assert_eq!(t["cut"], true, "{t}");
    assert_eq!(
        t["withdrawn"], false,
        "a session's turn is never withdrawn: {t}"
    );
    assert!(git(&wt, &["log", "-1", "--format=%s"]).ends_with("(cut short)"));
    assert!(
        git(&wt, &["show", "--name-only", "--format=", "HEAD"]).contains("slow.txt"),
        "the work is kept"
    );
    assert_eq!(
        v["session"]["queue"].as_array().unwrap().len(),
        2,
        "after a restart the messages wait"
    );
}

#[test]
fn after_a_stop_with_a_message_waiting_saying_again_sends_them_in_order() {
    let root = scratch("us12-go");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r#"
[[step]]
kind = "assistant_turn"
when = "slow change"
files = [{ path = "slow.txt", text = "half done\n" }]
linger_ms = 60000
output = { reply = "never reached" }

[[step]]
kind = "assistant_turn"
when = "first after"
output = { reply = "first done" }

[[step]]
kind = "assistant_turn"
when = "second after"
output = { reply = "second done" }
"#,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "slow change");
    wait_conv(tx, &s, "the turn working", |v| v["busy"] == true);
    assert!(say_solo(tx, &s, "first after").get("queued").is_some());
    sreq(tx, Req::Cancel { conv: s.clone() }).unwrap();
    let v = wait_turn(tx, &s, 1);
    assert_eq!(v["turns"][0]["cut"], true);
    // A stopped turn holds the waiting message (R-9)…
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
    assert_eq!(
        v["turns"].as_array().unwrap().len(),
        1,
        "held after the stop: {}",
        v["session"]["queue"]
    );
    // …until the engineer says something: the waiting one goes first, this one after it.
    say_solo(tx, &s, "second after");
    let v = wait_turn(tx, &s, 3);
    let said: Vec<&str> = v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["said"].as_str().unwrap())
        .collect();
    assert_eq!(said, ["slow change", "first after", "second after"]);
    assert_eq!(v["turns"][2]["reply"], "second done");
    assert!(v["session"]["queue"].as_array().unwrap().is_empty());
}
