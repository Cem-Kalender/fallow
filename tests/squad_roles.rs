//! 004 T018, T021: roles enforced by Fallow (US2, FR-008 to FR-015, SC-002).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, StartState, squad, tools};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::{Value, json};

/// #1 (the lead) in project 1 ("api"); sessions the burst starts are added as they start.
struct Fake(Vec<SessionInfo>);

fn info(id: u32, profile: &str) -> SessionInfo {
    SessionInfo {
        id,
        project: 1,
        project_name: "api".into(),
        profile_id: profile.into(),
        profile: profile.into(),
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

fn burst(h: &mut Hub, f: &Fake, squad: squad::Squad, now: Instant) {
    h.start_burst(
        f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: None,
            squad,
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

/// The lead starts a session in `role`; it becomes session `id` in the fake.
fn start(h: &mut Hub, f: &mut Fake, role: &str, id: u32, now: Instant) {
    let r = h.call(
        f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": role}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    assert_eq!(v["state"], "approved", "{v}");
    f.0.push(info(id, "claude"));
    h.start_done(f, v["start"].as_u64().unwrap(), Ok(id), now);
    assert_eq!(h.role_of(id), Some(role));
}

fn setup() -> (Hub, Fake, Instant) {
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake(vec![info(1, "claude")]);
    let now = Instant::now();
    burst(&mut h, &f, squad::builtin(), now);
    start(&mut h, &mut f, "builder", 2, now);
    start(&mut h, &mut f, "reviewer", 3, now);
    (h, f, now)
}

fn names(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn refused(r: &Reply, code: &str) -> bool {
    matches!(r, Reply::Error(c, _) if *c == code)
}

#[test]
fn each_role_sees_and_may_call_only_its_tools() {
    // SC-002, contracts/mcp-tools.md "Tool list per role".
    let (mut h, f, now) = setup();
    let all = names(&tools::list());
    let set = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
    let builder = set(&[
        "whoami",
        "session_list",
        "squad_status",
        "board_post",
        "board_read",
        "event_signal",
        "event_wait",
        "event_status",
        "deliver",
        "inbox",
        "task_list",
        "task_report",
        "review_verdict",
    ]);
    let reviewer = set(&[
        "whoami",
        "session_list",
        "squad_status",
        "board_post",
        "board_read",
        "event_signal",
        "event_wait",
        "event_status",
        "inbox",
        "task_list",
        "task_report",
        "review_verdict",
    ]);
    let expect = |want: &BTreeSet<String>| -> BTreeSet<String> {
        want.intersection(&all).cloned().collect()
    };
    for (id, want) in [
        (1, all.clone()),
        (2, expect(&builder)),
        (3, expect(&reviewer)),
    ] {
        let got = names(&h.accepted(id).tools.unwrap());
        assert_eq!(got, want, "session #{id}");
        // Every other tool is refused with not_allowed, whatever the arguments.
        for t in all.difference(&got) {
            let r = h.call(&f, id, t, json!({}), now);
            assert!(refused(&r, "not_allowed"), "#{id} {t}: {r:?}");
            let Reply::Error(_, why) = r else { panic!() };
            assert!(why.contains("your role"), "{why}");
        }
    }
}

#[test]
fn restricted_events_belong_to_their_roles_and_gates_to_fallow() {
    // FR-012, FR-020, SC-004.
    let (mut h, f, now) = setup();
    let sig = |h: &mut Hub, id: u32, name: &str| {
        h.call(&f, id, "event_signal", json!({ "name": name }), now)
    };
    assert!(refused(&sig(&mut h, 2, "review-approved"), "not_allowed"));
    assert!(
        refused(&sig(&mut h, 1, "review-approved"), "not_allowed"),
        "no self-approval"
    );
    assert!(matches!(sig(&mut h, 3, "review-approved"), Reply::Done(_)));
    assert!(matches!(sig(&mut h, 2, "api-ready"), Reply::Done(_)));
    assert!(refused(&sig(&mut h, 3, "api-ready"), "not_allowed"));
    for id in [1, 2, 3] {
        assert!(
            refused(&sig(&mut h, id, "gate-tests-passed"), "not_allowed"),
            "#{id}"
        );
    }
}

#[test]
fn a_role_precondition_needs_a_signal_from_that_role() {
    // FR-012: {"name": "review-approved", "role": "reviewer"}.
    let (mut h, f, now) = setup();
    let r = h.call(
        &f,
        1,
        "task_create",
        json!({
            "text": "merge it",
            "target": {"session": 2},
            "preconditions": [{"name": "review-approved", "role": "reviewer"}],
        }),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let t = v["task"].as_u64().unwrap() as u32;
    // The engineer's signal has no role: it does not meet the condition.
    let key = (
        fallow::orchestration::board::BoardId::Project(1),
        "review-approved".to_string(),
    );
    h.signal(&key, fallow::orchestration::Party::User, None)
        .unwrap();
    h.tick(&f, now);
    assert!(h.tasks.get(t).unwrap().assigned.is_none(), "still blocked");
    // The reviewer's does.
    h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    h.tick(&f, now);
    assert_eq!(h.tasks.get(t).unwrap().assigned, Some(2));
}

#[test]
fn the_role_is_fallows_and_stamped_on_what_others_receive() {
    // FR-009, FR-010.
    let (mut h, f, now) = setup();
    let r = h.call(&f, 2, "whoami", json!({"role": "lead"}), now);
    assert!(matches!(r, Reply::Rpc(-32602, _)), "{r:?}");
    let r = h.call(&f, 2, "whoami", json!({}), now);
    let Reply::Done(v) = r else { panic!() };
    assert_eq!(v["role"], "builder");
    assert!(v["briefing"].as_str().unwrap().contains("builder"));
    h.call(&f, 2, "board_post", json!({"text": "api done"}), now);
    let Reply::Done(v) = h.call(&f, 3, "board_read", json!({}), now) else {
        panic!()
    };
    let last = v["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["from"]["role"], "builder");
    assert_eq!(last["from"]["session"], 2);
    let Reply::Done(v) = h.call(&f, 3, "session_list", json!({}), now) else {
        panic!()
    };
    let roles: Vec<&str> = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["lead", "builder", "reviewer"]);
    assert_eq!(v["roles"].as_array().unwrap().len(), 3);
}

#[test]
fn session_start_needs_a_role_within_the_squads_limits() {
    let (mut h, mut f, now) = setup();
    let ask = |h: &mut Hub, f: &Fake, args: Value| h.call(f, 1, "session_start", args, now);
    // A role is required during a burst, and must exist.
    let r = ask(&mut h, &f, json!({"profile": "claude", "project": "api"}));
    assert!(refused(&r, "invalid"), "{r:?}");
    let r = ask(
        &mut h,
        &f,
        json!({"profile": "claude", "project": "api", "role": "admin"}),
    );
    assert!(refused(&r, "invalid"), "{r:?}");
    // No flags for the level: refused, naming how to give them.
    let r = ask(
        &mut h,
        &f,
        json!({"profile": "gemini", "project": "api", "role": "builder"}),
    );
    let Reply::Error("not_allowed", why) = r else {
        panic!("{r:?}")
    };
    assert!(why.contains("[role.flags]"), "{why}");
    // The reviewer maximum (2): one runs, a second is fine, a third is refused.
    start(&mut h, &mut f, "reviewer", 4, now);
    let r = ask(
        &mut h,
        &f,
        json!({"profile": "claude", "project": "api", "role": "reviewer"}),
    );
    assert!(refused(&r, "not_allowed"), "{r:?}");
    // The allowance (3) is used: builder, reviewer, reviewer.
    let r = ask(
        &mut h,
        &f,
        json!({"profile": "claude", "project": "api", "role": "builder"}),
    );
    let Reply::Error("not_allowed", why) = r else {
        panic!("{r:?}")
    };
    assert!(why.contains("ask the user"), "{why}");
    // Outside a burst there are no roles.
    let mut plain = Hub::new(OrchestrationSettings::default());
    let r = plain.call(
        &f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": "builder"}),
        now,
    );
    assert!(refused(&r, "invalid"), "{r:?}");
}

#[test]
fn role_flags_are_resolved_and_a_bypass_waits_for_the_engineer() {
    // FR-008, FR-014.
    let mut s = squad::builtin();
    s.roles
        .iter_mut()
        .find(|r| r.name == "reviewer")
        .unwrap()
        .flags
        .insert(
            "claude".into(),
            vec!["--dangerously-skip-permissions".into()],
        );
    let mut h = Hub::new(OrchestrationSettings::default());
    let f = Fake(vec![info(1, "claude")]);
    let now = Instant::now();
    burst(&mut h, &f, s, now);
    let r = h.call(
        &f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": "builder"}),
        now,
    );
    assert!(
        matches!(r, Reply::Done(ref v) if v["state"] == "approved"),
        "{r:?}"
    );
    let b = h.starts.last().unwrap();
    assert_eq!(
        b.args,
        [
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Bash",
            "PowerShell"
        ]
    );
    assert!(!b.bypass);
    let r = h.call(
        &f,
        1,
        "session_start",
        json!({"profile": "claude", "project": "api", "role": "reviewer"}),
        now,
    );
    assert!(
        matches!(r, Reply::Done(ref v) if v["state"] == "pending"),
        "{r:?}"
    );
    let s = h.starts.last().unwrap();
    assert!(s.bypass);
    assert_eq!(s.state, StartState::Pending);
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn a_read_only_worktree_that_changes_stops_counting() {
    // FR-013 with a real repository.
    let d = std::env::temp_dir().join(format!("fallow-ro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    git(&d, &["config", "user.name", "t"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "init"]);
    use fallow::orchestration::git::read_only_check;
    let (tip, files, commits) = read_only_check(&d, Some("main"), None).unwrap();
    assert!(files.is_empty() && commits.is_empty());
    std::fs::write(d.join("a.txt"), "changed\n").unwrap();
    let (_, files, _) = read_only_check(&d, Some("main"), Some(&tip)).unwrap();
    assert_eq!(files, ["a.txt"]);
    git(&d, &["commit", "-q", "-am", "edit"]);
    let (_, files, commits) = read_only_check(&d, Some("main"), Some(&tip)).unwrap();
    assert!(files.is_empty());
    assert_eq!(commits.len(), 1);
    let _ = std::fs::remove_dir_all(&d);

    // The hub: the reviewer's verdict is cleared and its signals refused.
    let (mut h, f, now) = setup();
    assert_eq!(h.read_only_sessions(), [3]);
    assert_eq!(h.check_worktrees, [3], "checked as it starts");
    h.check_worktrees.clear();
    h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert_eq!(
        h.check_worktrees,
        [3],
        "a read-only signal asks for a check"
    );
    h.taint(3, "1 changed file(s): a.txt".into());
    assert!(h.tainted(3));
    assert!(h.read_only_sessions().is_empty());
    let r = h.call(
        &f,
        3,
        "event_signal",
        json!({"name": "review-approved"}),
        now,
    );
    assert!(refused(&r, "not_allowed"), "{r:?}");
    let key = (
        fallow::orchestration::board::BoardId::Project(1),
        "review-approved".to_string(),
    );
    assert!(h.events.map.get(&key).unwrap().signal.is_none(), "cleared");
    assert!(h.notices.iter().any(|n| n.contains("no longer count")));
}

#[test]
fn a_next_free_task_never_returns_to_its_creator_and_the_creator_can_withdraw_it() {
    // 004 manual run: the lead's "next free claude" task was handed to the lead itself.
    let (mut h, mut f, now) = setup();
    // Only the lead is idle: the builder and reviewer are busy for now.
    for s in f.0.iter_mut().filter(|s| s.id != 1) {
        s.idle = false;
    }
    let r = h.call(
        &f,
        1,
        "task_create",
        json!({"text": "build it", "target": {"profile": "claude", "project": "api"}}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let t = v["task"].as_u64().unwrap() as u32;
    h.tick(&f, now);
    assert_eq!(
        h.tasks.get(t).unwrap().assigned,
        None,
        "not handed back to #1"
    );
    f.0.iter_mut().find(|s| s.id == 2).unwrap().idle = true;
    h.tick(&f, now);
    assert_eq!(h.tasks.get(t).unwrap().assigned, Some(2));
    // A builder cannot withdraw the lead's task; the lead can.
    let r = h.call(
        &f,
        2,
        "task_report",
        json!({"task": t, "outcome": "cancelled"}),
        now,
    );
    assert!(refused(&r, "not_allowed"), "{r:?}");
    let r = h.call(
        &f,
        1,
        "task_report",
        json!({"task": t, "outcome": "cancelled", "note": "sent twice"}),
        now,
    );
    assert!(
        matches!(r, Reply::Done(ref v) if v["state"].as_str().unwrap().starts_with("cancelled")),
        "{r:?}"
    );
}

#[test]
fn a_session_at_a_permission_prompt_is_not_free_and_says_so() {
    let (mut h, mut f, now) = setup();
    for s in f.0.iter_mut().filter(|s| s.id == 2) {
        s.state = fallow::orchestration::WAITING_FOR_APPROVAL.into();
    }
    let r = h.call(
        &f,
        1,
        "task_create",
        json!({"text": "build it", "target": {"session": 2}}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let t = v["task"].as_u64().unwrap() as u32;
    h.tick(&f, now);
    assert_eq!(
        h.tasks.get(t).unwrap().assigned,
        None,
        "not typed over a prompt"
    );
    let Reply::Done(v) = h.call(&f, 1, "session_list", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["sessions"][1]["state"], "waiting for the user's approval");
}
