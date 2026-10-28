//! 005 T009, T030, T034: one loop by default, prepared delegation, the plan-commit and
//! ownership checks (US1, US3, FR-001 to FR-010, FR-022 to FR-025).

use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, manifest, squad, tasks};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::{Value, json};

pub struct Fake(pub Vec<SessionInfo>);

pub fn info(id: u32) -> SessionInfo {
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

fn setup() -> (Hub, Fake, Instant) {
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

fn start(h: &mut Hub, f: &mut Fake, args: Value, id: u32, now: Instant) -> Reply {
    let mut a = json!({"profile": "claude", "project": "api"});
    for (k, v) in args.as_object().unwrap() {
        a[k] = v.clone();
    }
    let r = h.call(f, 1, "session_start", a, now);
    if let Reply::Done(v) = &r {
        if v["state"] == "approved" {
            f.0.push(info(id));
            h.start_done(f, v["start"].as_u64().unwrap(), Ok(id), now);
        }
    }
    r
}

fn refused(r: &Reply, needle: &str) -> bool {
    matches!(r, Reply::Error("not_allowed", m) if m.contains(needle))
}

fn lead_tasks(h: &Hub) -> Vec<&tasks::Task> {
    h.tasks
        .list
        .iter()
        .filter(|t| t.assigned == Some(1) && t.work.is_some())
        .collect()
}

fn check(h: &Hub, cond: &str) -> (bool, String) {
    let c = h
        .readiness(1)
        .unwrap()
        .checks
        .into_iter()
        .find(|c| c.condition == cond)
        .unwrap();
    (c.met, c.why)
}

fn manifest(text: &str) -> manifest::Manifest {
    manifest::parse(manifest::WORK_FILE, text).unwrap()
}

#[test]
fn without_a_manifest_the_lead_builds_the_work_itself() {
    // FR-001, FR-003, FR-008, R-16.
    let (mut h, mut f, now) = setup();
    let Reply::Done(w) = h.call(&f, 1, "whoami", json!({}), now) else {
        panic!()
    };
    let briefing = w["briefing"].as_str().unwrap();
    assert!(
        briefing.contains("build each piece of work yourself"),
        "{briefing}"
    );
    assert!(briefing.contains("subagents"), "{briefing}");
    assert_eq!(w["burst"]["manifest_commit"], Value::Null);
    // The lead's task for the whole work: in progress, never typed into its session.
    let t = lead_tasks(&h);
    assert_eq!(t.len(), 1, "{:?}", h.tasks.list);
    assert_eq!(t[0].state, tasks::State::InProgress);
    assert_eq!(t[0].work.as_deref(), Some(manifest::BURST_WORK));
    assert!(t[0].delivery.is_none());
    // A builder is refused, with the reason and what a split needs.
    let r = start(&mut h, &mut f, json!({"role": "builder"}), 2, now);
    assert!(refused(&r, "give work"), "{r:?}");
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart"}),
        2,
        now,
    );
    assert!(
        refused(&r, "built by the lead") && refused(&r, "build = \"builder\""),
        "{r:?}"
    );
    // A reviewer needs no work.
    let r = start(&mut h, &mut f, json!({"role": "reviewer"}), 3, now);
    assert!(
        matches!(&r, Reply::Done(v) if v["state"] == "approved"),
        "{r:?}"
    );
    // work:done waits for the lead's report; review:approved for the reviewer (004's rule).
    assert!(!check(&h, "work:done").0);
    assert!(check(&h, "work:done").1.contains("the lead's work"));
    let id = lead_tasks(&h)[0].id;
    let r = h.call(
        &f,
        1,
        "task_report",
        json!({"task": id, "outcome": "done", "evidence": {"commit": "abc1234", "files": ["a.rs"]}}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    assert!(check(&h, "work:done").0);
    assert!(!check(&h, "review:approved").0);
    h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(check(&h, "review:approved").0);
    assert!(h.readiness(1).unwrap().ready);
}

#[test]
fn the_manifest_gives_the_lead_its_pieces_and_delegates_the_rest() {
    // FR-001 to FR-003, R-16.
    let (mut h, mut f, now) = setup();
    h.apply_manifest(
        1,
        Some(manifest(
            "[[work]]\nid = \"copy\"\ncriteria = [{ id = \"t\", text = \"new wording\" }]\n\n[[work]]\nid = \"api\"\nbuild = \"builder\"\nreason = \"a fresh context from the plan\"\ncriteria = [{ id = \"a\", text = \"it works\" }]\n",
        )),
    );
    // The whole-work task goes; the lead gets one for the piece it builds.
    let t = lead_tasks(&h);
    let open: Vec<&str> = t
        .iter()
        .filter(|t| !t.state.finished())
        .map(|t| t.work.as_deref().unwrap())
        .collect();
    assert_eq!(open, ["copy"]);
    assert!(
        t.iter()
            .any(|t| t.work.as_deref() == Some(manifest::BURST_WORK)
                && matches!(t.state, tasks::State::Cancelled(_)))
    );
    assert!(t[1].text.contains("new wording"), "{}", t[1].text);
    // Work the lead builds gets no builder; delegated work gets exactly one.
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "copy"}),
        2,
        now,
    );
    assert!(refused(&r, "records copy as built by the lead"), "{r:?}");
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "api", "first_task": "build the api"}),
        2,
        now,
    );
    assert!(
        matches!(&r, Reply::Done(v) if v["state"] == "approved"),
        "{r:?}"
    );
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "api"}),
        3,
        now,
    );
    assert!(refused(&r, "has one"), "{r:?}");
    // The builder's first task, and tasks for it later, build what it was started for.
    let first = h
        .tasks
        .list
        .iter()
        .find(|t| t.text == "build the api")
        .unwrap();
    assert_eq!(first.work.as_deref(), Some("api"));
    let Reply::Done(v) = h.call(
        &f,
        1,
        "task_create",
        json!({"text": "and the docs", "target": {"session": 2}}),
        now,
    ) else {
        panic!()
    };
    let t = h.tasks.get(v["task"].as_u64().unwrap() as u32).unwrap();
    assert_eq!(t.work.as_deref(), Some("api"));
    // work:done needs both pieces.
    let (met, why) = check(&h, "work:done");
    assert!(!met && why.contains("copy") && why.contains("api"), "{why}");
}

#[test]
fn the_leads_own_work_does_not_keep_other_tasks_from_it() {
    // A task for the lead is still handed over while its own piece of work is in progress.
    let (mut h, f, now) = setup();
    let (id, _) = h
        .create_task(
            &f,
            fallow::orchestration::Party::User,
            "look at this".into(),
            tasks::Target::Session(1),
            Vec::new(),
            now,
        )
        .unwrap();
    h.tick(&f, now);
    assert_ne!(h.tasks.get(id).unwrap().state, tasks::State::Queued);
}

#[test]
fn a_squad_written_for_004_keeps_004s_builders() {
    // A squad whose merge waits for done:builder starts builders without a manifest.
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1)]);
    let now = Instant::now();
    let mut s = squad::builtin();
    s.merge_requires = vec![squad::Condition::AllDone("builder".into())];
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: None,
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
    assert!(lead_tasks(&h).is_empty(), "no lead task in a 004 squad");
    let r = start(&mut h, &mut f, json!({"role": "builder"}), 2, now);
    assert!(
        matches!(&r, Reply::Done(v) if v["state"] == "approved"),
        "{r:?}"
    );
}
