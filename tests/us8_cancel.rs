//! 008 T047: the engineer cancels the lead's answer. The turn ends withdrawn; what the lead
//! changed for it is undone (its own commit too), the engineer's edits that went with it stay and
//! go with the next message, and the next turn tells the lead the message was withdrawn.

mod common;

use common::*;
use fallow::studio::Req;

#[test]
fn a_cancelled_answer_is_withdrawn_and_undone() {
    let root = scratch("cancel");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "A cart for the shop."
files = [{ path = "specs/001-cart/spec.md", text = "# Cart\n" }]
output = { reply = "I wrote the first spec.", step = "specify" }

[[step]]
kind = "lead_turn"
when = "Make it bigger."
say = "Rewriting the spec."
files = [{ path = "specs/001-cart/spec.md", text = "# Rewritten\n" }, { path = "notes.txt", text = "scratch\n" }]
commit = "the lead commits its rewrite"
linger_ms = 30000
output = { reply = "Never seen.", step = "none" }

[[step]]
kind = "lead_turn"
when = "> Make it bigger."
output = { reply = "Understood, I left it.", step = "none" }
"##,
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
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    let say = |text: &str| {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: text.into(),
            },
        )
    };
    say("A cart for the shop.").unwrap();
    let v = turn_ended(tx, &conv, 1);
    let workdir = std::path::PathBuf::from(v["workdir"].as_str().unwrap());
    let spec = workdir.join("specs/001-cart/spec.md");
    let first = git(&repo, &["rev-parse", "001-cart"]);
    let session = v["session"].clone();

    // Nothing to cancel between turns.
    let idle = sreq(tx, Req::Cancel { conv: conv.clone() });
    assert!(idle.unwrap_err().contains("not answering"));

    // The engineer's edit, not committed until a turn ends.
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
        },
    )
    .unwrap();
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
            text: "# Cart\n\nOnly whole items.\n".into(),
            version: d["version"].as_str().unwrap().into(),
        },
    )
    .unwrap();

    // The lead rewrites and commits; the engineer cancels while it is still at work.
    say("Make it bigger.").unwrap();
    let t0 = std::time::Instant::now();
    while git(&workdir, &["log", "-1", "--format=%s"]) != "the lead commits its rewrite" {
        assert!(t0.elapsed().as_secs() < 30, "the lead never committed");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let out = sreq(tx, Req::Cancel { conv: conv.clone() }).unwrap();
    assert_eq!(out["said"], "Make it bigger.", "{out}");
    let v = turn_ended(tx, &conv, 2);
    // The cancel is in the conversation's log, so a restart before the turn ended undoes it too.
    let log = std::fs::read_to_string(
        root.join("data")
            .join("conversations")
            .join(&conv)
            .join("events.jsonl"),
    )
    .unwrap();
    assert!(log.contains("\"cancel_asked\""), "{log}");
    assert!(
        t0.elapsed().as_secs() < 25,
        "the lead's call was stopped, not waited for"
    );
    let t = &v["turns"][1];
    assert_eq!(t["withdrawn"], true, "{t}");
    assert_eq!(t["reply"], "");
    assert_eq!(t["documents_changed"], serde_json::json!([]));
    assert!(t["commit"].is_null());
    assert_eq!(v["busy"], false);
    // The session stays the one the lead last answered in: a call stopped early may not have
    // saved its own.
    assert_eq!(v["session"], session, "{v}");
    // What the lead did is undone: its commit, its rewrite, its scratch file.
    assert_eq!(git(&repo, &["rev-parse", "001-cart"]), first);
    assert_eq!(git(&workdir, &["rev-parse", "HEAD"]), first);
    assert!(!workdir.join("notes.txt").exists());
    // The engineer's edit is still there, waiting for the next message.
    assert_eq!(
        std::fs::read_to_string(&spec)
            .unwrap()
            .replace("\r\n", "\n"),
        "# Cart\n\nOnly whole items.\n"
    );
    assert_eq!(
        v["pending_edits"],
        serde_json::json!(["specs/001-cart/spec.md"]),
        "{v}"
    );

    // The next turn says the message was withdrawn (the scenario answers only when it sees the
    // quote), carries the edit, and commits it.
    say("Keep it as it is.").unwrap();
    let v = turn_ended(tx, &conv, 3);
    let t = &v["turns"][2];
    assert_eq!(t["reply"], "Understood, I left it.", "{v}");
    assert_eq!(t["edits"][0]["path"], "specs/001-cart/spec.md");
    assert!(git(&repo, &["show", "001-cart:specs/001-cart/spec.md"]).contains("Only whole items."));
}

