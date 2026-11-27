//! 008 US3 (T011; FR-008, SC-003; contracts/agents-api.md): the Agents tab saves only the fields
//! the engineer changed. A project copy of a default differs from it in that line only; comments,
//! the order of the keys and hand-written values stay; a list item with a space reads back as
//! written; other keys are refused; a change that stops a pipeline in use from verifying is
//! refused as in 007.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::{Map, Value, json};

fn fields(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn only_the_changed_fields_are_saved() {
    let root = scratch("agent-fields");
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
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

    // "Save for this project only" on a default: a project copy, the model line changed only.
    sreq(
        tx,
        Req::PutAgentFields {
            name: "reviewer".into(),
            scope: format!("project:{p}"),
            fields: fields(json!({ "model": "claude-opus-5-5" })),
            body: None,
        },
    )
    .unwrap();
    let copy = std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap();
    let changed: Vec<(&str, &str)> = default
        .lines()
        .zip(copy.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        vec![("model: claude-sonnet-5-5", "model: claude-opus-5-5")],
        "{copy}"
    );
    assert_eq!(default.lines().count(), copy.lines().count());

    // A hand-written value and a comment stay; a list item with a space reads back unchanged.
    let hand = replace_found(
        &copy,
        "model: claude-opus-5-5",
        "model: claude-opus-x # pinned by hand",
    );
    std::fs::write(repo.join(".fallow/agents/reviewer.md"), &hand).unwrap();
    sreq(
        tx,
        Req::PutAgentFields {
            name: "reviewer".into(),
            scope: format!("project:{p}"),
            fields: fields(json!({ "may_run": ["gates", "plan.commands", "npm audit"] })),
            body: None,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let now = std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap();
    assert!(
        now.contains("model: claude-opus-x # pinned by hand"),
        "{now}"
    );
    assert!(
        now.contains("may_run: [gates, plan.commands, \"npm audit\"]"),
        "{now}"
    );
    let view = sreq(
        tx,
        Req::Agents {
            project: Some(p.clone()),
        },
    )
    .unwrap();
    let r = view["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "reviewer")
        .unwrap();
    assert_eq!(
        r["may_run"],
        json!(["gates", "plan.commands", "npm audit"]),
        "read back as written"
    );

    // Keys the form may not set are refused, naming them; nothing is written.
    for bad in [json!({ "name": "other" }), json!({ "limits": "1" })] {
        let why = sreq(
            tx,
            Req::PutAgentFields {
                name: "reviewer".into(),
                scope: format!("project:{p}"),
                fields: fields(bad.clone()),
                body: None,
            },
        )
        .unwrap_err();
        let key = bad.as_object().unwrap().keys().next().unwrap().clone();
        assert!(why.contains(&key), "{why}");
    }
    assert_eq!(
        std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap(),
        now
    );

    // A change that stops a pipeline in use from verifying is refused, as in 007 (must fail).
    let why = sreq(
        tx,
        Req::PutAgentFields {
            name: "reviewer".into(),
            scope: "user".into(),
            fields: fields(json!({ "kind": "maker", "returns": "work" })),
            body: None,
        },
    )
    .unwrap_err();
    assert!(why.contains("no longer verify"), "{why}");
    assert_eq!(
        std::fs::read_to_string(config.join("agents/reviewer.md")).unwrap(),
        default,
        "nothing was written"
    );

    // Fields and instructions in one save, in the file's own line endings: a CRLF file stays
    // CRLF, and a value with a quote inside stays plain.
    let crlf = now.replace("\r\n", "\n").replace('\n', "\r\n");
    std::fs::write(repo.join(".fallow/agents/reviewer.md"), &crlf).unwrap();
    sreq(
        tx,
        Req::PutAgentFields {
            name: "reviewer".into(),
            scope: format!("project:{p}"),
            fields: fields(json!({ "description": "Checks the \"happy path\" too" })),
            body: Some("Check every scenario.\nRun something.".into()),
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let after = std::fs::read_to_string(repo.join(".fallow/agents/reviewer.md")).unwrap();
    assert!(
        after.contains("description: Checks the \"happy path\" too\r\n"),
        "{after:?}"
    );
    assert!(
        after.ends_with("---\r\n\r\nCheck every scenario.\r\nRun something.\r\n"),
        "{after:?}"
    );
    assert!(
        !after.replace("\r\n", "").contains('\n'),
        "no mixed line endings"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
