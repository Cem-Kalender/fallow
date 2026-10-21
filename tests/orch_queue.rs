//! 003 T039: the task queue (US4, FR-018 to FR-020, data-model.md "Task").

use std::cell::RefCell;
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::board::BoardId;
use fallow::orchestration::delivery::{Purpose, State as DState};
use fallow::orchestration::identity::Peer;
use fallow::orchestration::tasks::{State, Target};
use fallow::orchestration::{Hub, Party, Reply, SessionInfo, Sessions};
use serde_json::{Value, json};

/// #1, #2 (Claude Code) and #3 (Codex) in project 1 "api"; #4 in project 2 "web". Idle unless
/// set busy.
struct Fake(RefCell<Vec<SessionInfo>>);

impl Fake {
    fn new() -> Fake {
        let s = |id, project: u32, name: &str, profile: &str| SessionInfo {
            id,
            project,
            project_name: name.into(),
            profile_id: profile.into(),
            profile: profile.into(),
            state: "needs attention".into(),
            running: true,
            idle: true,
        };
        Fake(RefCell::new(vec![
            s(1, 1, "api", "claude"),
            s(2, 1, "api", "claude"),
            s(3, 1, "api", "codex"),
            s(4, 2, "web", "claude"),
        ]))
    }

    fn set(&self, id: u32, f: impl Fn(&mut SessionInfo)) {
        if let Some(s) = self.0.borrow_mut().iter_mut().find(|s| s.id == id) {
            f(s);
        }
    }
}

impl Sessions for Fake {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.0.borrow().iter().find(|s| s.id == id).cloned()
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.0.borrow().clone()
    }
    fn verify(&self, _: u32, _: &str, _: Peer) -> Result<(), String> {
        Ok(())
    }
}

fn hub() -> Hub {
    Hub::new(OrchestrationSettings::default())
}

fn user_task(h: &mut Hub, f: &Fake, text: &str, target: Target, pre: &[&str]) -> u32 {
    let pre = pre
        .iter()
        .map(|n| (BoardId::Project(1), n.to_string()))
        .collect();
    h.create_task(f, Party::User, text.into(), target, pre, Instant::now())
        .unwrap()
        .0
}

fn state(h: &Hub, id: u32) -> State {
    h.tasks.get(id).unwrap().state.clone()
}

/// Simulates the typing loop: every approved hand-over is typed.
fn type_all(h: &mut Hub) {
    for id in h.ready() {
        h.delivered(id, Ok(()));
    }
}

fn done(r: Reply) -> Value {
    match r {
        Reply::Done(v) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn tasks_run_in_order_one_per_session_and_wait_for_preconditions() {
    // US4 independent test: three tasks for #2, the second with a precondition.
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    let t1 = user_task(&mut h, &f, "write the API", Target::Session(2), &[]);
    let t2 = user_task(
        &mut h,
        &f,
        "merge feature/x",
        Target::Session(2),
        &["deploy-ok"],
    );
    let t3 = user_task(&mut h, &f, "write the changelog", Target::Session(2), &[]);
    assert_eq!(state(&h, t1), State::Queued, "US4 scenario 1");
    h.tick(&f, now);
    assert_eq!(state(&h, t1), State::InProgress);
    assert_eq!(state(&h, t2), State::Blocked, "US4 scenario 3");
    assert_eq!(
        state(&h, t3),
        State::Queued,
        "one task per session at a time"
    );
    // The engineer's task is typed without a prompt (their own action).
    let d = &h.deliveries.list[0];
    assert_eq!(d.purpose, Purpose::Task(t1));
    assert_eq!(d.state, DState::Approved);
    type_all(&mut h);
    // #2 reports done: t2 is still blocked, so t3 goes next.
    done(h.call(
        &f,
        2,
        "task_report",
        json!({"task": t1, "outcome": "done", "note": "ok"}),
        now,
    ));
    assert_eq!(state(&h, t1), State::Done);
    h.tick(&f, now);
    assert_eq!(state(&h, t2), State::Blocked);
    assert_eq!(state(&h, t3), State::InProgress);
    type_all(&mut h);
    done(h.call(
        &f,
        2,
        "task_report",
        json!({"task": t3, "outcome": "failed", "note": "no access"}),
        now,
    ));
    assert_eq!(state(&h, t3), State::Failed("no access".into()));
    // The event arrives: t2 goes.
    h.signal(
        &(BoardId::Project(1), "deploy-ok".into()),
        Party::User,
        None,
    )
    .unwrap();
    h.tick(&f, now);
    assert_eq!(state(&h, t2), State::InProgress);
    assert_eq!(h.queue_counts(), (0, 0));
}

#[test]
fn only_free_sessions_get_tasks() {
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    f.set(2, |s| s.idle = false);
    let t = user_task(&mut h, &f, "x", Target::Session(2), &[]);
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::Queued, "busy");
    f.set(2, |s| s.idle = true);
    // Waiting for an event is not free either.
    h.call_as(&f, 2, 9, "event_wait", json!({"name": "e"}), now);
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::Queued, "waiting");
    h.session_ended(2);
    f.set(2, |s| s.running = false);
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::Queued, "not running");
}

