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

const PARTS: &str = r#"
[[work]]
id = "cart"
build = "parts"
reason = "API and page meet only at the contract"

[[part]]
id = "api"
work = "cart"
owns = ["src/api/**", "tests/api/**"]
criteria = [{ id = "add", text = "adds" }]

[[part]]
id = "page"
work = "cart"
owns = ["web/**"]
criteria = [{ id = "shows", text = "shows" }]

[[contract]]
path = "contracts/cart.yaml"
between = ["api", "page"]

[[work]]
id = "extras"
build = "features"
reason = "two unrelated features"

[[part]]
id = "export"
work = "extras"
owns = ["src/export/**"]
criteria = [{ id = "csv", text = "exports csv" }]

[[part]]
id = "import"
work = "extras"
owns = ["src/import/**"]
criteria = [{ id = "csv-in", text = "imports csv" }]
"#;

#[test]
fn parallel_parts_start_only_when_prepared() {
    // T030: FR-004 to FR-006, FR-010.
    let (mut h, mut f, now) = setup();
    let mut m = manifest(PARTS);
    m.commit = Some("c".repeat(40));
    h.apply_manifest(1, Some(m.clone()));
    // The files at the lead's commit: the contract is committed.
    h.bursts[0].manifest_files = vec!["contracts/cart.yaml".into(), "src/api/x.rs".into()];
    // A part is named, and it must be one of the work's parts.
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart"}),
        2,
        now,
    );
    assert!(refused(&r, "give part, one of api, page"), "{r:?}");
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart", "part": "nope"}),
        2,
        now,
    );
    assert!(refused(&r, "not a part of cart"), "{r:?}");
    let ok = |r: &Reply| matches!(r, Reply::Done(v) if v["state"] == "approved");
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart", "part": "api"}),
        2,
        now,
    );
    assert!(ok(&r), "{r:?}");
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart", "part": "api"}),
        3,
        now,
    );
    assert!(refused(&r, "already has a builder"), "{r:?}");
    // Features need no contract.
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "extras", "part": "export"}),
        3,
        now,
    );
    assert!(ok(&r), "{r:?}");
    // The contract must be committed.
    h.bursts[0].manifest_files = vec!["src/api/x.rs".into()];
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "cart", "part": "page"}),
        4,
        now,
    );
    assert!(refused(&r, "contracts/cart.yaml is not committed"), "{r:?}");
    // The squad's cap on parallel builders (the burst's allowance by default: 5).
    h.bursts[0].squad.max_parallel_builders = Some(2);
    h.bursts[0].manifest_files = vec!["contracts/cart.yaml".into()];
    let r = start(
        &mut h,
        &mut f,
        json!({"role": "builder", "work": "extras", "part": "import"}),
        4,
        now,
    );
    assert!(refused(&r, "at most 2 builder(s)"), "{r:?}");
}

#[test]
fn overlapping_test_only_and_uncontracted_parts_are_refused() {
    // T030: FR-004, FR-006.
    let cases = [
        // Overlapping ownership, named.
        (
            "[[work]]\nid = \"w\"\nbuild = \"features\"\nreason = \"r\"\n[[part]]\nid = \"a\"\nwork = \"w\"\nowns = [\"src/**\"]\n[[part]]\nid = \"b\"\nwork = \"w\"\nowns = [\"src/b/**\"]\n",
            "a",
            "both own",
        ),
        // A part that owns only tests: one feature split into building and testing.
        (
            "[[work]]\nid = \"w\"\nbuild = \"features\"\nreason = \"r\"\n[[part]]\nid = \"a\"\nwork = \"w\"\nowns = [\"src/a/**\"]\n[[part]]\nid = \"b\"\nwork = \"w\"\nowns = [\"tests/**\"]\n",
            "b",
            "owns only tests",
        ),
        // A part of parallel work without a contract.
        (
            "[[work]]\nid = \"w\"\nbuild = \"parts\"\nreason = \"r\"\n[[part]]\nid = \"a\"\nwork = \"w\"\nowns = [\"src/a/**\"]\n[[part]]\nid = \"b\"\nwork = \"w\"\nowns = [\"src/b/**\"]\n",
            "a",
            "has no contract",
        ),
        // A part that owns nothing.
        (
            "[[work]]\nid = \"w\"\nbuild = \"features\"\nreason = \"r\"\n[[part]]\nid = \"a\"\nwork = \"w\"\n",
            "a",
            "owns no files",
        ),
    ];
    for (text, part, needle) in cases {
        let (mut h, mut f, now) = setup();
        h.apply_manifest(1, Some(manifest(text)));
        let r = start(
            &mut h,
            &mut f,
            json!({"role": "builder", "work": "w", "part": part}),
            2,
            now,
        );
        assert!(refused(&r, needle), "{needle}: {r:?}");
    }
    // Overlap is found from the files at the commit as well as from the globs.
    use fallow::orchestration::delegation::{overlap, owns_code};
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert!(overlap(&s(&["src/*.rs"]), &s(&["src/main.rs"]), &[]).len() == 1);
    assert!(overlap(&s(&["src/a/**"]), &s(&["src/b/**"]), &[]).is_empty());
    assert_eq!(
        overlap(
            &s(&["**/*.rs"]),
            &s(&["src/**"]),
            &s(&["src/x.rs", "README.md"])
        ),
        ["src/x.rs"]
    );
    let tests = s(&["tests/**", "**/*_test.*"]);
    assert!(owns_code(&s(&["src/a/**", "tests/a/**"]), &tests, &[], &[]));
    assert!(!owns_code(&s(&["tests/**"]), &tests, &[], &[]));
    assert!(!owns_code(
        &s(&["docs/plan.md"]),
        &tests,
        &s(&["docs/plan.md"]),
        &[]
    ));
}

#[test]
fn the_leads_own_work_passes_to_the_next_lead() {
    // 005 review, FR-033: the lead's exit does not fail its work; a new lead takes it over.
    let (mut h, mut f, now) = setup();
    let id = lead_tasks(&h)[0].id;
    h.session_ended(1);
    let t = h.tasks.list.iter().find(|t| t.id == id).unwrap();
    assert_eq!(t.state, tasks::State::InProgress, "{t:?}");
    f.0.push(info(2));
    h.set_lead(1, 2).unwrap();
    let t = h.tasks.list.iter().find(|t| t.id == id).unwrap();
    assert_eq!(t.assigned, Some(2));
    assert_eq!(t.target, tasks::Target::Session(2));
    // Applying the manifest again (as after the new lead's next commit) creates no second
    // task for the same work.
    h.call(&f, 2, "whoami", json!({}), now);
    h.apply_manifest(1, None);
    let open = h
        .tasks
        .list
        .iter()
        .filter(|t| t.work.as_deref() == Some(manifest::BURST_WORK) && !t.state.finished())
        .count();
    assert_eq!(open, 1);
}
