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
        json!({"task": task, "outcome": "done", "evidence": {"commit": "abc1234", "files": []}}),
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

fn builtin_burst(has_remote: bool) -> (Hub, Fake, Instant) {
    let mut h = Hub::new(OrchestrationSettings::default());
    let f = Fake(vec![info(1)]);
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: Some(5),
            squad: squad::builtin(),
            start_commit: None,
            target_branch: Some("main".into()),
            has_remote,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    (h, f, now)
}

fn verdict(h: &mut Hub, f: &Fake, who: u32, v: &str, notes: &str, now: Instant) -> Reply {
    h.call(
        f,
        who,
        "review_verdict",
        json!({"verdict": v, "notes": notes}),
        now,
    )
}

#[test]
fn verdicts_are_reviewers_own_and_disagreement_goes_to_the_engineer() {
    // FR-027, US6 scenarios 1 and 2.
    let (mut h, mut f, now) = builtin_burst(false);
    start(&mut h, &mut f, "builder", 2, now);
    start(&mut h, &mut f, "reviewer", 3, now);
    start(&mut h, &mut f, "reviewer", 4, now);
    assert!(
        h.notices.iter().any(|n| n.contains("same agent and model")),
        "{:?}",
        h.notices
    );
    let t = hand(&mut h, &f, 2, now);
    done(&mut h, &f, 2, t, now);
    for who in [1, 2] {
        let r = verdict(&mut h, &f, who, "approved", "fine", now);
        assert!(matches!(r, Reply::Error("not_allowed", _)), "#{who}: {r:?}");
    }
    assert!(matches!(
        verdict(&mut h, &f, 3, "approved", "spec covered", now),
        Reply::Done(_)
    ));
    assert!(!ready(&h), "both reviewers must approve");
    assert!(matches!(
        verdict(&mut h, &f, 4, "changes-requested", "missing tests", now),
        Reply::Done(_)
    ));
    let n = h
        .notices
        .iter()
        .find(|n| n.contains("Reviewers disagree"))
        .expect("disagreement shown");
    assert!(
        n.contains("spec covered") && n.contains("missing tests"),
        "{n}"
    );
    assert!(!ready(&h));
    // The request for changes became a repair for the lead (no part named).
    h.tick(&f, now);
    assert!(h.tasks.list.iter().any(|t| t.repair_round == 1));
    // #4 approves after all: ready, and squad_status lists both verdicts.
    verdict(&mut h, &f, 4, "approved", "now fine", now);
    assert!(ready(&h));
    let Reply::Done(v) = h.call(&f, 1, "squad_status", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["verdicts"].as_array().unwrap().len(), 2);
    // No agent is ever handed a merge: Fallow's own tasks are repairs only (SC-009).
    assert!(
        h.tasks
            .list
            .iter()
            .filter(|t| t.created_by == fallow::orchestration::Party::Fallow)
            .all(|t| t.repair_round > 0 && !t.text.contains("git merge"))
    );
}

