//! 012 US5 (T043; quickstart Q6; FR-009 to FR-012, FR-026, FR-027; research R-8, R-9): what a
//! coding assistant needs. A screenshot goes with a message (checked by its magic bytes, up to
//! 5 MiB, at most 4); a runtime that cannot take images refuses it before anything is sent.
//! Messages typed while the assistant works wait, can be edited and removed, and go in order,
//! their `@` files read when they are sent; after a restart they wait for "Send now". The
//! project's instruction files are handed and listed. The assistant's web lookups show, and web
//! access can be switched off when the runtime can do that.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

/// A small valid-looking png: its magic bytes, then anything.
fn png() -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend_from_slice(b"\0\0\0\rIHDR fake image body");
    b
}

/// A project whose assistant runs on the fake runtime with `caps`, playing `scenario`.
fn setup(
    name: &str,
    scenario: &str,
    caps: &[&str],
) -> (
    fallow::service::ToolEngine,
    std::path::PathBuf,
    String,
    std::path::PathBuf,
) {
    let root = scratch(name);
    let repo = repo(&root);
    std::fs::write(repo.join("AGENTS.md"), "Always answer in French.\n").unwrap();
    std::fs::write(repo.join("notes.txt"), "remember the milk\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "notes"]);
    assistant_on_fake(&repo);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario).unwrap();
    let e = start_studio_settings(&root, studio_settings_caps(&sc, caps));
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project, root)
}

fn attach(tx: &Tx, s: &str, name: &str, bytes: Vec<u8>) -> Result<Value, String> {
    sreq(
        tx,
        Req::SoloAttach {
            session: s.to_string(),
            name: name.to_string(),
            bytes,
        },
    )
}

fn say_with(tx: &Tx, s: &str, text: &str, ids: &[&str]) -> Result<Value, String> {
    sreq(
        tx,
        Req::SoloSay {
            session: s.to_string(),
            text: text.to_string(),
            attachments: ids.iter().map(|i| i.to_string()).collect(),
        },
    )
}

fn queue(
    tx: &Tx,
    s: &str,
    id: &str,
    text: Option<&str>,
    remove: bool,
    send: bool,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::SoloQueue {
            session: s.to_string(),
            id: id.to_string(),
            text: text.map(str::to_string),
            remove,
            send,
        },
    )
}

fn view(tx: &Tx, s: &str) -> Value {
    sreq(tx, Req::Conversation { id: s.to_string() }).unwrap()
}

