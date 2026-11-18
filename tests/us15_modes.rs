//! 015 US1 (FR-001, FR-002, FR-005, FR-006; research R-6, R-7): the lead's spec session and the
//! assistant's solo session have the same two modes, set through the page's routes; the old
//! names are refused; a new chat starts in the default mode; after a restart every chat is in
//! the default mode and says so when it was in another; an old lead log in `run_anything` opens
//! in the default mode.

mod common;

use common::*;
use fallow::service::answer::{KeptTheme, PageRequest, PageResponse, answer};
use fallow::studio::Req;
use serde_json::{Value, json};

fn post(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    url: &str,
    body: Value,
) -> PageResponse {
    answer(
        PageRequest {
            method: "POST".into(),
            url: url.into(),
            body: body.to_string().into_bytes(),
        },
        tx,
        &KeptTheme::default(),
    )
}

fn body(r: &PageResponse) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}

#[test]
fn one_switch_in_both_chats() {
    let root = scratch("us15-modes");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        "[[step]]\nkind = \"assistant_turn\"\noutput = { reply = \"hi\" }\n",
    )
    .unwrap();
    let (lead, solo, old);
    {
        let e = start_studio(&root, &scenario, None);
        let tx = &e.tx;
        let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
            .as_str()
            .unwrap()
            .to_string();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "spec kit"]);
        lead = sreq(
            tx,
            Req::NewFeature {
                project: project.clone(),
                title: "Modes".into(),
                lead: None,
            },
        )
        .unwrap()["conversation"]
            .as_str()
            .unwrap()
            .to_string();
        solo = start_session(tx, &project, "hello");
        wait_turn(tx, &solo, 1);
        old = sreq(
            tx,
            Req::NewFeature {
                project: project.clone(),
                title: "Old".into(),
                lead: None,
            },
        )
        .unwrap()["conversation"]
            .as_str()
            .unwrap()
            .to_string();
        // Both chats start in the default mode, Manual.
        let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
        assert_eq!(
            (l["mode"].as_str(), l["default_mode"].as_str()),
            (Some("manual"), Some("manual"))
        );
        let s = sreq(tx, Req::Conversation { id: solo.clone() }).unwrap();
        assert_eq!(s["session"]["mode"], "manual", "{}", s["session"]);
        // The same two modes through each chat's route; the old names are refused.
        for (url, old_names) in [
            (
                format!("/api/conversations/{lead}/mode"),
                ["ask", "run_anything", "read_only", "approved"],
            ),
            (
                format!("/api/sessions/{solo}/mode"),
                ["ask", "until_switch", "run_anything", "approved"],
            ),
        ] {
            for o in old_names {
                let r = post(tx, &url, json!({ "mode": o }));
                assert_ne!(r.status, 200, "{url} {o}");
                assert!(
                    String::from_utf8_lossy(&r.body).contains("manual or auto"),
                    "{url} {o}: {}",
                    String::from_utf8_lossy(&r.body)
                );
            }
            let r = post(tx, &url, json!({ "mode": "auto" }));
            assert_eq!(r.status, 200, "{url}: {}", String::from_utf8_lossy(&r.body));
        }
        // The solo route is a conversation too: the lead's route sets a session's mode.
        let r = post(
            tx,
            &format!("/api/conversations/{solo}/mode"),
            json!({ "mode": "auto" }),
        );
        assert_eq!(r.status, 200);
        let _ = body(&r);
        let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
        assert_eq!(l["mode"], "auto");
        assert_eq!(
            l["mode_changes"][0]["mode"], "auto",
            "{}",
            l["mode_changes"]
        );
        assert_eq!(l["mode_changes"][0]["after"], 0, "before any turn");
        // Back to Manual through the route: noted too.
        let r = post(
            tx,
            &format!("/api/conversations/{lead}/mode"),
            json!({ "mode": "manual" }),
        );
        assert_eq!(r.status, 200);
        let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
        assert_eq!(l["mode"], "manual");
        assert_eq!(
            l["mode_changes"][1]["mode"], "manual",
            "{}",
            l["mode_changes"]
        );
        let s = sreq(tx, Req::Conversation { id: solo.clone() }).unwrap();
        assert_eq!(s["session"]["mode"], "auto");
        assert_eq!(s["mode"], "auto");
        assert_eq!(
            s["mode_changes"].as_array().unwrap().len(),
            1,
            "set twice, noted once"
        );
        assert_eq!(s["mode_changes"][0]["after"], 1, "after the first turn");
        drop(e);
    }
    // An old lead conversation, recorded in 007's Run anything before 015: no mode mark.
    let dir = root.join("data").join("conversations").join(&old);
    let _ = std::fs::remove_file(dir.join("mode"));
    let log = dir.join("events.jsonl");
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str(&format!(
        "{}\n",
        json!({ "seq": 99, "at": "2026-11-01T00:00:00Z", "type": "mode_set", "mode": "run_anything" })
    ));
    std::fs::write(&log, text).unwrap();
    // After a restart with Auto as the default: every chat is in Auto; the one that was in
    // Manual says so; the old one opens in the default without a note; a new chat starts in Auto.
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
    assert_eq!(l["mode"], "auto");
    assert!(
        l["mode_note"]
            .as_str()
            .is_some_and(|n| n.contains("Auto, your default")),
        "it was in Manual: {}",
        l["mode_note"]
    );
    let o = sreq(tx, Req::Conversation { id: old.clone() }).unwrap();
    assert_eq!(o["mode"], "auto", "{o}");
    assert_eq!(o["default_mode"], "auto");
    assert!(
        o["mode_note"].is_null(),
        "no mark from before 015: {}",
        o["mode_note"]
    );
    let s = sreq(tx, Req::Conversation { id: solo.clone() }).unwrap();
    assert_eq!(s["session"]["mode"], "auto");
    assert!(s["session"]["mode_note"].is_null(), "it was in Auto");
    let project = l["project"].as_str().unwrap().to_string();
    let n = sreq(
        tx,
        Req::NewFeature {
            project,
            title: "New".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let n = sreq(tx, Req::Conversation { id: n }).unwrap();
    assert_eq!(n["mode"], "auto", "a new chat starts in the default");
    assert!(n["mode_note"].is_null());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 015 (FR-002, R-7): both composers draw the same switch; the old chip is gone.
#[test]
fn the_page_has_one_switch_for_both_chats() {
    let app = include_str!("../src/service/web/app.js");
    let solo = include_str!("../src/service/web/screens/solo.js");
    let project = include_str!("../src/service/web/screens/project.js");
    assert!(app.contains("role=\"radiogroup\" aria-label=\"Mode\""));
    assert!(
        app.contains("${modeSwitch(mode, c && c.default_mode)}"),
        "the lead's composer"
    );
    assert!(
        solo.contains("Fallow.modeSwitch("),
        "the assistant's composer"
    );
    for (name, text) in [("app.js", app), ("solo.js", solo), ("project.js", project)] {
        for gone in [
            "mode-chip",
            "solo-mode",
            "until_switch",
            "Until I switch back",
            "run_anything",
            "mode_reset",
        ] {
            assert!(!text.contains(gone), "{name} still has {gone}");
        }
    }
}

/// 015 US2 (FR-003, FR-008, SC-002, SC-003): in Auto neither agent is asked: the assistant's
/// reads outside its copy, edits, commands Fallow cannot read and a force-push of its own branch
/// all run, as do the lead's command and its edit of a spec document; every action is in the
/// activity. The fake agent asks in Claude Code's own request shape.
#[test]
fn auto_asks_nothing() {
    let root = scratch("us15-auto-nothing");
    let repo = repo(&root);
    let outside = root.join("elsewhere.txt");
    std::fs::write(&outside, "outside\n").unwrap();
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            r#"[[step]]
kind = "assistant_turn"
read = [{outside}]
edit = [{{ path = "src/new.txt", text = "x" }}]
exec = ["python3 -c 'print(40 + 2)'", "bash -c 'echo from-bash'", "echo redirected > out.txt", "X=$HOME; echo $X > home.txt", "git push --force origin HEAD || true"]
output = {{ reply = "done" }}

[[step]]
kind = "lead_turn"
approve = "cargo test --no-run"
edit = [{{ path = "specs/001-modes/notes.md", text = "a note" }}]
output = {{ reply = "$APPROVED", step = "none" }}
"#,
            outside = lit(&outside.display().to_string())
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "do the lot");
    let v = wait_turn(tx, &s, 1);
    let t = &v["turns"][0];
    assert!(t["approvals"].as_array().unwrap().is_empty(), "{t}");
    assert!(t["refusals"].as_array().unwrap().is_empty(), "{t}");
    let activity = v["activity"].as_array().unwrap();
    // Each call has its result, none refused.
    let calls = activity
        .iter()
        .filter(|a| matches!(a["kind"].as_str(), Some("read" | "write" | "run")))
        .count();
    assert_eq!(calls, 7, "{}", v["activity"]);
    assert!(
        !activity.iter().any(|a| a["kind"] == "refused"),
        "{}",
        v["activity"]
    );
    let text = v["activity"].to_string();
    for seen in ["42", "from-bash", "outside"] {
        assert!(text.contains(seen), "{seen}: {text}");
    }
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    assert!(wt.join("out.txt").exists() && wt.join("src/new.txt").exists());

    // The lead, in Auto too: its command is not asked, its spec document is written.
    let lead = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Modes".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "go".into(),
        },
    )
    .unwrap();
    let l = turn_ended(tx, &lead, 1);
    let t = &l["turns"][0];
    assert_eq!(t["reply"], "allow", "{t}");
    assert!(t["approvals"].as_array().unwrap().is_empty(), "{t}");
    assert!(t["refusals"].as_array().unwrap().is_empty(), "{t}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 015 US3 (FR-007): in Manual, each chat's request card offers "Switch to Auto": the request
/// is allowed once, as the engineer's answer, and the chat is in Auto from then on.
#[test]
fn manual_offers_auto() {
    let root = scratch("us15-manual-auto");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r#"[[step]]
kind = "assistant_turn"
exec = ["echo via-the-card"]
output = { reply = "ran" }

[[step]]
kind = "lead_turn"
approve = "python3 -c 1"
output = { reply = "$APPROVED", step = "none" }
"#,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    // The assistant: through the page's route.
    let s = start_session(tx, &project, "run it");
    let w = wait_conv(tx, &s, "a card", |v| !v["pending_approval"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    let r = post(
        tx,
        &format!("/api/sessions/{s}/approve"),
        json!({ "id": id, "answer": "auto" }),
    );
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let v = wait_turn(tx, &s, 1);
    assert_eq!(
        v["turns"][0]["approvals"][0]["answer"], "once",
        "{}",
        v["turns"][0]
    );
    assert!(v["activity"].to_string().contains("via-the-card"));
    assert_eq!(v["session"]["mode"], "auto");
    assert_ne!(
        v["turns"][0]["approvals"][0]["auto"], true,
        "the engineer's own answer"
    );
    assert_eq!(v["mode_changes"][0]["mode"], "auto", "noted");
    // The lead.
    let lead = sreq(
        tx,
        Req::NewFeature {
            project,
            title: "Card".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "go".into(),
        },
    )
    .unwrap();
    let w = wait_conv(tx, &lead, "a card", |v| !v["pending_approval"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::ApproveAuto {
            conv: lead.clone(),
            id,
        },
    )
    .unwrap();
    let l = turn_ended(tx, &lead, 1);
    assert_eq!(l["turns"][0]["reply"], "allow", "{}", l["turns"][0]);
    assert_eq!(l["turns"][0]["approvals"][0]["answer"], "once");
    assert_ne!(l["turns"][0]["approvals"][0]["auto"], true);
    assert_eq!(l["mode"], "auto");
    assert_eq!(l["mode_changes"][0]["mode"], "auto");
    // Both cards offer it.
    for page in [
        include_str!("../src/service/web/app.js"),
        include_str!("../src/service/web/screens/solo.js"),
    ] {
        assert!(page.contains("data-answer=\"auto\"") && page.contains("Switch to Auto"));
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