#[test]
fn publishing_follows_the_mode_and_only_the_lead_reports_a_pull_request() {
    // FR-028: a remote means draft-pr, none means local-merge.
    let (h, _, _) = builtin_burst(false);
    assert_eq!(h.readiness(1).unwrap().mode.label(), "local-merge");
    let (mut h, mut f, now) = builtin_burst(true);
    assert_eq!(h.readiness(1).unwrap().mode.label(), "draft-pr");
    start(&mut h, &mut f, "builder", 2, now);
    start(&mut h, &mut f, "reviewer", 3, now);
    let publish = |h: &mut Hub, f: &Fake, who: u32| {
        h.call(
            f,
            who,
            "publish",
            json!({"title": "Login page", "url": "https://example.com/pr/1"}),
            now,
        )
    };
    let r = publish(&mut h, &f, 1);
    assert!(
        matches!(r, Reply::Error("not_allowed", ref m) if m.contains("not every merge condition")),
        "{r:?}"
    );
    let t = hand(&mut h, &f, 2, now);
    done(&mut h, &f, 2, t, now);
    verdict(&mut h, &f, 3, "approved", "ok", now);
    let r = publish(&mut h, &f, 2);
    assert!(
        matches!(r, Reply::Error("not_allowed", _)),
        "builder: {r:?}"
    );
    let r = publish(&mut h, &f, 1);
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    assert_eq!(
        h.burst_of(1).unwrap().published.as_deref(),
        Some("https://example.com/pr/1")
    );
    // Local-merge mode refuses it: the engineer merges.
    let (mut h, mut f, now) = builtin_burst(false);
    start(&mut h, &mut f, "builder", 2, now);
    start(&mut h, &mut f, "reviewer", 3, now);
    let t = hand(&mut h, &f, 2, now);
    done(&mut h, &f, 2, t, now);
    verdict(&mut h, &f, 3, "approved", "ok", now);
    let r = publish(&mut h, &f, 1);
    assert!(
        matches!(r, Reply::Error("not_allowed", ref m) if m.contains("merges the gated commit in Fallow")),
        "{r:?}"
    );
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn the_local_merge_takes_exactly_the_gated_commit_and_reports_foreign_ones() {
    // FR-028, FR-028a on a real repository.
    use fallow::orchestration::publish::{foreign, local_merge};
    let d = std::env::temp_dir().join(format!("fallow-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "x\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "start"]);
    let start = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["checkout", "-q", "-b", "lead"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "gated"]);
    let gated = git(&d, &["rev-parse", "HEAD"]);
    std::fs::write(d.join("b.txt"), "b\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "after the gate"]);
    let later = git(&d, &["rev-parse", "HEAD"]);
    // Not on the target branch: refused.
    let e = local_merge(&d, "main", &gated).unwrap_err();
    assert!(e.contains("not on the target branch main"), "{e}");
    git(&d, &["checkout", "-q", "main"]);
    // Not clean: refused.
    std::fs::write(d.join("README"), "changed\n").unwrap();
    let e = local_merge(&d, "main", &gated).unwrap_err();
    assert!(e.contains("uncommitted"), "{e}");
    git(&d, &["checkout", "--", "README"]);
    // Someone commits on main during the burst: foreign.
    std::fs::write(d.join("hotfix.txt"), "h\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "hotfix"]);
    let hotfix = git(&d, &["rev-parse", "HEAD"]);
    assert_eq!(foreign(&d, &start, "main", &[]).unwrap(), [hotfix.as_str()]);
    // The merge takes the gated commit, not the later one.
    let merge = local_merge(&d, "main", &gated).unwrap();
    git(&d, &["merge-base", "--is-ancestor", &gated, "main"]);
    let o = std::process::Command::new("git")
        .current_dir(&d)
        .args(["merge-base", "--is-ancestor", &later, "main"])
        .output()
        .unwrap();
    assert!(
        !o.status.success(),
        "the commit after the gate is not merged"
    );
    let merged = vec![(gated.clone(), merge.clone())];
    assert_eq!(
        foreign(&d, &start, "main", &merged).unwrap(),
        [hotfix.as_str()]
    );
    // The hub reports each foreign commit once.
    let (mut h, _, _) = builtin_burst(false);
    h.foreign_commits(1, vec![hotfix.clone()]);
    h.foreign_commits(1, vec![hotfix.clone()]);
    assert_eq!(
        h.notices
            .iter()
            .filter(|n| n.contains("not merged by Fallow"))
            .count(),
        1
    );
    h.merged(1, &gated, &merge);
    assert_eq!(h.burst_of(1).unwrap().merged.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_squad_may_merge_automatically() {
    let text = "[merge]\nmode = \"automatic\"\nrequires = [\"done:builder\"]\n";
    let s = squad::parse(".fallow/squad.toml", text, &[]).unwrap();
    assert_eq!(s.merge, squad::Merge::Automatic);
}
