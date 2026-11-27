//! 017 US2 (T023; FR-010 to FR-012; contracts/web-api.md), replacing 008's field save: an agent
//! is saved as its whole text. A copy changed in one line differs in that line only; a body with
//! `fields` is refused; a new agent starts from the template, which is refused untouched with
//! every missing field and the empty instructions, and nothing is written.

mod common;

use common::*;
use fallow::service::answer::{KeptTheme, PageRequest, PageResponse, answer};
use fallow::studio::Req;
use serde_json::{Value, json};

fn call(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    method: &str,
    url: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let PageResponse { status, body, .. } = answer(
        PageRequest {
            method: method.into(),
            url: url.into(),
            body: body.map(|b| b.to_string().into_bytes()).unwrap_or_default(),
        },
        tx,
        &KeptTheme::default(),
    );
    let v = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, v)
}

#[test]
fn an_agent_is_saved_as_its_whole_text() {
    let root = scratch("agent-text");
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    fallow::defs::install_defaults(&config).unwrap();
    let repo = repo_named(&root, "shop");
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let p = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    let default = std::fs::read_to_string(config.join("agents/reviewer.md")).unwrap();

    // "Save for this project only" with one line changed: the copy differs in that line only,
    // byte for byte (replaces 008 SC-003).
    let changed = replace_found(
        &default,
        "model: claude-sonnet-5-5",
        "model: claude-opus-5-5",
    );
    let (status, v) = call(
        tx,
        "PUT",
        &format!("/api/agents/reviewer?scope=project:{p}"),
        Some(json!({ "text": changed })),
    );
    assert_eq!(status, 200, "{v}");
    let copy = std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap();
    assert_eq!(copy, changed);
    let diff: Vec<(&str, &str)> = default
        .lines()
        .zip(copy.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        diff,
        vec![("model: claude-sonnet-5-5", "model: claude-opus-5-5")]
    );
    assert_eq!(default.lines().count(), copy.lines().count());

    // 008's form save is gone: a body with `fields` is refused and nothing is written.
    let (status, v) = call(
        tx,
        "PUT",
        &format!("/api/agents/reviewer?scope=project:{p}"),
        Some(json!({ "fields": { "model": "claude-sonnet-5-5" } })),
    );
    assert_eq!(status, 409, "{v}");
    assert_eq!(v["error"], "an agent is saved as its whole text", "{v}");
    assert_eq!(
        std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap(),
        copy
    );

    // The template a new agent starts from.
    let (status, v) = call(tx, "GET", "/api/agents/template?name=ui-checker", None);
    assert_eq!(status, 200, "{v}");
    let template = v["value"]["text"].as_str().unwrap().to_string();
    // Only the runtimes set up here: these settings have no `[agents.fake]`, so the test
    // runtime is not offered (017, decided for the engineer).
    let usable = fallow::defs::usable_runtimes(&studio_settings(&scenario));
    assert_eq!(
        usable.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        ["claude"]
    );
    assert_eq!(
        template,
        fallow::defs::agent::template("ui-checker", &usable)
    );
    assert!(template.contains("name: ui-checker"), "{template}");
    assert!(!template.contains("fake"), "{template}");
    for bad in ["name=Bad%20Name", "name=", "name=a/b", "other=x"] {
        let (status, v) = call(tx, "GET", &format!("/api/agents/template?{bad}"), None);
        assert_eq!(status, 409, "{bad}: {v}");
    }

    // Saved untouched: refused with 7 problems at their lines; no file is written.
    let (status, v) = call(
        tx,
        "PUT",
        "/api/agents/ui-checker?scope=user",
        Some(json!({ "text": template })),
    );
    assert_eq!(status, 409, "{v}");
    let why = v["error"].as_str().unwrap();
    let problems: Vec<&str> = why.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(problems.len(), 7, "{why}");
    assert!(
        problems.iter().all(|l| l.starts_with("ui-checker.md:")),
        "{why}"
    );
    assert!(!config.join("agents/ui-checker.md").exists());

    // Filled in from its comments: saved as written, and listed.
    let filled = template
        .replace("description:", "description: Checks the pages")
        .replace("runtime:", "runtime: claude")
        .replace("model:", "model: claude-sonnet-5-5")
        .replace("kind:", "kind: checker")
        .replace("may_run:", "may_run: [gates]")
        .replace("returns:", "returns: verdict")
        + "Open every page and check it.\n";
    let (status, v) = call(
        tx,
        "PUT",
        "/api/agents/ui-checker?scope=user",
        Some(json!({ "text": filled })),
    );
    assert_eq!(status, 200, "{v}\n{filled}");
    assert_eq!(
        std::fs::read_to_string(config.join("agents/ui-checker.md")).unwrap(),
        filled
    );
    let view = sreq(tx, Req::Agents { project: None }).unwrap();
    let a = view["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "ui-checker")
        .unwrap();
    assert_eq!(a["kind"], "checker");
    assert_eq!(a["model"], "claude-sonnet-5-5");

    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