#[test]
fn a_screenshot_goes_with_the_message_and_what_is_not_an_image_is_refused() {
    let sc = format!(
        "{}{}",
        assistant_step("Bonjour.", "when = \"Always answer in French.\""),
        assistant_step(
            "I see a picker.",
            "when = \"look at this\"\nexpect_images = 1"
        ),
    );
    let (e, _repo, project, _root) = setup("us12-extras-img", &sc, &["--images", "--web-off"]);
    let tx = &e.tx;
    let s = start_session(tx, &project, "hello");
    let v = wait_turn(tx, &s, 1);
    // The instruction files: handed on the first turn, and listed by the session.
    assert_eq!(
        v["turns"][0]["reply"], "Bonjour.",
        "AGENTS.md was handed: {}",
        v["turns"][0]
    );
    let listed = v["session"]["instructions"].as_array().unwrap();
    assert!(
        listed.iter().any(|i| i["path"] == "AGENTS.md"),
        "{}",
        v["session"]
    );

    let a = attach(tx, &s, "picker.png", png()).unwrap();
    assert_eq!(a["media_type"], "image/png", "{a}");
    let id = a["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 64, "a sha-256: {a}");
    // What is not an image by its magic bytes, or too big, is refused.
    let err = attach(tx, &s, "notes.png", b"just text, not an image".to_vec()).unwrap_err();
    assert!(err.contains("png, jpeg, gif or webp"), "{err}");
    let mut big = png();
    big.resize(5 * 1024 * 1024 + 1, 0);
    let err = attach(tx, &s, "big.png", big).unwrap_err();
    assert!(err.contains("5 MiB"), "{err}");
    let err = say_with(tx, &s, "five", &[&id, &id, &id, &id, &id]).unwrap_err();
    assert!(err.contains("4"), "{err}");
    let err = say_with(tx, &s, "unknown", &["0".repeat(64).as_str()]).unwrap_err();
    assert!(err.contains("attach"), "{err}");
    // The file is kept with the session, and served back.
    let f = sreq(
        tx,
        Req::SoloAttachment {
            session: s.clone(),
            sha: id.clone(),
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(f["path"].as_str().unwrap()).unwrap(), png());
    assert_eq!(f["media_type"], "image/png");
    // A file planted beside the kept images is never served: its bytes do not hash to its name.
    let dir = std::path::Path::new(f["path"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let forged = "a".repeat(64);
    std::fs::write(
        dir.join(format!("{forged}.json")),
        format!(r#"{{"sha":"{forged}","name":"x","media_type":"text/html","size":9}}"#),
    )
    .unwrap();
    std::fs::write(
        dir.join(format!("{forged}.png")),
        b"<script>alert(1)</script>",
    )
    .unwrap();
    assert!(
        sreq(
            tx,
            Req::SoloAttachment {
                session: s.clone(),
                sha: forged.clone()
            }
        )
        .is_err()
    );
    assert!(
        say_with(tx, &s, "forged", &[&forged]).is_err(),
        "nor handed to the assistant"
    );

    say_with(tx, &s, "look at this", &[&id]).unwrap();
    let v = wait_turn(tx, &s, 2);
    let t = &v["turns"][1];
    assert_eq!(
        t["reply"], "I see a picker.",
        "the image went with the message: {t}"
    );
    assert_eq!(t["attachments"][0]["sha"], id.as_str(), "{t}");
    assert_eq!(t["attachments"][0]["name"], "picker.png", "{t}");
}

#[test]
fn a_runtime_without_images_refuses_before_sending_and_web_cannot_be_switched_off() {
    let sc = assistant_step("Hi.", "");
    let (e, _repo, project, _root) = setup("us12-extras-noimg", &sc, &[]);
    let tx = &e.tx;
    let s = start_session(tx, &project, "hello");
    wait_turn(tx, &s, 1);
    let err = attach(tx, &s, "picker.png", png()).unwrap_err();
    assert!(err.contains("cannot take images"), "{err}");
    let err = sreq(
        tx,
        Req::SoloWeb {
            session: s.clone(),
            on: false,
        },
    )
    .unwrap_err();
    assert!(err.contains("cannot switch web access off"), "{err}");
    assert_eq!(
        view(tx, &s)["turns"].as_array().unwrap().len(),
        1,
        "nothing sent"
    );
}

#[test]
fn waiting_messages_are_edited_removed_and_sent_in_order_with_their_files_read_then() {
    let sc = format!(
        "{}{}{}",
        assistant_step("Working.", "when = \"slow one\"\nsleep_ms = 4000"),
        assistant_step("It says milk.", "when = \"remember the milk\""),
        assistant_step("Third.", "when = \"third, edited\""),
    );
    let (e, _repo, project, _root) = setup("us12-extras-queue", &sc, &["--images", "--web-off"]);
    let tx = &e.tx;
    let s = start_session(tx, &project, "slow one");
    wait_conv(tx, &s, "the turn at work", |v| v["busy"] == true);
    let q1 = say_solo(tx, &s, "What is in @notes.txt?")["queued"]
        .as_str()
        .unwrap()
        .to_string();
    let q2 = say_solo(tx, &s, "second")["queued"]
        .as_str()
        .unwrap()
        .to_string();
    let q3 = say_solo(tx, &s, "third")["queued"]
        .as_str()
        .unwrap()
        .to_string();
    queue(tx, &s, &q2, None, true, false).unwrap();
    queue(tx, &s, &q3, Some("third, edited"), false, false).unwrap();
    let ids: Vec<String> = view(tx, &s)["session"]["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["What is in @notes.txt?", "third, edited"]);
    let err = queue(tx, &s, &q2, Some("gone"), false, false).unwrap_err();
    assert!(err.contains("no waiting message"), "{err}");
    // Sent in order, each as its own turn; the file is read as the message is sent.
    let v = wait_turn(tx, &s, 3);
    assert_eq!(v["turns"][1]["said"], "What is in @notes.txt?");
    assert_eq!(
        v["turns"][1]["reply"], "It says milk.",
        "the file reached the assistant: {}",
        v["turns"][1]
    );
    assert_eq!(v["turns"][2]["said"], "third, edited");
    assert!(v["session"]["queue"].as_array().unwrap().is_empty());
    let _ = q1;
}

#[test]
fn after_a_restart_waiting_messages_wait_for_send_now() {
    let sc = format!(
        "{}{}",
        assistant_step("Working.", "when = \"slow one\"\nsleep_ms = 3000"),
        assistant_step("Sent now.", "when = \"later please\""),
    );
    let (e, _repo, project, root) = setup("us12-extras-restart", &sc, &["--images", "--web-off"]);
    let s;
    {
        let tx = &e.tx;
        s = start_session(tx, &project, "slow one");
        wait_conv(tx, &s, "the turn at work", |v| v["busy"] == true);
        assert!(say_solo(tx, &s, "later please").get("queued").is_some());
        // Fallow goes down while the turn still works: the message has not been sent.
        sreq(tx, Req::Cancel { conv: s.clone() }).unwrap();
    }
    stop(e);
    let settings = studio_settings_caps(&root.join("scenario.toml"), &["--images", "--web-off"]);
    let e = start_studio_settings(&root, settings);
    let tx = &e.tx;
    let v = view(tx, &s);
    let q = v["session"]["queue"].as_array().unwrap().clone();
    assert_eq!(
        q.len(),
        1,
        "the message survived the restart: {}",
        v["session"]
    );
    std::thread::sleep(std::time::Duration::from_millis(2500));
    assert_eq!(
        view(tx, &s)["turns"].as_array().unwrap().len(),
        1,
        "nothing sent by itself"
    );
    let id = q[0]["id"].as_str().unwrap().to_string();
    queue(tx, &s, &id, None, false, true).unwrap();
    let v = wait_turn(tx, &s, 2);
    assert_eq!(v["turns"][1]["reply"], "Sent now.", "{}", v["turns"][1]);
}

#[test]
fn web_lookups_show_and_web_access_can_be_switched_off() {
    let sc = format!(
        "{}{}",
        assistant_step(
            "Looked it up.",
            "when = \"look it up\"\nweb = [\"https://example.com/docs\"]"
        ),
        assistant_step(
            "Could not.",
            "when = \"again offline\"\nweb = [\"https://example.com/docs\"]"
        ),
    );
    let (e, _repo, project, root) = setup("us12-extras-web", &sc, &["--images", "--web-off"]);
    let tx = &e.tx;
    let s = start_session(tx, &project, "look it up");
    let v = wait_turn(tx, &s, 1);
    assert!(
        v["activity"]
            .to_string()
            .contains("Fetched https://example.com/docs"),
        "{}",
        v["activity"]
    );
    assert_eq!(v["session"]["web"]["on"], true);
    sreq(
        tx,
        Req::SoloWeb {
            session: s.clone(),
            on: false,
        },
    )
    .unwrap();
    assert_eq!(view(tx, &s)["session"]["web"]["on"], false);
    say_solo(tx, &s, "again offline");
    wait_turn(tx, &s, 2);
    // The call carried web off: the fake's fetch was refused as Claude Code refuses it.
    let raw = std::fs::read_to_string(
        root.join("data/conversations")
            .join(&s)
            .join("turns/2.jsonl"),
    )
    .unwrap();
    assert!(raw.contains("No such tool available: WebFetch"), "{raw}");
}

// 014 (T005): the HTTP transport is in the `automation` build only on Windows.
#[cfg(any(not(windows), feature = "automation"))]
/// 012 T047: the page for `checks/solo-extras.png`: a turn that carried a screenshot, the web
/// switch, the instruction files, and a message waiting while a turn works; served for a
/// headless browser (`FALLOW_VISUAL_ROOT`, `FALLOW_VISUAL_SECS`, the image `FALLOW_VISUAL_PNG`).
#[test]
#[ignore]
fn serve_the_012_extras_demo() {
    let root =
        std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    let image = std::env::var("FALLOW_VISUAL_PNG")
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_else(png);
    std::fs::create_dir_all(&root).unwrap();
    let sc = format!(
        "{}{}{}",
        assistant_step(
            "Bonjour ! I read AGENTS.md.",
            "when = \"Always answer in French.\"\nweb = [\"https://developer.mozilla.org/docs/Web/API/Intl/Locale/getWeekInfo\"]"
        ),
        assistant_step(
            "The picker starts the week on Sunday; I see it in your screenshot.",
            "when = \"look at this\"\nexpect_images = 1"
        ),
        assistant_step(
            "Working on it.",
            "when = \"make Monday\"\nsleep_ms = 900000"
        ),
    );
    let repo = repo(&root);
    std::fs::write(repo.join("AGENTS.md"), "Always answer in French.\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "agents"]);
    assistant_on_fake(&repo);
    let scp = root.join("scenario.toml");
    std::fs::write(&scp, sc).unwrap();
    let e = start_studio_settings(
        &root,
        studio_settings_caps(&scp, &["--images", "--web-off"]),
    );
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(
        tx,
        &project,
        "How does the week picker choose its first day?",
    );
    wait_turn(tx, &s, 1);
    let id = attach(tx, &s, "picker.png", image).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    say_with(tx, &s, "look at this", &[&id]).unwrap();
    wait_turn(tx, &s, 2);
    say_solo(tx, &s, "make Monday the first day");
    wait_conv(tx, &s, "the turn at work", |v| v["busy"] == true);
    say_solo(tx, &s, "then add a test for it");
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), &project).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
