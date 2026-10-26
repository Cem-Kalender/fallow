//! 004 T025: gates Fallow observes (US4, FR-020 to FR-022a, SC-004, research R-5, R-6).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::board::BoardId;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::gates::{self, Class, GateRun, Kind, RunState};
use fallow::orchestration::identity::Peer;
use fallow::orchestration::squad::{self, Failures};
use fallow::orchestration::{Hub, Party, Reply, SessionInfo, Sessions};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::json;

fn names(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn failing_checks_are_named_from_common_runners() {
    let auto = Failures::Auto;
    let cargo =
        "running 3 tests\ntest a::ok ... ok\ntest a::bad ... FAILED\ntest b::worse ... FAILED\n";
    assert_eq!(
        gates::failing_names(cargo, &auto),
        names(&["a::bad", "b::worse"])
    );
    let pytest =
        "=== short test summary info ===\nFAILED tests/test_x.py::test_a - AssertionError\n";
    assert_eq!(
        gates::failing_names(pytest, &auto),
        names(&["tests/test_x.py::test_a"])
    );
    let go = "=== RUN   TestA\n--- FAIL: TestA (0.00s)\n    --- FAIL: TestA/sub (0.00s)\nFAIL\n";
    assert_eq!(
        gates::failing_names(go, &auto),
        names(&["TestA", "TestA/sub"])
    );
    let dotnet = "  Failed Ns.Tests.Adds [12 ms]\n  Passed Ns.Tests.Subs\n";
    assert_eq!(
        gates::failing_names(dotnet, &auto),
        names(&["Ns.Tests.Adds"])
    );
    let jest = "FAIL src/sum.test.js\n  ✕ adds numbers (5 ms)\n  ✓ subtracts\n";
    assert_eq!(
        gates::failing_names(jest, &auto),
        names(&["src/sum.test.js", "adds numbers"])
    );
    assert!(gates::failing_names(cargo, &Failures::ExitCode).is_empty());
    let re = Failures::Regex(r"^BROKEN: (\w+)".into());
    assert_eq!(
        gates::failing_names("BROKEN: login\nfine\nBROKEN: logout\n", &re),
        names(&["login", "logout"])
    );
}

fn run(kind: Kind, state: RunState, failing: &[&str]) -> GateRun {
    let now = Instant::now();
    GateRun {
        gate: "tests".into(),
        kind,
        session: 1,
        commit: None,
        state,
        failing: names(failing),
        class: None,
        started: now,
        deadline: now,
        after_tasks: 0,
        dir: None,
    }
}

#[test]
fn final_runs_are_classified_against_the_baseline() {
    // research R-5's table.
    let failed = RunState::Failed(Some(101));
    let base_fail = run(Kind::Baseline, failed.clone(), &["a", "b"]);
    let base_pass = run(Kind::Baseline, RunState::Passed, &[]);
    let fin = |f: &[&str]| run(Kind::Final, failed.clone(), f);
    assert_eq!(
        gates::classify(&run(Kind::Final, RunState::Passed, &[]), Some(&base_fail)),
        Class::Pass
    );
    assert_eq!(
        gates::classify(&fin(&["a"]), Some(&base_fail)),
        Class::BaselineOnly
    );
    assert_eq!(
        gates::classify(&fin(&["a", "c"]), Some(&base_fail)),
        Class::Regression
    );
    assert_eq!(
        gates::classify(&fin(&["a"]), Some(&base_pass)),
        Class::Regression
    );
    assert_eq!(
        gates::classify(&fin(&[]), Some(&base_fail)),
        Class::Fail,
        "no names"
    );
    assert_eq!(
        gates::classify(&fin(&["a"]), None),
        Class::Fail,
        "no baseline"
    );
    assert!(Class::BaselineOnly.opens() && !Class::Regression.opens());
}

#[test]
fn a_gate_command_has_per_os_overrides() {
    // FR-022a.
    let text = r#"
[[gate]]
name = "tests"
command = "make test"
windows = "nmake test"
"#;
    let s = squad::parse(".fallow/squad.toml", text, &[]).unwrap();
    let g = &s.gates[0];
    assert_eq!(g.command_for("windows"), "nmake test");
    assert_eq!(g.command_for("linux"), "make test");
    assert_eq!(g.command_for("macos"), "make test");
    assert_eq!(g.timeout_min, 30, "default");
}

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
[[gate]]
name = "tests"
command = "cargo test"
timeout_min = 1
baseline = true
[merge]
requires = ["gate:tests", "done:builder"]
"#;

#[test]
fn baseline_first_then_a_final_run_fallow_signals_itself() {
    // FR-020, FR-021, FR-022, SC-004.
    let s = squad::parse(".fallow/squad.toml", SQUAD, &squad::builtin().roles).unwrap();
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1)]);
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: None,
            squad: s,
            start_commit: Some("c0ffee0".into()),
            target_branch: Some("main".into()),
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    // The baseline is due at once, on the starting commit.
    let jobs = h.gates_due();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        (jobs[0].kind, jobs[0].commit.as_deref()),
        (Kind::Baseline, Some("c0ffee0"))
    );
    // A builder's first task waits for it.
    let r = h.call(
        &f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": "builder", "first_task": "build"}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    f.0.push(info(2));
    h.start_done(&f, v["start"].as_u64().unwrap(), Ok(2), now);
    h.gate_started(&jobs[0], Ok(7), jobs[0].commit.clone(), None, now);
    assert_eq!(h.role_of(7), Some(gates::GATE_ROLE));
    assert!(h.gates_due().is_empty(), "one run at a time");
    h.tick(&f, now);
    let task = h.tasks.list.last().unwrap().id;
    assert!(
        h.tasks.get(task).unwrap().assigned.is_none(),
        "held for the baseline"
    );
    // An agent cannot fake the gate.
    let r = h.call(
        &f,
        1,
        "event_signal",
        json!({"name": "gate-tests-passed"}),
        now,
    );
    assert!(matches!(r, Reply::Error("not_allowed", _)), "{r:?}");
    // The baseline fails on test a::old; builders begin.
    let run = h
        .gate_finished(7, Some(101), "test a::old ... FAILED\n")
        .unwrap();
    assert_eq!(run.failing, names(&["a::old"]));
    h.tick(&f, now);
    assert_eq!(h.tasks.get(task).unwrap().assigned, Some(2));
    assert!(
        h.gates_due().is_empty(),
        "no final run before the builder is done"
    );
    // The builder reports; the final run is due in the lead's worktree.
    h.call(
        &f,
        2,
        "task_report",
        json!({"task": task, "outcome": "done"}),
        now,
    );
    let jobs = h.gates_due();
    assert_eq!(
        (jobs.len(), jobs[0].kind, jobs[0].lead),
        (1, Kind::Final, 1)
    );
    h.gate_started(&jobs[0], Ok(8), Some("abc1234".into()), None, now);
    h.gate_finished(8, Some(101), "test a::old ... FAILED\n");
    let b = h.burst_of(1).unwrap();
    assert_eq!(b.gate_runs[1].class, Some(Class::BaselineOnly));
    // Fallow signalled it, as itself.
    let key = (BoardId::Project(1), "gate-tests-passed".to_string());
    let sig = h.events.map.get(&key).unwrap().signal.clone().unwrap();
    assert_eq!(sig.from, Party::Fallow);
    let r = h.readiness(1).unwrap();
    assert!(r.ready, "{r:?}");
    assert_eq!(r.commit.as_deref(), Some("abc1234"));
    assert!(
        h.gates_due().is_empty(),
        "not again until more work is done"
    );
}