/// A cancel in the first moments of a conversation, before the lead's session exists, leaves the
/// conversation without one: the next message starts a new session instead of resuming one that
/// was never saved.
#[test]
fn a_quick_cancel_leaves_no_session_behind() {
    let root = scratch("cancel-quick");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "Hello, slowly."
sleep_ms = 30000
output = { reply = "Never seen.", step = "none" }

[[step]]
kind = "lead_turn"
when = "Hello, slowly."
output = { reply = "Hello.", step = "none" }
"##,
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
    let conv = sreq(tx, Req::ProjectConversation { project }).unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let say = |text: &str| {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: text.into(),
            },
        )
    };
    say("Hello, slowly.").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    sreq(tx, Req::Cancel { conv: conv.clone() }).unwrap();
    // A second cancel while the first is under way changes nothing.
    let again = sreq(tx, Req::Cancel { conv: conv.clone() });
    assert!(again.is_ok() || again.unwrap_err().contains("not answering"));
    let v = turn_ended(tx, &conv, 1);
    assert_eq!(v["turns"][0]["withdrawn"], true, "{v}");
    assert!(v["session"].is_null(), "no session recorded: {v}");
    assert_eq!(v["cancelling"], false);
    say("Hello, slowly.").unwrap();
    let v = turn_ended(tx, &conv, 2);
    assert_eq!(v["turns"][1]["reply"], "Hello.", "{v}");
    assert!(v["turns"][1]["failure"].is_null(), "{v}");
}

/// Fallow stops after the engineer cancelled but before the turn ended: when it starts again,
/// the turn is withdrawn and undone as if it had ended then.
#[test]
fn a_cancel_survives_a_restart() {
    use fallow::conversation::events::{ConversationLog, Event, KeptDoc};
    use fallow::run::engine::Msg;

    let root = scratch("cancel-restart");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "A cart for the shop."
files = [{ path = "specs/001-cart/spec.md", text = "# Cart\n" }]
output = { reply = "I wrote the first spec.", step = "specify" }

[[step]]
kind = "lead_turn"
when = "Make it bigger."
files = [{ path = "specs/001-cart/spec.md", text = "# Rewritten\n" }]
commit = "the lead commits its rewrite"
linger_ms = 20000
output = { reply = "Never seen.", step = "none" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let project = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        &e.tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    sreq(
        &e.tx,
        Req::Say {
            conv: conv.clone(),
            text: "A cart for the shop.".into(),
        },
    )
    .unwrap();
    let v = turn_ended(&e.tx, &conv, 1);
    let workdir = std::path::PathBuf::from(v["workdir"].as_str().unwrap());
    let first = git(&repo, &["rev-parse", "001-cart"]);
    sreq(
        &e.tx,
        Req::Say {
            conv: conv.clone(),
            text: "Make it bigger.".into(),
        },
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    while git(&workdir, &["log", "-1", "--format=%s"]) != "the lead commits its rewrite" {
        assert!(t0.elapsed().as_secs() < 20, "the lead never committed");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // Fallow stops; the cancel was recorded just before (as `Cancel` records it).
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    let (mut log, _, _) =
        ConversationLog::open(&root.join("data").join("conversations").join(&conv)).unwrap();
    log.append(Event::CancelAsked {
        n: 2,
        head: Some(first.clone()),
        kept: vec![KeptDoc {
            path: "specs/001-cart/spec.md".into(),
            text: "# Cart\n".into(),
        }],
    })
    .unwrap();
    drop(log);

    let e = start_studio(&root, &scenario, None);
    let v = sreq(&e.tx, Req::Conversation { id: conv.clone() }).unwrap();
    let t = &v["turns"][1];
    assert_eq!(t["withdrawn"], true, "{v}");
    assert!(t["failure"].is_null(), "{t}");
    assert_eq!(v["busy"], false);
    assert_eq!(git(&repo, &["rev-parse", "001-cart"]), first);
    assert_eq!(
        std::fs::read_to_string(workdir.join("specs/001-cart/spec.md"))
            .unwrap()
            .replace("\r\n", "\n"),
        "# Cart\n"
    );
}
