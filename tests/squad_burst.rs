//! 004 T009: coordination bursts (US1, FR-001 to FR-004, SC-001, SC-007).

use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::permissions::Scope;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, squad};
use serde_json::json;

/// #1, #2 in project 1 ("api"); #3 in project 2 ("web").
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
            idle: true,
            role: None,
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

fn request(lead: u32, hours: u64) -> BurstRequest {
    BurstRequest {
        lead,
        duration: Duration::from_secs(hours * 3600),
        allowance: None,
        squad: squad::builtin(),
        start_commit: None,
        target_branch: None,
        has_remote: false,
        spec_kit: false,
        max_sessions: 8,
    }
}

fn hub() -> Hub {
    Hub::new(OrchestrationSettings::default())
}

#[test]
fn a_burst_grants_the_projects_permission_with_the_session_as_lead() {
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    h.start_burst(&f, request(1, 4), now).unwrap();
    let b = h.burst_of(1).unwrap();
    assert_eq!(b.lead, 1);
    assert_eq!(b.allowance, 3, "default allowance");
    assert_eq!(h.role_of(1), Some("lead"));
    let p = &h.permissions.list[0];
    assert_eq!((p.source, p.target), (Scope::Project(1), Scope::Project(1)));
    assert!(p.deliver && p.create_task);
    assert_eq!(p.start_limit, Some(3));
    assert_eq!(p.expires, now + Duration::from_secs(4 * 3600));
    // Hand-offs inside the project need no approval (SC-001).
    let r = h.call(&f, 1, "deliver", json!({"session": 2, "text": "go"}), now);
    assert!(
        matches!(r, Reply::Done(ref v) if v["state"] == "approved"),
        "{r:?}"
    );
    // Another project is untouched.
    let r = h.call(&f, 3, "deliver", json!({"session": 1, "text": "hi"}), now);
    assert!(
        matches!(r, Reply::Done(ref v) if v["state"] == "pending"),
        "{r:?}"
    );
    // The common briefing is on the project board, from Fallow.
    let board = h.call(&f, 2, "board_read", json!({}), now);
    let Reply::Done(v) = board else { panic!() };
    assert_eq!(v["messages"][0]["from"], json!({"kind": "fallow"}));
}

#[test]
fn limits_on_duration_allowance_and_one_burst_per_project() {
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    let e = h.start_burst(&f, request(1, 9), now).unwrap_err();
    assert!(e.contains("at most 8 hours"), "{e}");
    assert!(
        h.permissions.list.is_empty(),
        "no shorter burst started silently"
    );
    let mut r = request(1, 1);
    r.allowance = Some(9);
    assert!(h.start_burst(&f, r, now).unwrap_err().contains("1..=8"));
    h.start_burst(&f, request(1, 1), now).unwrap();
    assert!(
        h.start_burst(&f, request(2, 1), now).is_err(),
        "one per project"
    );
    h.start_burst(&f, request(3, 1), now).unwrap();
    assert_eq!(h.bursts.len(), 2);
}

#[test]
fn ending_extending_and_changing_the_lead() {
    let f = Fake::new();
    let mut h = hub();
    let t0 = Instant::now();
    h.start_burst(&f, request(1, 1), t0).unwrap();
    // Ten minutes before the end: one notice.
    h.tick(&f, t0 + Duration::from_secs(3000));
    h.tick(&f, t0 + Duration::from_secs(3010));
    assert_eq!(
        h.notices.iter().filter(|n| n.contains("ends in")).count(),
        1
    );
    // Extending replaces the end with a new grant of at most 8 hours.
    assert!(
        h.extend_burst(1, Duration::from_secs(9 * 3600), t0)
            .is_err()
    );
    h.extend_burst(
        1,
        Duration::from_secs(2 * 3600),
        t0 + Duration::from_secs(3010),
    )
    .unwrap();
    assert_eq!(h.permissions.list.len(), 1);
    h.set_lead(1, 2).unwrap();
    assert_eq!(h.role_of(2), Some("lead"));
    assert_eq!(h.role_of(1), None, "one lead");
    // The burst ends by time: requests prompt again (SC-007), the engineer is told, Fallow posts.
    let end = t0 + Duration::from_secs(3010 + 2 * 3600);
    h.tick(&f, end);
    assert!(h.burst_of(1).is_none());
    assert!(h.permissions.list.is_empty());
    assert!(h.notices.iter().any(|n| n.contains("Coordination ended")));
    let r = h.call(&f, 1, "deliver", json!({"session": 2, "text": "go"}), end);
    assert!(matches!(r, Reply::Done(ref v) if v["state"] == "pending"));
    let Reply::Done(v) = h.call(&f, 2, "board_read", json!({}), end) else {
        panic!()
    };
    let last = v["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["from"], json!({"kind": "fallow"}));
    assert!(
        last["text"]
            .as_str()
            .unwrap()
            .contains("Coordination ended")
    );
    // Ending early.
    h.start_burst(&f, request(1, 1), end).unwrap();
    assert!(h.end_burst(1, "ended by you"));
    assert!(h.permissions.list.is_empty());
}
