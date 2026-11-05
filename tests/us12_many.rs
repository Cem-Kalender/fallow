//! 012 US2 (T019; quickstart Q2; FR-002, FR-004, FR-026, FR-027; research R-13, R-19): several
//! sessions work at once, each on its own branch; the dropdown lists them with their state; a
//! rename shows; with one agent slot a second session's message waits and is sent when the
//! first turn ends; after a restart a session made halfway is forgotten, one whose branch was
//! deleted is marked gone, and a pending approval is cancelled.

mod common;

use common::*;
use fallow::studio::Req;

fn project_with(
    root: &std::path::Path,
    scenario: &str,
) -> (fallow::service::ToolEngine, std::path::PathBuf, String) {
    let repo = repo(root);
    std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "a"]);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario).unwrap();
    let e = start_studio(root, &sc, None);
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project)
}

#[test]
fn two_sessions_on_the_same_file_keep_their_own_changes_and_show_in_the_list() {
    let root = scratch("us12-two");
    // Room for two agents at once, whatever this machine's memory (research S-19).
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]
max_parallel = 4
memory_budget_mb = 100000
",
    )
    .unwrap();
    let (e, repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "make it red"
files = [{ path = "a.txt", text = "one\ntwo\nRED\n" }]
linger_ms = 5000
output = { reply = "red" }

[[step]]
kind = "assistant_turn"
when = "make it blue"
files = [{ path = "a.txt", text = "one\ntwo\nBLUE\n" }]
linger_ms = 5000
output = { reply = "blue" }
"#,
    );
    let tx = &e.tx;
    let red = start_session(tx, &project, "make it red");
    let blue = start_session(tx, &project, "make it blue");
    // Both at work at once (FR-002: a mark while its assistant works).
    let list = wait_conv(tx, &red, "both working", |_| {
        let l = sreq(
            tx,
            Req::Sessions {
                project: project.clone(),
            },
        )
        .unwrap();
        l["open"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["busy"] == true)
            .count()
            == 2
    });
    let _ = list;
    let r = wait_turn(tx, &red, 1);
    let b = wait_turn(tx, &blue, 1);
    let rwt = std::path::PathBuf::from(r["session"]["worktree"].as_str().unwrap());
    let bwt = std::path::PathBuf::from(b["session"]["worktree"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(rwt.join("a.txt")).unwrap(),
        "one\ntwo\nRED\n"
    );
    assert_eq!(
        std::fs::read_to_string(bwt.join("a.txt")).unwrap(),
        "one\ntwo\nBLUE\n"
    );
    assert_eq!(git(&repo, &["show", "main:a.txt"]), "one\ntwo\nthree");
    // Each branch holds only its own change (SC-005).
    assert_eq!(
        git(&repo, &["show", "fallow/solo/make-it-red:a.txt"]),
        "one\ntwo\nRED"
    );
    assert_eq!(
        git(&repo, &["show", "fallow/solo/make-it-blue:a.txt"]),
        "one\ntwo\nBLUE"
    );
    // The list: both, with their changed files; the one made last on top (it was opened last).
    let l = sreq(
        tx,
        Req::Sessions {
            project: project.clone(),
        },
    )
    .unwrap();
    let open = l["open"].as_array().unwrap();
    assert_eq!(open.len(), 2, "{l}");
    assert_eq!(open[0]["id"], blue.as_str());
    assert!(open.iter().all(|i| i["changes"] == 1), "{l}");
    assert!(l["records_bytes"].as_u64().is_some_and(|b| b > 0), "{l}");
    // Opening the other puts it on top; a rename shows.
    sreq(
        tx,
        Req::OpenSession {
            session: red.clone(),
        },
    )
    .unwrap();
    sreq(
        tx,
        Req::RenameSession {
            session: red.clone(),
            name: "Red picker".into(),
        },
    )
    .unwrap();
    let l = sreq(
        tx,
        Req::Sessions {
            project: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(l["open"][0]["id"], red.as_str());
    assert_eq!(l["open"][0]["name"], "Red picker");
    let v = sreq(tx, Req::Conversation { id: red.clone() }).unwrap();
    assert_eq!(
        v["session"]["branch"], "fallow/solo/make-it-red",
        "the branch keeps its name"
    );
}

#[test]
fn with_one_slot_a_second_sessions_message_waits_and_goes_when_it_frees() {
    let root = scratch("us12-slot");
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nmax_parallel = 1\n",
    )
    .unwrap();
    let (e, _repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "first task"
linger_ms = 3000
output = { reply = "first" }

[[step]]
kind = "assistant_turn"
when = "second task"
output = { reply = "second" }
"#,
    );
    let tx = &e.tx;
    let one = start_session(tx, &project, "first task");
    wait_conv(tx, &one, "working", |v| v["busy"] == true);
    let v = sreq(
        tx,
        Req::NewSession {
            project: project.clone(),
            agent: None,
            text: "second task".into(),
        },
    )
    .unwrap();
    let two = v["session"].as_str().unwrap().to_string();
    assert!(v["queued"].is_string(), "no slot: the message waits: {v}");
    let w = sreq(tx, Req::Conversation { id: two.clone() }).unwrap();
    assert!(
        w["session"]["slot_wait"]
            .as_str()
            .is_some_and(|s| s.contains("slot")),
        "and says why: {}",
        w["session"]
    );
    wait_turn(tx, &one, 1);
    let w = wait_turn(tx, &two, 1);
    assert_eq!(
        w["turns"][0]["reply"], "second",
        "sent when the slot freed: {w}"
    );
}

#[test]
fn after_a_restart_an_approval_is_cancelled_a_half_made_session_forgotten_and_a_gone_one_marked() {
    let root = scratch("us12-restart2");
    let scenario = r#"
[[step]]
kind = "assistant_turn"
when = "delete main"
exec = ["git branch -D main"]
output = { reply = "deleted?" }

[[step]]
kind = "assistant_turn"
when = "short one"
output = { reply = "ok" }

[[step]]
kind = "assistant_turn"
when = "third one"
output = { reply = "ok" }

[[step]]
kind = "assistant_turn"
when = "half made"
output = { reply = "ok" }
"#;
    let (s_wait, s_gone, s_half, repo);
    {
        let (e, r, project) = project_with(&root, scenario);
        repo = r;
        let tx = &e.tx;
        s_wait = start_session(tx, &project, "delete main");
        wait_conv(tx, &s_wait, "a command waiting", |v| {
            !v["pending_approval"].is_null()
        });
        sreq(
            tx,
            Req::SessionMode {
                session: s_wait.clone(),
                mode: "until_switch".into(),
            },
        )
        .unwrap();
        s_gone = start_session(tx, &project, "short one");
        wait_turn(tx, &s_gone, 1);
        s_half = start_session(tx, &project, "half made");
        wait_turn(tx, &s_half, 1);
        drop(e);
    }
    // A session Fallow stopped while making it, after its branch got its name (fourth review,
    // 3): its record cut back to "being made".
    let log = root
        .join("data")
        .join("conversations")
        .join(&s_half)
        .join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let upto = text
        .lines()
        .position(|l| l.contains("\"session_creating\""))
        .unwrap();
    let kept: Vec<&str> = text.lines().take(upto + 1).collect();
    std::fs::write(&log, kept.join("\n") + "\n").unwrap();
    assert!(git(&repo, &["branch", "--list", "fallow/solo/half-made"]).contains("half-made"));
    // Outside Fallow: the second session's branch is deleted, its working copy left.
    git(
        &repo,
        &["update-ref", "-d", "refs/heads/fallow/solo/short-one"],
    );
    let sc = root.join("scenario.toml");
    let e = start_studio(&root, &sc, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Conversation { id: s_wait.clone() }).unwrap();
    let t = &v["turns"][0];
    assert_eq!(t["cut"], true, "{t}");
    assert_eq!(t["approvals"][0]["answer"], "deny", "{t}");
    assert_eq!(v["session"]["mode"], "ask", "{}", v["session"]);
    assert_eq!(
        v["session"]["mode_reset"], true,
        "the session says it is asking again"
    );
    assert_eq!(
        v["turns"].as_array().unwrap().len(),
        1,
        "nothing is sent again: {v}"
    );
    assert!(git(&repo, &["branch", "--list", "main"]).contains("main"));
    // The half-made one is forgotten: its record, its working copy, its renamed branch.
    assert!(sreq(tx, Req::Conversation { id: s_half.clone() }).is_err());
    assert!(
        !root
            .join("data")
            .join("conversations")
            .join(&s_half)
            .exists()
    );
    assert_eq!(
        git(&repo, &["branch", "--list", "fallow/solo/half-made"]),
        ""
    );
    assert!(!git(&repo, &["worktree", "list"]).contains(&s_half));
    let g = sreq(tx, Req::Conversation { id: s_gone.clone() }).unwrap();
    assert!(
        g["session"]["gone"]
            .as_str()
            .is_some_and(|s| s.contains("gone")),
        "{}",
        g["session"]
    );
    let refused = sreq(
        tx,
        Req::Say {
            conv: s_gone.clone(),
            text: "again".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("can only be closed"), "{refused}");
    // While Fallow runs: a session whose working copy is removed is gone at its next message.
    let project = g["project"].as_str().unwrap().to_string();
    let l = sreq(
        tx,
        Req::Sessions {
            project: project.clone(),
        },
    )
    .unwrap();
    assert!(
        l["open"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["id"] != s_half.as_str()),
        "{l}"
    );
    let third = start_session(tx, &project, "third one");
    let w = wait_turn(tx, &third, 1);
    let path = w["session"]["worktree"].as_str().unwrap().to_string();
    git(&repo, &["worktree", "remove", "--force", &path]);
    // The list says so before anything is sent (fifth review, 4).
    let l = sreq(
        tx,
        Req::Sessions {
            project: project.clone(),
        },
    )
    .unwrap();
    let item = l["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == third.as_str())
        .cloned()
        .unwrap();
    assert!(
        item["gone"]
            .as_str()
            .is_some_and(|s| s.contains("working copy")),
        "{item}"
    );
    assert_eq!(item["changes"], 0, "{item}");
    let refused = sreq(
        tx,
        Req::Say {
            conv: third.clone(),
            text: "more".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("can only be closed"), "{refused}");
    let w = sreq(tx, Req::Conversation { id: third.clone() }).unwrap();
    assert!(
        w["session"]["gone"]
            .as_str()
            .is_some_and(|s| s.contains("working copy")),
        "{}",
        w["session"]
    );
}