#[test]
fn a_gate_past_its_time_limit_fails_and_its_session_is_ended() {
    let s = squad::parse(".fallow/squad.toml", SQUAD, &squad::builtin().roles).unwrap();
    let mut h = Hub::new(OrchestrationSettings::default());
    let f = Fake(vec![info(1)]);
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: None,
            squad: s,
            start_commit: Some("c0ffee0".into()),
            target_branch: None,
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    let job = h.gates_due().remove(0);
    h.gate_started(&job, Ok(7), None, None, now);
    h.tick(&f, now + Duration::from_secs(61));
    let b = h.burst_of(1).unwrap();
    assert_eq!(b.gate_runs[0].state, RunState::TimedOut);
    assert_eq!(h.end_gates, [7]);
    // A gate that could not start is recorded as failed, not retried at once.
    let mut h2 = Hub::new(OrchestrationSettings::default());
    h2.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: None,
            squad: squad::parse(".fallow/squad.toml", SQUAD, &squad::builtin().roles).unwrap(),
            start_commit: Some("c0ffee0".into()),
            target_branch: None,
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    let job = h2.gates_due().remove(0);
    h2.gate_started(&job, Err("limit".into()), None, None, now);
    assert!(h2.gates_due().is_empty());
    assert!(!h2.baseline_pending(1));
}
