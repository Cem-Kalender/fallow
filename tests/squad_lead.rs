//! 004 T042: the lead drives the squad (US7, FR-032 to FR-038).

use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{
    Hub, Party, Reply, SessionInfo, Sessions, WAITING_FOR_APPROVAL, squad,
};
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
        state: "needs attention".into(),
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

/// Lead #1, builders #2 and #3, reviewer #4.
fn setup() -> (Hub, Fake, Instant) {
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1)]);
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(4 * 3600),
            allowance: Some(5),
            squad: squad::builtin(),
            start_commit: None,
            target_branch: Some("main".into()),
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    for (id, role) in [(2, "builder"), (3, "builder"), (4, "reviewer")] {
        let r = h.call(
            &f,
            1,
            "session_start",
            json!({"profile": "claude", "project": "api", "role": role}),
            now,
        );
        let Reply::Done(v) = r else { panic!("{r:?}") };
        f.0.push(info(id));
        h.start_done(&f, v["start"].as_u64().unwrap(), Ok(id), now);
    }
    (h, f, now)
}

/// Squad updates typed (or to be typed) into the lead, in order.
fn updates(h: &Hub) -> Vec<String> {
    h.deliveries
        .list
        .iter()
        .filter(|d| d.to == 1 && d.from == Party::Fallow && d.text.starts_with("Squad update:"))
        .map(|d| d.text.clone())
        .collect()
}

fn hand(h: &mut Hub, f: &Fake, to: u32, now: Instant) -> u64 {
    let r = h.call(
        f,
        1,
        "task_create",
        json!({"text": "build it", "target": {"session": to}, "part": format!("p{to}")}),
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

#[test]
fn reports_reach_the_lead_grouped_and_only_while_it_is_idle() {
    let (mut h, mut f, now) = setup();
    let t2 = hand(&mut h, &f, 2, now);
    let t3 = hand(&mut h, &f, 3, now);
    // The lead is busy: nothing is typed, nothing is lost.
    f.0[0].idle = false;
    done(&mut h, &f, 2, t2, now);
    done(&mut h, &f, 3, t3, now);
    h.tick(&f, now);
    assert!(updates(&h).is_empty(), "not while the lead works");
    f.0[0].idle = true;
    h.tick(&f, now);
    let u = updates(&h);
    assert_eq!(u.len(), 1, "one grouped update: {u:?}");
    assert!(u[0].contains(&format!(
        "Task T{t2} (part p2) was reported done by #2 builder"
    )));
    assert!(u[0].contains(&format!(
        "Task T{t3} (part p3) was reported done by #3 builder"
    )));
    assert!(
        u[0].contains("Merge still needs: signal:review-approved@reviewer"),
        "{}",
        u[0]
    );
    // Approved as it is made: typed without the engineer.
    assert!(!h.ready().is_empty());
    // At most one update every 10 s.
    let r = h.call(
        &f,
        4,
        "review_verdict",
        json!({"verdict": "approved", "notes": "fine"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)));
    h.tick(&f, now + Duration::from_secs(5));
    assert_eq!(updates(&h).len(), 1);
    h.tick(&f, now + Duration::from_secs(11));
    let u = updates(&h);
    assert_eq!(u.len(), 2);
    assert!(u[1].contains("#4 reviewer approved: fine"), "{}", u[1]);
    // The merge is ready: the lead is told it is finished, and the engineer is alerted.
    assert!(u[1].contains("Every merge condition holds"), "{}", u[1]);
    assert!(
        h.alerts.iter().any(|a| a.contains("ready to merge")),
        "{:?}",
        h.alerts
    );
}

#[test]
fn repairs_fallow_creates_are_reported_to_the_lead() {
    // The manual-run finding: a repair task's report reached nobody.
    let (mut h, f, now) = setup();
    let t2 = hand(&mut h, &f, 2, now);
    done(&mut h, &f, 2, t2, now);
    h.call(
        &f,
        4,
        "review_verdict",
        json!({"verdict": "changes-requested", "notes": "add a test", "part": "p2"}),
        now,
    );
    h.tick(&f, now);
    let repair = h
        .tasks
        .list
        .iter()
        .find(|t| t.repair_round == 1)
        .unwrap()
        .id;
    h.tick(&f, now + Duration::from_secs(11));
    done(&mut h, &f, 2, u64::from(repair), now);
    h.tick(&f, now + Duration::from_secs(22));
    let all = updates(&h).join("\n");
    assert!(
        all.contains("#4 reviewer asks for changes: add a test"),
        "{all}"
    );
    assert!(all.contains("Repair round 1 for p2 goes to #2"), "{all}");
    assert!(
        all.contains(&format!(
            "Task T{repair} (part p2) was reported done by #2 builder"
        )),
        "{all}"
    );
    // The engineer is not told about routine events.
    assert!(h.alerts.is_empty(), "{:?}", h.alerts);
}

#[test]
fn a_member_at_a_permission_prompt_and_a_quiet_squad_reach_the_lead() {
    let (mut h, mut f, now) = setup();
    hand(&mut h, &f, 2, now);
    f.0[2].state = WAITING_FOR_APPROVAL.into();
    h.tick(&f, now + Duration::from_secs(11));
    h.tick(&f, now + Duration::from_secs(22));
    let all = updates(&h).join("\n");
    assert_eq!(all.matches("#3 builder is stuck").count(), 1, "once: {all}");
    // Ten quiet minutes with open work: a status.
    let n = updates(&h).len();
    h.tick(&f, now + Duration::from_secs(22 + 601));
    let u = updates(&h);
    assert_eq!(u.len(), n + 1);
    assert!(
        u.last().unwrap().contains("Status: open tasks: T1"),
        "{}",
        u.last().unwrap()
    );
}

#[test]
fn others_consult_the_lead_and_without_a_lead_the_engineer_hears_it() {
    let (mut h, mut f, now) = setup();
    let r = h.call(
        &f,
        2,
        "consult_lead",
        json!({"question": "Python or Rust?"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    let q = h
        .deliveries
        .list
        .iter()
        .find(|d| d.to == 1 && d.text.starts_with("Question from #2 builder"))
        .unwrap();
    assert!(
        q.text.contains("Python or Rust?") && q.text.contains("deliver to session 2"),
        "{}",
        q.text
    );
    assert!(h.ready().contains(&q.id), "typed without the engineer");
    let r = h.call(&f, 1, "consult_lead", json!({"question": "?"}), now);
    assert!(
        matches!(r, Reply::Error(..)),
        "the lead asks the user itself: {r:?}"
    );
    // The lead exited: updates go to the engineer.
    f.0[0].running = false;
    let t = hand(&mut h, &f, 2, now);
    let _ = t;
    h.call(
        &f,
        4,
        "review_verdict",
        json!({"verdict": "approved", "notes": "ok"}),
        now,
    );
    h.tick(&f, now + Duration::from_secs(11));
    assert!(
        h.notices.iter().any(|n| n.contains("#4 reviewer approved")),
        "{:?}",
        h.notices
    );
}
