//! 004 T029: evidence and capped repair (US5, FR-023 to FR-026, research R-7).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::repair::Decision;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, squad, tasks};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::{Value, json};

/// Sessions in project 1; every session works in `dir` on branch "feat" (the lead on "main").
struct Fake {
    list: Vec<SessionInfo>,
    dir: PathBuf,
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
    fn workdir(&self, id: u32) -> Option<(PathBuf, Option<String>)> {
        let branch = if id == 1 { "main" } else { "feat" };
        Some((self.dir.clone(), Some(branch.into())))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
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

/// A repository: c0 on main (the burst's start), c1 on feat (a.txt), c2 on other (b.txt).
fn repo(tag: &str) -> (PathBuf, String, String, String) {
    let d = std::env::temp_dir().join(format!("fallow-ev-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "x\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "c0"]);
    let c0 = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["checkout", "-q", "-b", "feat"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "c1"]);
    let c1 = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["checkout", "-q", "-b", "other", &c0]);
    std::fs::write(d.join("b.txt"), "b\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "c2"]);
    let c2 = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["checkout", "-q", "main"]);
    (d, c0, c1, c2)
}

fn setup(dir: &Path, c0: &str, cap: u8) -> (Hub, Fake, Instant) {
    // 004's merge conditions: these tests cover 004's evidence and repairs.
    let mut s = squad::builtin();
    s.merge_requires = vec![
        squad::Condition::AllDone("builder".into()),
        squad::Condition::Signal {
            event: "review-approved".into(),
            role: "reviewer".into(),
        },
    ];
    s.repair_cap = cap;
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake {
        list: vec![info(1)],
        dir: dir.to_path_buf(),
    };
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(4 * 3600),
            allowance: None,
            squad: s,
            start_commit: Some(c0.into()),
            target_branch: Some("main".into()),
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    for (id, role) in [(2, "builder"), (3, "reviewer")] {
        let r = h.call(
            &f,
            1,
            "session_start",
            json!({"profile": "claude", "project": "api", "role": role}),
            now,
        );
        let Reply::Done(v) = r else { panic!("{r:?}") };
        f.list.push(info(id));
        h.start_done(&f, v["start"].as_u64().unwrap(), Ok(id), now);
    }
    (h, f, now)
}

