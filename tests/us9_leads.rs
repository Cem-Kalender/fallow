//! 009 US2: choosing the lead. A project has a default lead; a session can pick another before
//! its first message and keeps it from then on; a changed runtime or model starts a new agent
//! session; a lead that is gone is said so; a loop's questions go to the feature's session lead.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;
use serde_json::{Value, json};

fn add(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, repo: &std::path::Path) -> String {
    let project = sreq(
        tx,
        Req::AddProject {
            path: repo.to_path_buf(),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "spec kit"]);
    project
}

fn feature(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    project: &str,
    title: &str,
) -> String {
    sreq(
        tx,
        Req::NewFeature {
            project: project.into(),
            title: title.into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string()
}

fn view(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, conv: &str) -> Value {
    sreq(tx, Req::Conversation { id: conv.into() }).unwrap()
}

fn say(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    conv: &str,
    text: &str,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::Say {
            conv: conv.into(),
            text: text.into(),
        },
    )
}

fn set_lead(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    conv: &str,
    lead: &str,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::SetLead {
            conv: conv.into(),
            lead: lead.into(),
        },
    )
}

#[test]
fn a_project_lead_starts_new_sessions_and_a_pick_comes_before_the_first_message() {
    let root = scratch("leads-pick");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
model = "claude-careful-1"
system = "CAREFUL-MARKER"
when = "Hello careful"
output = { reply = "careful here", step = "none" }

[[step]]
kind = "lead_turn"
model = "claude-opus-5-5"
when = "Hello plain"
output = { reply = "plain here", step = "none" }
"##,
    )
    .unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-careful-1",
        "CAREFUL-MARKER",
    );
    // On a runtime this Fallow has no profile for: listed, but not choosable.
    second_lead(&repo, "ghost-lead", "fake", "fake-x", "GHOST");
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = add(tx, &repo);

    let patch = |p: Value| {
        sreq(
            tx,
            Req::PatchProject {
                id: project.clone(),
                patch: p,
            },
        )
    };
    assert!(
        patch(json!({"lead": "nope"}))
            .unwrap_err()
            .contains("there is no lead nope")
    );
    assert!(
        patch(json!({"lead": "builder"}))
            .unwrap_err()
            .contains("not a lead")
    );
    assert!(
        patch(json!({"lead": "ghost-lead"}))
            .unwrap_err()
            .contains("runtime is not available")
    );
    // A refused key leaves every key as it was.
    assert!(patch(json!({"lead": "careful-lead", "autonomy": "bogus"})).is_err());
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(p["lead"]["name"], "lead", "{p}");
    patch(json!({"lead": "careful-lead"})).unwrap();
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(p["lead"]["name"], "careful-lead");
    assert!(p["lead"]["problem"].is_null(), "{p}");

    // A new feature starts with the project's lead.
    let a = feature(tx, &project, "First");
    let v = view(tx, &a);
    assert_eq!(v["lead"]["name"], "careful-lead", "{v}");
    assert_eq!(v["lead"]["model"], "claude-careful-1");
    assert_eq!(v["lead"]["fixed"], false);
    let leads = v["leads"].as_array().unwrap();
    let find = |n: &str| leads.iter().find(|l| l["name"] == n).unwrap().clone();
    assert_eq!(find("careful-lead")["available"], true);
    assert_eq!(find("lead")["available"], true);
    assert_eq!(find("ghost-lead")["available"], false);
    assert!(find("ghost-lead")["reason"].is_string());
    say(tx, &a, "Hello careful").unwrap();
    let v = turn_ended(tx, &a, 1);
    assert_eq!(v["turns"][0]["reply"], "careful here", "{v}");
    assert_eq!(v["lead"]["fixed"], true);
    assert!(
        set_lead(tx, &a, "lead")
            .unwrap_err()
            .contains("A session keeps its lead")
    );

    // Another feature: the engineer picks `lead` before the first message.
    let b = feature(tx, &project, "Second");
    assert!(
        set_lead(tx, &b, "ghost-lead")
            .unwrap_err()
            .contains("runtime is not available")
    );
    set_lead(tx, &b, "lead").unwrap();
    assert_eq!(view(tx, &b)["lead"]["name"], "lead");
    say(tx, &b, "Hello plain").unwrap();
    let v = turn_ended(tx, &b, 1);
    assert_eq!(v["turns"][0]["reply"], "plain here", "{v}");

    // A pick when the feature is started.
    let c = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Third".into(),
            lead: Some("lead".into()),
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(view(tx, &c)["lead"]["name"], "lead");
}