#[test]
fn next_free_targets_pick_a_matching_idle_session_and_never_start_one() {
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    let next = || Target::NextFree {
        profile: "codex".into(),
        project: 1,
        project_name: "api".into(),
    };
    let t = user_task(&mut h, &f, "lint", next(), &[]);
    h.tick(&f, now);
    assert_eq!(
        h.tasks.get(t).unwrap().assigned,
        Some(3),
        "the codex session in api"
    );
    // Another codex task waits: no free codex session (US4 scenario 7).
    let t2 = user_task(&mut h, &f, "lint again", next(), &[]);
    h.tick(&f, now);
    assert_eq!(state(&h, t2), State::Queued);
    // The target exits while in progress: a "next free" task goes back to the queue.
    type_all(&mut h);
    h.session_ended(3);
    assert_eq!(state(&h, t), State::Queued);
    // A task for a specific session fails instead.
    let t3 = user_task(&mut h, &f, "x", Target::Session(1), &[]);
    h.tick(&f, now);
    type_all(&mut h);
    h.session_ended(1);
    assert_eq!(state(&h, t3), State::Failed("target exited".into()));
}

#[test]
fn agents_tasks_need_approval_or_a_create_task_permission() {
    // US4 scenario 5.
    use fallow::orchestration::permissions::{Grant, Scope};
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    let r = done(h.call(
        &f,
        1,
        "task_create",
        json!({"text": "write a changelog entry", "target": {"session": 2}}),
        now,
    ));
    assert_eq!(r["state"], "pending_approval");
    let t = r["task"].as_u64().unwrap() as u32;
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::PendingApproval, "not handed over");
    assert_eq!(h.pending_tasks().count(), 1);
    h.decide_task(t, true);
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::InProgress);
    // An agent's hand-over is a delivery like any other: it waits for approval.
    let d = h.deliveries.list.last().unwrap();
    assert_eq!(d.state, DState::Pending);
    assert!(h.ready().is_empty());
    // Rejecting the hand-over pauses the task instead of offering it again at once.
    let did = d.id;
    h.reject(did);
    assert_eq!(state(&h, t), State::Queued);
    assert!(h.tasks.get(t).unwrap().paused);
    h.tick(&f, now);
    assert_eq!(state(&h, t), State::Queued, "paused tasks are skipped");
    // A permission to create tasks for #2 lets the next one in without approval.
    h.grant(
        &Grant {
            source: Scope::Session(1),
            target: Scope::Session(2),
            deliver: false,
            create_task: true,
            start_limit: None,
            duration: Duration::from_secs(600),
        },
        now,
    )
    .unwrap();
    let r = done(h.call(
        &f,
        1,
        "task_create",
        json!({"text": "another", "target": {"session": 2}}),
        now,
    ));
    assert_eq!(r["state"], "queued");
    // Rejected requests are cancelled and never handed over.
    let r = done(h.call(
        &f,
        3,
        "task_create",
        json!({"text": "nope", "target": {"session": 2}}),
        now,
    ));
    let t4 = r["task"].as_u64().unwrap() as u32;
    h.decide_task(t4, false);
    assert_eq!(state(&h, t4), State::Cancelled("rejected".into()));
    let inbox = done(h.call(&f, 3, "inbox", json!({}), now));
    assert_eq!(inbox["tasks"][0]["state"], "cancelled (rejected)");
}

