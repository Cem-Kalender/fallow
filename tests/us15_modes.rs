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
