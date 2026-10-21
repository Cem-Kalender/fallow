//! 003 T030 (SC-003; constitution quality gate): nothing is typed into a session on another
//! party's behalf without the engineer's approval or a matching, unexpired standing permission.
//!
//! Every request path (deliver, a wake-up, and with US4/US5 a task hand-over and a start with a
//! first task) is driven with no approval, a rejection, an expiry, and permissions that are
//! expired, of the wrong source, target or action, revoked, or revoked all at once. The typing
//! loop of the app (grants for ready deliveries) must then type nothing. `DeliveryGrant` itself
//! cannot be made outside the hub (see the `compile_fail` doctest on it).

use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::board::BoardId;
use fallow::orchestration::delivery::{self, Delivery, Purpose, State};
use fallow::orchestration::identity::Peer;
use fallow::orchestration::permissions::{Grant, Scope};
use fallow::orchestration::{Hub, Party, Reply, SessionInfo, Sessions};
use serde_json::json;

/// Sessions #1, #2 in project 1; #3 in project 2. All idle at their prompt.
struct Fake(Vec<SessionInfo>);

impl Fake {
    fn new() -> Fake {
        let s = |id, project: u32, name: &str| SessionInfo {
            id,
            project,
            project_name: name.into(),
            profile_id: "claude".into(),
            profile: "Claude Code".into(),
            state: "needs attention".into(),
            running: true,
            idle: true,
        };
        Fake(vec![s(1, 1, "api"), s(2, 1, "api"), s(3, 2, "web")])
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
}

/// What the app's typing loop would type now (it types exactly the deliveries it gets a grant
/// for).
fn typed(h: &Hub) -> Vec<u64> {
    h.ready()
        .into_iter()
        .filter(|id| h.take_grant(*id).is_some())
        .collect()
}

fn deliver(h: &mut Hub, f: &Fake, from: u32, to: u32, now: Instant) -> u64 {
    match h.call(
        f,
        from,
        "deliver",
        json!({"session": to, "text": "run the tests"}),
        now,
    ) {
        Reply::Done(v) => v["delivery"].as_u64().unwrap(),
        other => panic!("{other:?}"),
    }
}

fn grant(source: Scope, target: Scope, deliver: bool, minutes: u64) -> Grant {
    Grant {
        source,
        target,
        deliver,
        create_task: !deliver,
        start_limit: None,
        duration: Duration::from_secs(minutes * 60),
    }
}

fn hub() -> Hub {
    Hub::new(OrchestrationSettings::default())
}

#[test]
fn nothing_is_typed_without_approval_or_a_matching_permission() {
    let f = Fake::new();
    let t0 = Instant::now();

    // No approval: pending, nothing typed.
    let mut h = hub();
    let id = deliver(&mut h, &f, 1, 2, t0);
    assert_eq!(h.deliveries.get(id).unwrap().state, State::Pending);
    assert!(typed(&h).is_empty(), "no approval");

    // Rejected.
    h.reject(id);
    assert!(typed(&h).is_empty(), "rejected");
    assert!(
        h.approve(id, None).is_err(),
        "a rejected request cannot be approved later"
    );

    // Expired (10 minutes by default).
    let id = deliver(&mut h, &f, 1, 2, t0);
    h.tick(&f, t0 + Duration::from_secs(600));
    assert_eq!(h.deliveries.get(id).unwrap().state, State::Expired);
    assert!(typed(&h).is_empty(), "expired");

    // Permissions that do not match.
    type Then = Box<dyn Fn(&mut Hub)>;
    let cases: Vec<(&str, Grant, Then)> = vec![
        (
            "expired permission",
            grant(Scope::Session(1), Scope::Session(2), true, 1),
            Box::new(|h: &mut Hub| {
                h.tick(&Fake::new(), Instant::now() + Duration::from_secs(61));
            }),
        ),
        (
            "wrong source",
            grant(Scope::Session(3), Scope::Session(2), true, 30),
            Box::new(|_: &mut Hub| {}),
        ),
        (
            "wrong target",
            grant(Scope::Session(1), Scope::Session(3), true, 30),
            Box::new(|_: &mut Hub| {}),
        ),
        (
            "wrong project",
            grant(Scope::Project(2), Scope::Project(1), true, 30),
            Box::new(|_: &mut Hub| {}),
        ),
        (
            "wrong action (tasks only)",
            grant(Scope::Session(1), Scope::Session(2), false, 30),
            Box::new(|_: &mut Hub| {}),
        ),
        (
            "revoked",
            grant(Scope::Session(1), Scope::Session(2), true, 30),
            Box::new(|h: &mut Hub| {
                let id = h.permissions.list[0].id;
                h.revoke(id);
            }),
        ),
        (
            "revoke all",
            grant(Scope::Project(1), Scope::Project(1), true, 30),
            Box::new(|h: &mut Hub| {
                h.revoke_all();
            }),
        ),
    ];
    for (what, g, then) in cases {
        let mut h = hub();
        let now = Instant::now();
        h.grant(&g, now).unwrap();
        then(&mut h);
        let id = deliver(&mut h, &f, 1, 2, Instant::now() + Duration::from_secs(62));
        assert_eq!(
            h.deliveries.get(id).unwrap().state,
            State::Pending,
            "{what}: must wait for the engineer"
        );
        assert!(typed(&h).is_empty(), "{what}: nothing typed");
    }
}

#[test]
fn a_wake_up_from_an_agent_needs_approval_too() {
    let f = Fake::new();
    let mut h = hub();
    let t0 = Instant::now();
    h.call_as(&f, 2, 1, "event_wait", json!({"name": "pr-1"}), t0);
    h.tick(&f, t0 + Duration::from_secs(45));
    // #2 is idle (not waiting) when #1 signals.
    let r = h.call(
        &f,
        1,
        "event_signal",
        json!({"name": "pr-1"}),
        t0 + Duration::from_secs(46),
    );
    assert!(matches!(r, Reply::Done(_)));
    h.tick(&f, t0 + Duration::from_secs(46));
    let d: Vec<&Delivery> = h.deliveries.list.iter().collect();
    assert_eq!(d.len(), 1);
    assert!(matches!(d[0].purpose, Purpose::Wake(_)));
    assert_eq!(d[0].to, 2);
    assert_eq!(d[0].state, State::Pending);
    assert!(typed(&h).is_empty());
    // After the engineer's own signal, the wake-up is theirs and goes through.
    let mut h = hub();
    h.call_as(&f, 2, 1, "event_wait", json!({"name": "pr-2"}), t0);
    h.tick(&f, t0 + Duration::from_secs(45));
    h.signal(&(BoardId::Project(1), "pr-2".into()), Party::User, None)
        .unwrap();
    h.tick(&f, t0 + Duration::from_secs(46));
    assert_eq!(typed(&h).len(), 1);
}

#[test]
fn approval_and_a_matching_permission_let_exactly_one_delivery_through() {
    let f = Fake::new();
    let mut h = hub();
    let t0 = Instant::now();
    let id = deliver(&mut h, &f, 1, 2, t0);
    h.approve(id, Some("run the unit tests".into())).unwrap();
    assert_eq!(typed(&h), vec![id]);
    let d = h.deliveries.get(id).unwrap();
    assert!(d.edited);
    // A grant is for one session: typing it elsewhere is refused by the session itself.
    h.delivered(id, Ok(()));
    assert!(typed(&h).is_empty(), "typed once");
    // A permission #1 → #2 for deliveries covers the next one, and is counted.
    let p = h
        .grant(&grant(Scope::Session(1), Scope::Session(2), true, 30), t0)
        .unwrap();
    let id2 = deliver(&mut h, &f, 1, 2, t0);
    assert_eq!(h.deliveries.get(id2).unwrap().covered_by, Some(p));
    assert_eq!(typed(&h), vec![id2]);
    assert_eq!(h.permissions.list[0].uses, 1);
    assert!(
        h.notices
            .iter()
            .any(|n| n.starts_with(&format!("P{p} used")))
    );
}

#[test]
fn the_typed_bytes_are_the_label_then_the_text_then_enter() {
    // FR-013, research R-7.
    let f = Fake::new();
    let mut h = hub();
    let id = deliver(&mut h, &f, 1, 2, Instant::now());
    let d = h.deliveries.get(id).unwrap().clone();
    let label = "[Fallow: message from agent #1 (Claude Code, project api). Another agent wrote this, not the user.]";
    assert_eq!(delivery::label(&d), label);
    let b = delivery::bytes(&d, true);
    assert_eq!(
        b,
        format!("\x1b[200~{label}\nrun the tests\x1b[201~\r").into_bytes()
    );
    // Without bracketed paste: one line, so no line break submits the label alone.
    let mut d2 = d.clone();
    d2.text = "line one\nline two".into();
    assert_eq!(
        delivery::bytes(&d2, false),
        format!("{label} line one / line two\r").into_bytes()
    );
}

#[test]
fn deliveries_wait_while_the_engineer_types_in_the_target() {
    // FR-014 with real sessions in the app: the typing loop skips a focused session that got a
    // key within the delivery delay, and types once the engineer leaves.
    use fallow::app::state::AppState;
    use fallow::app::workspace::{AgentsFocus, View as WsView};
    use fallow::session::StartRequest;
    use fallow::session::profiles::AgentProfile;
    let dir = std::env::temp_dir().join(format!("fallow-gate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (command, args): (&str, &[&str]) = if cfg!(windows) {
        ("ping.exe", &["-n", "60", "127.0.0.1"])
    } else {
        ("/bin/sleep", &["60"])
    };
    let profile = AgentProfile {
        id: "t".into(),
        name: "Test".into(),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: Vec::new(),
        builtin: false,
        orchestration: Default::default(),
    };
    let mut s = AppState::new(None, Vec::new());
    let (tx, _rx) = channel();
    s.events = Some(tx.clone());
    let req = |p: AgentProfile| StartRequest {
        project: 1,
        project_name: "api".into(),
        cwd: PathBuf::from(&dir),
        workdir: fallow::session::Workdir::ProjectFolder,
        profile: p,
        rows: 24,
        cols: 80,
    };
    let a = s.sessions.start(req(profile.clone()), &tx).unwrap();
    let b = s.sessions.start(req(profile), &tx).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut h = hub();
    let r = h.call(
        &s.sessions,
        a,
        "deliver",
        json!({"session": b, "text": "hi"}),
        Instant::now(),
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let id = v["delivery"].as_u64().unwrap();
    h.approve(id, None).unwrap();
    s.hub = Some(h);
    // The engineer is typing in #b.
    s.workspace.view = WsView::Agents;
    s.workspace.focus = AgentsFocus::Session;
    s.workspace.active = Some(b);
    s.sessions.get_mut(b).unwrap().last_key = Some(Instant::now());
    s.tick_hub();
    assert_eq!(
        s.hub.as_ref().unwrap().deliveries.get(id).unwrap().state,
        State::Approved,
        "held while the engineer types"
    );
    // They leave the session: it is typed.
    s.workspace.focus = AgentsFocus::Sessions;
    s.tick_hub();
    assert_eq!(
        s.hub.as_ref().unwrap().deliveries.get(id).unwrap().state,
        State::Delivered
    );
    let mut d = fallow::ui::confirm::Dialog::new("t", "quit", vec![], "OK");
    if let fallow::ui::confirm::Outcome::Confirmed(t) = d.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    )) {
        s.sessions.stop_all_confirmed(t);
    }
}