#[test]
fn forks_new_sessions_and_a_lead_that_is_gone() {
    let root = scratch("leads-fork");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
forked = false
when = "Same lead"
output = { reply = "own", step = "none" }

[[step]]
kind = "lead_turn"
forked = false
model = "claude-sonnet-5-5"
when = "Lead changed under a fork"
output = { reply = "fresh after the fork", step = "none" }

[[step]]
kind = "lead_turn"
forked = false
model = "claude-careful-1"
when = "Other lead"
output = { reply = "new", step = "none" }

[[step]]
kind = "lead_turn"
model = "claude-careful-1"
when = "Again"
output = { reply = "resumed", step = "none" }

[[step]]
kind = "lead_turn"
forked = false
model = "claude-careful-2"
when = "After the change"
output = { reply = "changed", step = "none" }
"##,
    )
    .unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-careful-1",
        "CAREFUL-MARKER",
    );
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = add(tx, &repo);
    // 012 (FR-035): a spec session forks no project conversation; it starts its own session.
    let a = feature(tx, &project, "Same");
    say(tx, &a, "Same lead").unwrap();
    let v = turn_ended(tx, &a, 1);
    assert_eq!(v["turns"][0]["reply"], "own", "{v}");
    assert!(!v["session"].as_str().unwrap().contains("-fork"), "{v}");
    // The lead's model changes under the session: said, and a new session follows.
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "lead")
        .unwrap()
        .1;
    std::fs::write(
        repo.join(".fallow/agents/lead.md"),
        replace_found(
            shipped,
            "model: claude-opus-5-5",
            "model: claude-sonnet-5-5",
        ),
    )
    .unwrap();
    assert_eq!(view(tx, &a)["lead"]["changed"], true, "{}", view(tx, &a));
    say(tx, &a, "Lead changed under a fork").unwrap();
    let v = turn_ended(tx, &a, 2);
    assert_eq!(v["turns"][1]["reply"], "fresh after the fork", "{v}");
    assert_eq!(v["lead"]["changed"], false, "{v}");
    std::fs::remove_file(repo.join(".fallow/agents/lead.md")).unwrap();

    // Another lead: a new session.
    sreq(
        tx,
        Req::PatchProject {
            id: project.clone(),
            patch: json!({"lead": "careful-lead"}),
        },
    )
    .unwrap();
    let b = feature(tx, &project, "Other");
    say(tx, &b, "Other lead").unwrap();
    let v = turn_ended(tx, &b, 1);
    assert_eq!(v["turns"][0]["reply"], "new", "{v}");
    let first = v["session"].as_str().unwrap().to_string();
    say(tx, &b, "Again").unwrap();
    let v = turn_ended(tx, &b, 2);
    assert_eq!(v["session"], first.as_str(), "resumed: {v}");
    assert_eq!(v["lead"]["changed"], false);

    // The lead's model changes between turns: said, then a new session on the new model.
    let file = repo.join(".fallow/agents/careful-lead.md");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        replace_found(&text, "model: claude-careful-1", "model: claude-careful-2"),
    )
    .unwrap();
    let v = view(tx, &b);
    assert_eq!(v["lead"]["changed"], true, "{v}");
    say(tx, &b, "After the change").unwrap();
    let v = turn_ended(tx, &b, 3);
    assert_eq!(v["turns"][2]["reply"], "changed", "{v}");
    assert_ne!(v["session"], first.as_str(), "{v}");
    assert_eq!(v["lead"]["changed"], false, "{v}");

    // The lead is deleted: its session says so and refuses; the project falls back to `lead`.
    std::fs::remove_file(&file).unwrap();
    let v = view(tx, &b);
    assert!(
        v["lead"]["problem"].as_str().unwrap().contains("gone"),
        "{v}"
    );
    let err = say(tx, &b, "Anyone?").unwrap_err();
    assert!(err.contains("gone"), "{err}");
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(p["lead"]["problem"].is_string(), "{p}");
    let c = feature(tx, &project, "Fallback");
    assert_eq!(view(tx, &c)["lead"]["name"], "lead");
}

