//! 004 T023: merge conditions and readiness (US3, FR-016 to FR-019, FR-027, SC-003).

use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::gates::{Class, GateRun, Kind, RunState};
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, squad};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::json;

struct Fake(Vec<SessionInfo>);

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
        self.0.iter().find(|s| s.id == id).cloned()
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.0.clone()
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
}

const SQUAD: &str = r#"
name = "web"
[[role]]
name = "lead"
description = "Leads."
level = "edit-run"
tools = "all"
signals = ["*"]
max = 1
[[role]]
name = "backend"
description = "Builds the API."
level = "edit-run"
tools = ["board", "events", "report", "sessions"]
signals = ["api-*"]
max = 1
[[role]]
name = "frontend"
description = "Builds the page."
level = "edit-run"
tools = ["board", "events", "report", "sessions"]
signals = ["page-*"]
max = 1
[[role]]
name = "reviewer"
description = "Reviews."
level = "read-only"
tools = ["board", "events", "report", "sessions"]
signals = ["review-*"]
max = 2
[[gate]]
name = "tests"
command = "cargo test"
[merge]
requires = ["done:backend", "done:frontend", "signal:review-approved@reviewer", "gate:tests"]
"#;

fn burst_with(h: &mut Hub, f: &Fake, s: squad::Squad, now: Instant) {
    h.start_burst(
        f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: Some(5),
            squad: s,
            start_commit: None,
            target_branch: None,
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
}

fn start(h: &mut Hub, f: &mut Fake, role: &str, id: u32, now: Instant) {
    let r = h.call(
        f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": role}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    f.0.push(info(id));
    h.start_done(f, v["start"].as_u64().unwrap(), Ok(id), now);
}

/// The lead hands `text` to `to`; returns the task number once it is in progress there.
fn hand(h: &mut Hub, f: &Fake, to: u32, now: Instant) -> u64 {
    let r = h.call(
        f,
        1,
        "task_create",
        json!({"text": "build it", "target": {"session": to}}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    h.tick(f, now);
    v["task"].as_u64().unwrap()
}

fn done(h: &mut Hub, f: &Fake, who: u32, task: u64, now: Instant) {
    let r = h.call(
        f,
        who,
        "task_report",
        json!({"task": task, "outcome": "done"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
}

fn ready(h: &Hub) -> bool {
    h.readiness(1).unwrap().ready
}

fn gate_run(state: RunState, class: Option<Class>, now: Instant) -> GateRun {
    GateRun {
        gate: "tests".into(),
        kind: Kind::Final,
        session: 9,
        commit: Some("abc1234".into()),
        state,
        failing: Default::default(),
        class,
        started: now,
        deadline: now + Duration::from_secs(60),
        after_tasks: 0,
        dir: None,
    }
}

#[test]
fn the_merge_stays_closed_until_every_condition_holds() {
    // SC-003, the US3 independent test.
    let s = squad::parse(".fallow/squad.toml", SQUAD, &[]).unwrap();
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1)]);
    let now = Instant::now();
    burst_with(&mut h, &f, s, now);
    start(&mut h, &mut f, "backend", 2, now);
    start(&mut h, &mut f, "frontend", 3, now);
    start(&mut h, &mut f, "reviewer", 4, now);
    let r = h.readiness(1).unwrap();
    assert_eq!(
        r.checks
            .iter()
            .map(|c| c.condition.as_str())
            .collect::<Vec<_>>(),
        [
            "done:backend",
            "done:frontend",
            "signal:review-approved@reviewer",
            "gate:tests"
        ]
    );
    assert!(r.checks.iter().all(|c| !c.met));
    let t2 = hand(&mut h, &f, 2, now);
    let t3 = hand(&mut h, &f, 3, now);
    done(&mut h, &f, 2, t2, now);
    assert!(!ready(&h));
    done(&mut h, &f, 3, t3, now);
    assert!(!ready(&h));
    h.call(
        &f,
        4,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(!ready(&h), "the gate has not run");
    h.bursts[0].gate_runs.push(gate_run(
        RunState::Failed(Some(1)),
        Some(Class::Regression),
        now,
    ));
    assert!(!ready(&h), "a regression keeps it closed");
    h.bursts[0].gate_runs.push(gate_run(
        RunState::Failed(Some(1)),
        Some(Class::BaselineOnly),
        now,
    ));
    let r = h.readiness(1).unwrap();
    assert!(r.ready, "{r:?}");
    assert_eq!(r.commit.as_deref(), Some("abc1234"));
    // A second reviewer joins: its verdict is needed too.
    start(&mut h, &mut f, "reviewer", 5, now);
    assert!(!ready(&h));
    h.call(
        &f,
        5,
        "event_signal",
        json!({"name": "review-changes-requested"}),
        now,
    );
    assert!(!ready(&h));
    h.call(
        &f,
        5,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(ready(&h));
    // A reviewer that changes its verdict closes it again.
    h.call(
        &f,
        4,
        "event_signal",
        json!({"name": "review-changes-requested"}),
        now,
    );
    assert!(!ready(&h));
}

#[test]
fn the_built_in_squad_needs_builder_work_and_a_review() {
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1)]);
    let now = Instant::now();
    burst_with(&mut h, &f, squad::builtin(), now);
    let r = h.readiness(1).unwrap();
    assert_eq!(
        r.checks
            .iter()
            .map(|c| c.condition.as_str())
            .collect::<Vec<_>>(),
        ["done:builder", "signal:review-approved@reviewer"]
    );
    assert_eq!(r.mode.label(), "local-merge", "no remote");
    assert!(r.checks[0].why.contains("no task"));
    start(&mut h, &mut f, "builder", 2, now);
    start(&mut h, &mut f, "reviewer", 3, now);
    let t = hand(&mut h, &f, 2, now);
    done(&mut h, &f, 2, t, now);
    // The lead's own approval does not count (no self-approval).
    let r = h.call(
        &f,
        1,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(matches!(r, Reply::Error("not_allowed", _)));
    assert!(!ready(&h));
    h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(ready(&h));
    // squad_status tells every session the same.
    let Reply::Done(v) = h.call(&f, 2, "squad_status", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["merge"]["ready"], true);
    assert_eq!(v["merge"]["publish"], "local-merge");
    assert_eq!(v["roles"][1]["sessions"], json!([2]));
}

#[test]
fn an_invalid_squad_file_blocks_the_burst_and_a_running_burst_keeps_its_rules() {
    // FR-017, FR-018.
    let d = std::env::temp_dir().join(format!("fallow-squadfile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".fallow")).unwrap();
    let file = d.join(".fallow").join("squad.toml");
    std::fs::write(
        &file,
        "name = \"x\"\n[[role]]\nname = \"lead\"\nlevel = \"admin\"\n",
    )
    .unwrap();
    let e = squad::load(&d, None).unwrap_err();
    assert!(e.iter().any(|p| p.contains("squad.toml:2:")), "{e:?}");
    std::fs::write(&file, SQUAD).unwrap();
    let s = squad::load(&d, None).unwrap();
    let mut h = Hub::new(OrchestrationSettings::default());
    let f = Fake(vec![info(1)]);
    let now = Instant::now();
    burst_with(&mut h, &f, s, now);
    std::fs::write(
        &file,
        SQUAD.replace("signal:review-approved@reviewer\", ", ""),
    )
    .unwrap();
    assert_eq!(
        h.readiness(1).unwrap().checks.len(),
        4,
        "frozen at the start"
    );
    let _ = std::fs::remove_dir_all(&d);
}