/// The lead hands a task for `part` to `to`; it is in progress there.
fn hand(h: &mut Hub, f: &Fake, to: u32, part: &str, now: Instant) -> u32 {
    let r = h.call(
        f,
        1,
        "task_create",
        json!({"text": format!("do {part}"), "target": {"session": to}, "part": part}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    h.tick(f, now);
    let id = v["task"].as_u64().unwrap() as u32;
    assert_eq!(h.tasks.get(id).unwrap().assigned, Some(to));
    id
}

fn report(
    h: &mut Hub,
    f: &Fake,
    who: u32,
    task: u32,
    evidence: Option<Value>,
    now: Instant,
) -> Reply {
    let mut a = json!({"task": task, "outcome": "done"});
    if let Some(e) = evidence {
        a["evidence"] = e;
    }
    h.call(f, who, "task_report", a, now)
}

fn invalid(r: &Reply, needle: &str) -> bool {
    matches!(r, Reply::Error("invalid", m) if m.contains(needle))
}

#[test]
fn done_reports_need_evidence_fallow_checks() {
    // FR-023, FR-024 against a real repository.
    let (d, c0, c1, c2) = repo("check");
    let (mut h, f, now) = setup(&d, &c0, 2);
    let t = hand(&mut h, &f, 2, "api", now);
    let r = report(&mut h, &f, 2, t, None, now);
    assert!(invalid(&r, "needs evidence"), "{r:?}");
    let r = report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": c2, "files": ["b.txt"]})),
        now,
    );
    assert!(invalid(&r, "not on your branch feat"), "{r:?}");
    let r = report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": "deadbeef", "files": []})),
        now,
    );
    assert!(invalid(&r, "does not exist"), "{r:?}");
    let many: Vec<Value> = (0..21)
        .map(|i| json!({"command": format!("c{i}"), "result": "ok"}))
        .collect();
    let r = report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": c1, "commands": many})),
        now,
    );
    assert!(invalid(&r, "at most 20"), "{r:?}");
    let files: Vec<String> = (0..201).map(|i| format!("f{i}")).collect();
    let r = report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": c1, "files": files})),
        now,
    );
    assert!(invalid(&r, "at most 200"), "{r:?}");
    // Claimed files that differ from git are noted, not refused.
    let r = report(
        &mut h,
        &f,
        2,
        t,
        Some(
            json!({"commit": c1, "commands": [{"command": "cargo test", "result": "ok"}], "files": ["a.txt", "z.txt"]}),
        ),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let verified = v["verified"].to_string();
    assert!(verified.contains("on feat"), "{verified}");
    assert!(verified.contains("not changed: z.txt"), "{verified}");
    // The evidence goes to the next role with its task for the same part.
    let rt = hand(&mut h, &f, 3, "api", now);
    let typed = h
        .deliveries
        .list
        .iter()
        .find(|d| {
            d.to == 3
                && matches!(d.purpose, fallow::orchestration::delivery::Purpose::Task(x) if x == rt)
        })
        .unwrap()
        .text
        .clone();
    assert!(typed.contains(&format!("Evidence from T{t}")), "{typed}");
    assert!(typed.contains("cargo test"), "{typed}");
    // task_list shows it.
    let Reply::Done(v) = h.call(&f, 1, "task_list", json!({"mine": false}), now) else {
        panic!()
    };
    assert_eq!(v["tasks"][0]["part"], "api");
    assert_eq!(v["tasks"][0]["owner"], 2);
    assert_eq!(v["tasks"][0]["evidence"]["commit"], c1.as_str());
    // A read-only reviewer may name a commit it did not make (the one it reviewed).
    let r = report(
        &mut h,
        &f,
        3,
        rt,
        Some(json!({"commit": c1, "files": []})),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn failures_count_rounds_and_the_lead_hands_out_repairs_until_the_cap() {
    // FR-025 with 004 run 2: Fallow counts and tells the lead; the lead hands out the repair.
    let (d, c0, c1, _) = repo("repair");
    let (mut h, f, now) = setup(&d, &c0, 2);
    let t = hand(&mut h, &f, 2, "api", now);
    report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": c1, "files": ["a.txt"]})),
        now,
    );
    let lead_create = |h: &mut Hub| {
        h.call(
            &f,
            1,
            "task_create",
            json!({"text": "fix it", "target": {"session": 2}, "part": "api"}),
            now,
        )
    };
    let lead_lines = |h: &Hub| -> String { h.burst_of(1).unwrap().lead_updates.join("\n") };
    // Round 1: told to the lead with the owner; Fallow creates no task itself.
    let before = h.tasks.list.len();
    h.repair(&f, 1, Some("api".into()), "tests fail", now);
    assert_eq!(h.tasks.list.len(), before, "no task from Fallow");
    assert!(
        lead_lines(&h).contains("Repair needed (round 1 of 2) for api: tests fail"),
        "{}",
        lead_lines(&h)
    );
    assert!(lead_lines(&h).contains("hand it to #2 with task_create and part \"api\""));
    let Reply::Done(v) = lead_create(&mut h) else {
        panic!()
    };
    let r1 = v["task"].as_u64().unwrap() as u32;
    assert_eq!(h.tasks.get(r1).unwrap().repair_round, 1);
    // An ordinary follow-up for the part without a failure is not a round.
    let Reply::Done(v) = lead_create(&mut h) else {
        panic!()
    };
    assert_eq!(
        h.tasks
            .get(v["task"].as_u64().unwrap() as u32)
            .unwrap()
            .repair_round,
        0
    );
    // Round 2, then the cap: the third failure goes to the user; tasks for the part wait.
    h.repair(&f, 1, Some("api".into()), "still failing", now);
    let Reply::Done(v) = lead_create(&mut h) else {
        panic!()
    };
    assert_eq!(
        h.tasks
            .get(v["task"].as_u64().unwrap() as u32)
            .unwrap()
            .repair_round,
        2
    );
    h.repair(&f, 1, Some("api".into()), "failing again", now);
    assert!(
        h.alerts
            .iter()
            .any(|n| n.contains("Repair cap reached for api"))
    );
    let r = lead_create(&mut h);
    assert!(
        matches!(r, Reply::Error("not_allowed", ref m) if m.contains("the user decides first")),
        "{r:?}"
    );
    // "Once more": round 3 is allowed and counted.
    h.repair_decide(&f, 1, 0, Decision::Again, now).unwrap();
    assert!(lead_lines(&h).contains("The user allows one more repair round for api"));
    let Reply::Done(v) = lead_create(&mut h) else {
        panic!()
    };
    assert_eq!(
        h.tasks
            .get(v["task"].as_u64().unwrap() as u32)
            .unwrap()
            .repair_round,
        3
    );
    // A review asking for changes without a part is the whole work's: the lead decides who.
    let r = h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-changes-requested", "message": "naming"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)));
    h.tick(&f, now);
    let all = updates_to_lead(&h);
    assert!(all.contains("for (whole work)"), "{all}");
    assert!(all.contains("asked for changes: naming"), "{all}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Everything typed into the lead so far.
fn updates_to_lead(h: &Hub) -> String {
    h.deliveries
        .list
        .iter()
        .filter(|d| d.to == 1)
        .map(|d| d.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn cancelling_a_stopped_part_cancels_its_open_tasks() {
    let (d, c0, _, _) = repo("cancel");
    let (mut h, f, now) = setup(&d, &c0, 0);
    let t = hand(&mut h, &f, 2, "api", now);
    h.repair(&f, 1, Some("api".into()), "broken", now);
    assert!(
        h.burst_of(1).unwrap().repairs[0].stopped,
        "cap 0 stops at once"
    );
    h.repair_decide(&f, 1, 0, Decision::Cancel, now).unwrap();
    assert!(matches!(
        h.tasks.get(t).unwrap().state,
        tasks::State::Cancelled(_)
    ));
    assert!(
        h.repair_decide(&f, 1, 0, Decision::Again, now).is_err(),
        "decided once"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_lead_hears_when_everyone_waits() {
    // FR-026, research R-7: once after wait_notice_min, then at most every 30 minutes; told to
    // the lead (004 US7).
    let (d, c0, _, _) = repo("wait");
    let (mut h, f, now) = setup(&d, &c0, 2);
    let to_lead = |h: &Hub| -> Vec<String> {
        h.deliveries
            .list
            .iter()
            .filter(|d| d.to == 1 && d.from == fallow::orchestration::Party::Fallow)
            .map(|d| d.text.clone())
            .collect()
    };
    let count = |h: &Hub| {
        to_lead(h)
            .iter()
            .filter(|n| n.contains("have waited"))
            .count()
    };
    h.tick(&f, now);
    h.tick(&f, now + Duration::from_secs(9 * 60));
    assert_eq!(count(&h), 0);
    h.tick(&f, now + Duration::from_secs(10 * 60));
    assert_eq!(count(&h), 1);
    let texts = to_lead(&h);
    let n = texts.iter().find(|n| n.contains("have waited")).unwrap();
    assert!(
        n.contains("All 3 sessions") && n.contains("#2 builder is idle"),
        "{n}"
    );
    h.tick(&f, now + Duration::from_secs(20 * 60));
    assert_eq!(count(&h), 1, "not again within 30 minutes");
    h.tick(&f, now + Duration::from_secs(41 * 60));
    assert_eq!(count(&h), 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_creator_is_told_when_its_task_is_reported() {
    // 004 manual run: an idle lead never read the builder's report.
    let (d, c0, c1, _) = repo("told");
    let (mut h, f, now) = setup(&d, &c0, 2);
    let t = hand(&mut h, &f, 2, "hello", now);
    report(
        &mut h,
        &f,
        2,
        t,
        Some(json!({"commit": c1, "files": ["a.txt"]})),
        now,
    );
    h.tick(&f, now);
    let to_lead: Vec<String> = h
        .deliveries
        .list
        .iter()
        .filter(|d| d.to == 1 && d.from == fallow::orchestration::Party::Fallow)
        .map(|d| d.text.clone())
        .collect();
    let msg = to_lead
        .iter()
        .find(|m| {
            m.contains(&format!(
                "Task T{t} (part hello) was reported done by #2 builder"
            ))
        })
        .unwrap_or_else(|| panic!("{to_lead:?}"));
    assert!(msg.contains("Evidence: commit"), "{msg}");
    assert!(
        h.ready()
            .iter()
            .any(|id| h.deliveries.get(*id).unwrap().text == *msg),
        "approved"
    );
    let _ = std::fs::remove_dir_all(&d);
}