#[test]
fn the_lead_is_fixed_while_answering_after_a_failure_but_not_after_a_withdrawal() {
    let root = scratch("leads-fixed");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "Slow"
sleep_ms = 3000
output = { reply = "slow", step = "none" }

[[step]]
kind = "lead_turn"
when = "Fail"
is_error = true
output = { reply = "x", step = "none" }

[[step]]
kind = "lead_turn"
when = "Cancel me"
sleep_ms = 30000
output = { reply = "never", step = "none" }
"##,
    )
    .unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-careful-1",
        "CAREFUL-MARKER",
    );
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = add(tx, &repo);

    // SC-006 (CI): right after `say` returns, the conversation is busy; the lead is fixed and a
    // change waits for the answer.
    let a = feature(tx, &project, "Slow");
    say(tx, &a, "Slow").unwrap();
    let v = view(tx, &a);
    assert_eq!(v["busy"], true, "{v}");
    assert_eq!(v["lead"]["fixed"], true);
    let err = set_lead(tx, &a, "careful-lead").unwrap_err();
    assert!(err.contains("Wait until the lead has answered"), "{err}");
    turn_ended(tx, &a, 1);

    // A first turn that failed still fixes the lead.
    let b = feature(tx, &project, "Fails");
    say(tx, &b, "Fail").unwrap();
    let v = wait_conv(tx, &b, "the failed turn", |v| {
        v["busy"] == false && !v["turns"][0]["failure"].is_null()
    });
    assert_eq!(v["lead"]["fixed"], true, "{v}");
    assert!(
        set_lead(tx, &b, "careful-lead")
            .unwrap_err()
            .contains("A session keeps its lead")
    );

    // A first message withdrawn before it had a session does not.
    let c = feature(tx, &project, "Withdrawn");
    say(tx, &c, "Cancel me").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    sreq(tx, Req::Cancel { conv: c.clone() }).unwrap();
    let v = wait_conv(tx, &c, "withdrawn", |v| v["busy"] == false);
    assert_eq!(v["turns"][0]["withdrawn"], true, "{v}");
    assert_eq!(v["lead"]["fixed"], false, "{v}");
    set_lead(tx, &c, "careful-lead").unwrap();
}

#[test]
fn a_session_keeps_its_lead_across_a_restart_and_old_logs_read_lead() {
    use fallow::run::engine::Msg;
    let root = scratch("leads-restart");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-careful-1",
        "CAREFUL-MARKER",
    );
    let e = start_studio(&root, &scenario, None);
    let project = add(&e.tx, &repo);
    let a = feature(&e.tx, &project, "Kept");
    set_lead(&e.tx, &a, "careful-lead").unwrap();
    let old = feature(&e.tx, &project, "Old");
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    // A log written before 009: no `lead` on its first event.
    let log = root
        .join("data")
        .join("conversations")
        .join(&old)
        .join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, text.replace(",\"lead\":null", "")).unwrap();
    assert!(!std::fs::read_to_string(&log).unwrap().contains("\"lead\""));

    let e = start_studio(&root, &scenario, None);
    assert_eq!(view(&e.tx, &a)["lead"]["name"], "careful-lead");
    assert_eq!(view(&e.tx, &old)["lead"]["name"], "lead");
}

/// A builder step that asks `question`.
fn asking_step(task: &str, question: &str, files: &[(&str, &str)]) -> String {
    task_step(task, files, &format!("ask = {question:?}\n")).replacen(
        "status = \"done\"",
        "status = \"done\", question = \"$ANSWER\"",
        1,
    )
}

fn lead_answer(q: &str, extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"lead_answer\"\npart = {q:?}\n{extra}\noutput = {{ outcome = \"decided\", answer = \"No floats\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [] }}\n\n"
    )
}

