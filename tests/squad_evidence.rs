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
    let mut s = squad::builtin();
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
fn failures_go_back_to_the_owner_until_the_cap() {
    // FR-025, FR-025a.
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
    let repairs = |h: &Hub| -> Vec<(u32, u8)> {
        h.tasks
            .list
            .iter()
            .filter(|t| t.repair_round > 0)
            .map(|t| (t.assigned.unwrap_or(0), t.repair_round))
            .collect()
    };
    // Tied to the part: its owner (#2).
    h.repair(&f, 1, Some("api".into()), "tests fail", now);
    h.tick(&f, now);
    assert_eq!(repairs(&h), [(2, 1)]);
    let open = h.tasks.list.last().unwrap();
    assert_eq!(open.part.as_deref(), Some("api"));
    assert!(open.text.contains("Repair round 1 of 2"), "{}", open.text);
    let id = open.id;
    report(
        &mut h,
        &f,
        2,
        id,
        Some(json!({"commit": c1, "files": ["a.txt"]})),
        now,
    );
    h.repair(&f, 1, Some("api".into()), "still failing", now);
    h.tick(&f, now);
    assert_eq!(repairs(&h).len(), 2);
    let id = h.tasks.list.last().unwrap().id;
    report(
        &mut h,
        &f,
        2,
        id,
        Some(json!({"commit": c1, "files": ["a.txt"]})),
        now,
    );
    // The third failure goes to the engineer.
    h.repair(&f, 1, Some("api".into()), "failing again", now);
    assert_eq!(repairs(&h).len(), 2, "no third repair task");
    assert!(
        h.notices
            .iter()
            .any(|n| n.contains("Repair cap reached for api"))
    );
    let l = &h.burst_of(1).unwrap().repairs[0];
    assert!(l.stopped && l.rounds == 2);
    // "Once more": one more round.
    h.repair_decide(&f, 1, 0, Decision::Again, now).unwrap();
    h.tick(&f, now);
    assert_eq!(repairs(&h).len(), 3);
    // A review asking for changes without a part goes to the lead.
    let r = h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-changes-requested", "message": "naming"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)));
    h.tick(&f, now);
    let last = h.tasks.list.last().unwrap();
    assert_eq!(last.target, tasks::Target::Session(1));
    assert!(
        last.text.contains("asked for changes: naming"),
        "{}",
        last.text
    );
    let _ = std::fs::remove_dir_all(&d);
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
fn the_engineer_hears_when_everyone_waits() {
    // FR-026, research R-7: once after wait_notice_min, then at most every 30 minutes.
    let (d, c0, _, _) = repo("wait");
    let (mut h, f, now) = setup(&d, &c0, 2);
    let count = |h: &Hub| {
        h.notices
            .iter()
            .filter(|n| n.contains("have waited"))
            .count()
    };
    h.tick(&f, now);
    h.tick(&f, now + Duration::from_secs(9 * 60));
    assert_eq!(count(&h), 0);
    h.tick(&f, now + Duration::from_secs(10 * 60));
    assert_eq!(count(&h), 1);
    let n = h
        .notices
        .iter()
        .find(|n| n.contains("have waited"))
        .unwrap();
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
