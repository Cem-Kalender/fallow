//! 005 T044: the memory budget (US6, FR-037, FR-038, SC-008, research R-14).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::memory::Sample;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, StartState, squad};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::json;

struct Fake {
    list: Vec<SessionInfo>,
    memory: RefCell<Option<Sample>>,
}

fn info(id: u32) -> SessionInfo {
    SessionInfo {
        id,
        project: 1,
        project_name: "api".into(),
        profile_id: "claude".into(),
        profile: "Claude Code".into(),
        state: "working".into(),
        running: true,
        idle: true,
        role: None,
    }
}

impl Sessions for Fake {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.list.iter().find(|s| s.id == id).cloned()
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.list.clone()
    }
    fn verify(&self, _: u32, _: &str, _: Peer) -> Result<(), String> {
        Ok(())
    }
    fn profiles(&self) -> Vec<(String, String)> {
        builtins().into_iter().map(|p| (p.id, p.name)).collect()
    }
    fn profile(&self, id: &str) -> Option<AgentProfile> {
        builtins().into_iter().find(|p| p.id == id)
    }
    fn memory(&self) -> Option<Sample> {
        self.memory.borrow().clone()
    }
}

fn sample(sessions: &[(u32, &str, u64)]) -> Sample {
    Sample {
        total_mb: 4096,
        fallow_mb: 40,
        sessions: sessions
            .iter()
            .map(|(id, p, mb)| (*id, (*p).to_string(), *mb))
            .collect(),
    }
}

fn setup(budget: u64) -> (Hub, Fake, Instant) {
    let settings = OrchestrationSettings {
        memory_budget_mb: budget,
        ..OrchestrationSettings::default()
    };
    let mut h = Hub::new(settings);
    let f = Fake {
        list: vec![info(1)],
        memory: RefCell::new(Some(sample(&[(1, "claude", 600)]))),
    };
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: Some(5),
            squad: squad::builtin(),
            start_commit: None,
            target_branch: None,
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    (h, f, now)
}

fn reviewer(h: &mut Hub, f: &Fake, now: Instant) -> u64 {
    let r = h.call(
        f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": "reviewer"}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    v["start"].as_u64().unwrap()
}

#[test]
fn the_budget_is_the_setting_or_three_quarters_of_memory_minus_fallow() {
    let (h, _, _) = setup(0);
    assert_eq!(h.memory_budget(&sample(&[])), 4096 * 3 / 4 - 40);
    let (h, _, _) = setup(1500);
    assert_eq!(h.memory_budget(&sample(&[])), 1500);
}

#[test]
fn a_start_that_does_not_fit_waits_and_goes_when_memory_is_free() {
    // FR-037, FR-038, SC-008: lead 600 MB + expected 700 MB > 1200 MB.
    let (mut h, f, now) = setup(1200);
    let id = reviewer(&mut h, &f, now);
    assert_eq!(h.memory_waits, [(id, 100)]);
    assert!(
        h.approved_starts().is_empty(),
        "nothing starts beyond the budget"
    );
    assert!(
        h.alerts
            .iter()
            .any(|a| a.contains("waits for memory: 100 MB short")),
        "{:?}",
        h.alerts
    );
    assert!(
        h.burst_of(1)
            .unwrap()
            .lead_updates
            .join("\n")
            .contains("waits for memory"),
        "the lead is told"
    );
    // A run in progress counts the wait as something in progress (FR-013).
    assert!(h.memory_waiting(1));
    let Reply::Done(v) = h.call(&f, 1, "squad_status", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["memory"]["waiting"][0]["short_mb"], 100);
    assert_eq!(v["memory"]["budget_mb"], 1200);
    // Still no room: it keeps waiting. The measured 650 MB is now the expected size of a
    // Claude session too: 650 + 650 - 1200.
    *f.memory.borrow_mut() = Some(sample(&[(1, "claude", 650)]));
    h.tick(&f, now);
    assert_eq!(h.memory_waits, [(id, 100)]);
    // The lead's footprint shrinks: the start goes ahead by itself.
    *f.memory.borrow_mut() = Some(sample(&[(1, "claude", 400)]));
    h.tick(&f, now);
    assert!(h.memory_waits.is_empty());
    assert_eq!(h.approved_starts().len(), 1);
    assert_eq!(
        h.starts.iter().find(|s| s.id == id).unwrap().state,
        StartState::Approved
    );
}

#[test]
fn the_expected_size_is_the_largest_seen_for_the_profile() {
    // R-14: measured sizes replace the setting once seen.
    let (mut h, f, now) = setup(2000);
    assert_eq!(h.expected_size("claude"), 700, "the setting until measured");
    *f.memory.borrow_mut() = Some(sample(&[(1, "claude", 900), (9, "gate", 120)]));
    h.tick(&f, now);
    assert_eq!(h.expected_size("claude"), 900);
    assert_eq!(
        h.expected_size("gate"),
        120,
        "gate sessions are measured too"
    );
    *f.memory.borrow_mut() = Some(sample(&[(1, "claude", 500)]));
    h.tick(&f, now);
    assert_eq!(h.expected_size("claude"), 900, "the largest seen");
    // 500 used + 900 expected <= 2000: fits.
    reviewer(&mut h, &f, now);
    assert!(h.memory_waits.is_empty());
    // Gates count in the measured sum: 500 + 1000 (a build in a gate) + 900 > 2000.
    *f.memory.borrow_mut() = Some(sample(&[(1, "claude", 500), (9, "gate", 1000)]));
    let short = h.memory_shortfall(&f.memory().unwrap(), "claude");
    assert_eq!(short, Some(400));
}