#[test]
fn reports_edits_cancels_and_limits() {
    let f = Fake::new();
    let mut h = hub();
    let now = Instant::now();
    let t = user_task(&mut h, &f, "x", Target::Session(2), &[]);
    h.tick(&f, now);
    // Only the assigned session may report.
    assert!(matches!(
        h.call(
            &f,
            1,
            "task_report",
            json!({"task": t, "outcome": "done"}),
            now
        ),
        Reply::Error("not_allowed", _)
    ));
    assert!(matches!(
        h.call(
            &f,
            2,
            "task_report",
            json!({"task": t, "outcome": "maybe"}),
            now
        ),
        Reply::Error("invalid", _)
    ));
    assert!(matches!(
        h.call(
            &f,
            2,
            "task_report",
            json!({"task": t, "outcome": "done", "note": "x".repeat(2049)}),
            now
        ),
        Reply::Error("too_large", _)
    ));
    // The assigned session learns its task through inbox too.
    let inbox = done(h.call(&f, 2, "inbox", json!({}), now));
    assert_eq!(inbox["tasks"][0]["task"], t);
    assert_eq!(inbox["tasks"][0]["text"]["from"], json!({"kind": "user"}));
    // Edit, reorder, pause, cancel (US4 scenario 6).
    let a = user_task(&mut h, &f, "a", Target::Session(4), &["never"]);
    let b = user_task(&mut h, &f, "b", Target::Session(4), &["never"]);
    h.edit_task(a, "a, edited".into()).unwrap();
    assert_eq!(h.tasks.get(a).unwrap().text, "a, edited");
    h.tasks.move_by(b, -1);
    let order: Vec<u32> = h.tasks.list.iter().map(|t| t.id).collect();
    assert!(order.iter().position(|i| *i == b) < order.iter().position(|i| *i == a));
    h.toggle_pause(a);
    assert!(h.tasks.get(a).unwrap().paused);
    h.cancel_task(b);
    h.signal(&(BoardId::Project(1), "never".into()), Party::User, None)
        .unwrap();
    h.tick(&f, now);
    assert_eq!(
        state(&h, b),
        State::Cancelled("by the user".into()),
        "never handed over"
    );
    // Validation.
    assert!(matches!(
        h.call(
            &f,
            1,
            "task_create",
            json!({"text": "x", "target": {"profile": "claude", "project": "nowhere"}}),
            now
        ),
        Reply::Error("not_found", _)
    ));
    assert!(matches!(
        h.call(
            &f,
            1,
            "task_create",
            json!({"text": "x", "target": {"session": 99}}),
            now
        ),
        Reply::Error("not_found", _)
    ));
    assert!(matches!(
        h.call(
            &f,
            1,
            "task_create",
            json!({"text": "x", "target": {}}),
            now
        ),
        Reply::Error("invalid", _)
    ));
    let many: Vec<Value> = (0..9).map(|i| json!({"name": format!("e{i}")})).collect();
    assert!(matches!(
        h.call(
            &f,
            1,
            "task_create",
            json!({"text": "x", "target": {"session": 2}, "preconditions": many}),
            now
        ),
        Reply::Error("invalid", _)
    ));
    // task_list: mine by default, everything with mine = false.
    let mine = done(h.call(&f, 2, "task_list", json!({}), now));
    assert_eq!(mine["tasks"].as_array().unwrap().len(), 1);
    let all = done(h.call(&f, 2, "task_list", json!({"mine": false}), now));
    assert!(all["tasks"].as_array().unwrap().len() >= 3);
}

#[test]
fn unapproved_agent_tasks_expire() {
    let f = Fake::new();
    let mut h = hub();
    let t0 = Instant::now();
    let r = done(h.call(
        &f,
        1,
        "task_create",
        json!({"text": "x", "target": {"session": 2}}),
        t0,
    ));
    let t = r["task"].as_u64().unwrap() as u32;
    h.tick(&f, t0 + Duration::from_secs(600));
    assert_eq!(state(&h, t), State::Cancelled("expired".into()));
}
