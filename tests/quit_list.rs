//! 014 (FR-009, T007/T020): the work Quit names: a session whose turn is running is listed, by
//! its project and its name; nothing is listed when nothing runs.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;

const SCENARIO: &str = r#"
[[step]]
kind = "assistant_turn"
when = "slow change"
sleep_ms = 20000
output = { reply = "never reached" }
"#;

#[test]
fn quit_names_the_sessions_whose_turn_runs() {
    let root = scratch("quit-list");
    let repo = repo_with_markers(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, SCENARIO).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);

    assert_eq!(ask(tx, Request::Running).unwrap(), serde_json::json!([]));

    let s = start_session(tx, &project, "slow change");
    wait_conv(tx, &s, "the turn running", |v| v["busy"] == true);
    let running = ask(tx, Request::Running).unwrap();
    let list = running.as_array().unwrap();
    assert_eq!(list.len(), 1, "{running}");
    assert_eq!(list[0]["kind"], "session");
    assert_eq!(list[0]["id"], s.as_str());
    assert_eq!(list[0]["name"], "slow change");
    assert!(list[0]["project"].as_str().is_some_and(|p| !p.is_empty()), "{running}");
}