#[test]
fn a_loop_question_goes_to_the_feature_sessions_lead() {
    let root = scratch("leads-loop");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], "sleep_ms = 2500"),
        handover_step(1, ""),
        asking_step(
            "T002",
            "Should add() accept floats?",
            &[("calc/__init__.py", CALC_INIT)],
        ),
        // careful-lead's model changed after the lock: a new session on the new model, not a
        // fork of the spec session's.
        lead_answer("q1", "model = \"claude-careful-2\"\nforked = false"),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", true, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    // The spec session's two turns are careful-lead's.
    let lead = lead_writes_calc().replace(
        "kind = \"lead_turn\"\n",
        "kind = \"lead_turn\"\nmodel = \"claude-careful-1\"\n",
    );
    std::fs::write(&scenario, format!("{lead}{steps}")).unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-careful-1",
        "CAREFUL-MARKER",
    );
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = add(tx, &repo);
    sreq(
        tx,
        Req::PatchProject {
            id: project.clone(),
            patch: json!({"lead": "careful-lead"}),
        },
    )
    .unwrap();
    let conv = feature(tx, &project, "Calc");
    for (n, text) in [(1, "Write it."), (2, "Analyze.")] {
        say(tx, &conv, text).unwrap();
        let v = turn_ended(tx, &conv, n);
        assert!(v["turns"][n - 1]["failure"].is_null(), "{v}");
    }
    let l = ask(
        tx,
        Request::Lock {
            project,
            feature: "001-calc".into(),
            pipeline: None,
            level: Some(fallow::run::events::Level::Free),
            budget_usd: Some(5.0),
            budget_minutes: Some(60),
            when_verified: None,
            update_limit: None,
            run_after: None,
        },
    )
    .unwrap();
    let id = l["loop"].as_u64().unwrap();
    let file = repo.join(".fallow/agents/careful-lead.md");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        replace_found(&text, "model: claude-careful-1", "model: claude-careful-2"),
    )
    .unwrap();

    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["questions"]["q1"]["by"], "careful-lead");
    // The loop view (contracts/web-api.md): who answered, and that it was a lead.
    let view = ask(tx, Request::Loop { id }).unwrap();
    let qs = view["questions"].as_array().unwrap();
    assert_eq!(qs[0]["by"], "careful-lead", "{qs:?}");
    assert_eq!(qs[0]["by_lead"], true, "{qs:?}");
    assert_eq!(qs[0]["overrulable"], true, "{qs:?}");
    // The engineer can overrule any lead's answer.
    ask(
        tx,
        Request::Overrule {
            id,
            q: "q1".into(),
            answer: "Accept floats too".into(),
        },
    )
    .unwrap();
}

/// 017 US3 AS5 (T031; FR-021): a spec session begun before 017 recorded its lead's model as the
/// alias (`opus`). After the update the shipped lead names `claude-opus-5-5`: the session says
/// its lead changed, and its next message starts a new agent session (no `--resume`) with the
/// conversation kept.
#[test]
fn a_session_begun_on_an_alias_starts_a_new_agent_session_after_the_update() {
    use fallow::run::engine::Msg;
    let root = scratch("leads-alias");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
forked = false
when = "Before the update"
output = { reply = "on 0.3.1", step = "none" }

[[step]]
kind = "lead_turn"
forked = false
model = "claude-opus-5-5"
when = "After the update"
output = { reply = "on the full name", step = "none" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let project = add(&e.tx, &repo);
    let conv = feature(&e.tx, &project, "Kept");
    say(&e.tx, &conv, "Before the update").unwrap();
    let v = turn_ended(&e.tx, &conv, 1);
    assert_eq!(v["turns"][0]["reply"], "on 0.3.1", "{v}");
    let first = v["session"].as_str().unwrap().to_string();
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    // The log as 0.3.1 wrote it: the lead `lead` on `claude`, its model the alias.
    let log = root
        .join("data")
        .join("conversations")
        .join(&conv)
        .join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(
        &log,
        replace_found(&text, "\"model\":\"claude-opus-5-5\"", "\"model\":\"opus\""),
    )
    .unwrap();

    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = view(tx, &conv);
    assert_eq!(v["lead"]["name"], "lead", "{v}");
    assert_eq!(v["lead"]["model"], "claude-opus-5-5", "{v}");
    assert_eq!(v["lead"]["changed"], true, "{v}");
    say(tx, &conv, "After the update").unwrap();
    let v = turn_ended(tx, &conv, 2);
    assert_eq!(v["turns"][1]["reply"], "on the full name", "{v}");
    assert!(v["turns"][1]["failure"].is_null(), "{v}");
    // A new agent session, not the old one resumed; the conversation is kept.
    assert_ne!(v["session"], first.as_str(), "{v}");
    assert_eq!(v["turns"][0]["reply"], "on 0.3.1", "{v}");
    assert_eq!(v["lead"]["changed"], false, "{v}");
}
