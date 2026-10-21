//! 003: the hub's operations, driven directly with a fake session table (no processes).

use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions};
use serde_json::{Value, json};

/// Sessions #1 and #2 in project 1 ("api"), #3 in project 2 ("web").
struct Fake(Vec<SessionInfo>);

impl Fake {
    fn new() -> Fake {
        let s = |id, project: u32, name: &str| SessionInfo {
            id,
            project,
            project_name: name.into(),
            profile_id: "claude".into(),
            profile: "Claude Code".into(),
            state: "working".into(),
            running: true,
            idle: false,
        };
        Fake(vec![s(1, 1, "api"), s(2, 1, "api"), s(3, 2, "web")])
    }
}

impl Sessions for Fake {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.0.iter().find(|s| s.id == id).cloned()
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.0.clone()
    }
    fn verify(&self, _: u32, _: &str, _: Peer) -> Result<(), String> {
        Ok(())
    }
}

fn hub() -> Hub {
    Hub::new(OrchestrationSettings::default())
}

fn done(r: Reply) -> Value {
    match r {
        Reply::Done(v) => v,
        other => panic!("expected a result, got {other:?}"),
    }
}

#[test]
fn no_hub_before_the_first_orchestrated_session() {
    // FR-005, SC-007: startup creates no hub, channel or thread.
    let mut s = fallow::app::state::AppState::new(None, Vec::new());
    assert!(s.hub.is_none());
    // A profile without orchestration does not create it either.
    s.ensure_hub(fallow::session::profiles::Style::None);
    assert!(s.hub.is_none());
    // Without an event channel (no running app), nothing is opened.
    s.ensure_hub(fallow::session::profiles::Style::Claude);
    assert!(s.hub.is_none());
    // Turned off in settings: sessions say why.
    s.orch_settings.enabled = false;
    s.ensure_hub(fallow::session::profiles::Style::Claude);
    assert!(s.hub.is_none());
    assert_eq!(
        s.sessions.orch_unavailable.as_deref(),
        Some("disabled in settings")
    );
}

#[test]
fn whoami_and_session_list_name_the_caller() {
    let mut h = hub();
    let f = Fake::new();
    let now = Instant::now();
    let me = done(h.call(&f, 2, "whoami", json!({}), now));
    assert_eq!(me["session"], 2);
    assert_eq!(me["project"], "api");
    assert_eq!(me["profile"], "Claude Code");
    let l = done(h.call(&f, 2, "session_list", json!({}), now));
    let ids: Vec<u64> = l["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1, 2], "the caller's project by default");
    assert_eq!(l["sessions"][1]["self"], true);
    let l = done(h.call(&f, 2, "session_list", json!({"scope": "shared"}), now));
    assert_eq!(l["sessions"].as_array().unwrap().len(), 3);
}

#[test]
fn unknown_tools_and_arguments_are_protocol_errors() {
    let mut h = hub();
    let f = Fake::new();
    let now = Instant::now();
    assert!(matches!(
        h.call(&f, 1, "kill_session", json!({}), now),
        Reply::Rpc(-32602, _)
    ));
    assert!(matches!(
        h.call(&f, 1, "whoami", json!({"session": 3}), now),
        Reply::Rpc(-32602, _)
    ));
    assert!(matches!(
        h.call(&f, 1, "session_list", json!({"scope": 5}), now),
        Reply::Rpc(-32602, _)
    ));
}

#[test]
fn calls_are_rate_limited_per_session() {
    // FR-008: 30 per minute by default.
    let mut h = hub();
    let f = Fake::new();
    let now = Instant::now();
    for _ in 0..30 {
        done(h.call(&f, 1, "whoami", json!({}), now));
    }
    match h.call(&f, 1, "whoami", json!({}), now) {
        Reply::Error("rate_limited", m) => assert!(m.contains("retry after"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(h.notices.len(), 1, "the engineer is told once");
    h.call(&f, 1, "whoami", json!({}), now);
    assert_eq!(h.notices.len(), 1, "…not again within a minute");
    // Another session has its own budget.
    done(h.call(&f, 2, "whoami", json!({}), now));
    // The budget refills.
    done(h.call(&f, 1, "whoami", json!({}), now + Duration::from_secs(3)));
}

#[test]
fn tool_errors_are_mcp_results_with_is_error() {
    let r = Reply::error("too_large", "8 KiB at most");
    let v = r.result().unwrap();
    assert_eq!(v["isError"], true);
    assert_eq!(v["structuredContent"]["error"], "too_large");
    let text: Value = serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["message"], "8 KiB at most");
}
