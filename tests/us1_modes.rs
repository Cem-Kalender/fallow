//! 007 US1 (T024; FR-007a; research S-7): each chat mode gives the lead its rights; in Ask mode a
//! command outside the rules reaches the engineer through Fallow's `approve` tool, over the MCP
//! bridge, checked against the turn's own processes; once, always (the prefix joins the
//! project's approved commands, which then run without asking), deny, and the time-out; a new
//! conversation starts in Ask.

mod common;

use std::time::Duration;

use common::*;
use fallow::agent::rights::Mode;
use fallow::conversation::events::ApprovalAnswer;
use fallow::studio::Req;

fn turn(cmd: &str) -> String {
    format!(
        "[[step]]\nkind = \"lead_turn\"\napprove = {cmd:?}\noutput = {{ reply = \"$APPROVED\", step = \"none\" }}\n"
    )
}

#[test]
fn ask_mode_asks_the_engineer_and_the_other_modes_do_not() {
    let root = scratch("modes");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let steps = [
        turn("pip list"),               // 1 Ask: allowed once
        turn("pip list --outdated"),    // 2 Ask: allowed always as "pip list"
        turn("pip list --format json"), // 3 Ask: now approved, runs without asking
        turn("python -c 1"),            // 4 Ask: denied
        turn("python -c 2"),            // 5 Ask: nobody answers
        turn("python -c 3"),            // 6 Read only: refused without asking
        turn("python -c 4"),            // 7 Run anything: runs without asking
        turn("python -c 5"),            // 8 Approved commands: refused without asking
    ];
    std::fs::write(&scenario, steps.concat()).unwrap();
    let e = start_studio(&root, &scenario, Some(Duration::from_secs(3)));
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    let conv = sreq(
        tx,
        Req::ProjectConversation {
            project: project.clone(),
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let v = sreq(tx, Req::Conversation { id: conv.clone() }).unwrap();
    assert_eq!(v["mode"], "ask", "a new conversation starts in Ask");

    let say = |n: usize| {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: format!("turn {n}"),
            },
        )
        .unwrap();
    };
    let waiting = || {
        wait_conv(tx, &conv, "a command waiting", |v| {
            !v["pending_approval"].is_null()
        })
    };
    let answer = |id: &str, a: ApprovalAnswer, prefix: Option<&str>| {
        sreq(
            tx,
            Req::Approve {
                conv: conv.clone(),
                id: id.into(),
                answer: a,
                prefix: prefix.map(str::to_string),
            },
        )
    };

    // 1: Once.
    say(1);
    let w = waiting();
    assert_eq!(w["pending_approval"]["command"], "pip list");
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    answer(&id, ApprovalAnswer::Once, None).unwrap();
    let v = turn_ended(tx, &conv, 1);
    assert_eq!(v["turns"][0]["reply"], "allow", "{v}");
    assert_eq!(v["turns"][0]["approvals"][0]["answer"], "once");

    // 2: Always, with a prefix the command starts with; a prefix it does not start with is refused.
    say(2);
    let w = waiting();
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    assert!(answer(&id, ApprovalAnswer::Always, Some("npm test")).is_err());
    assert!(
        answer(&id, ApprovalAnswer::Always, Some("git log")).is_err(),
        "not a command agents may run"
    );
    answer(&id, ApprovalAnswer::Always, Some("pip list")).unwrap();
    let v = turn_ended(tx, &conv, 2);
    assert_eq!(v["turns"][1]["reply"], "allow");
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(
        p["approved_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "pip list"),
        "{}",
        p["approved_commands"]
    );

    // 3: Now an approved command: it runs without asking.
    say(3);
    let v = turn_ended(tx, &conv, 3);
    assert_eq!(v["turns"][2]["reply"], "allow");
    assert!(
        v["turns"][2]["approvals"].as_array().unwrap().is_empty(),
        "nobody was asked"
    );

    // 4: Deny.
    say(4);
    let w = waiting();
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    answer(&id, ApprovalAnswer::Deny, None).unwrap();
    let v = turn_ended(tx, &conv, 4);
    assert_eq!(v["turns"][3]["reply"], "deny");

    // 5: Nobody answers: denied after the wait (3 s here, 10 minutes for real).
    say(5);
    waiting();
    let v = turn_ended(tx, &conv, 5);
    assert_eq!(v["turns"][4]["reply"], "deny");
    assert_eq!(v["turns"][4]["approvals"][0]["answer"], "deny");

    // 6: Read only refuses without asking.
    sreq(
        tx,
        Req::SetMode {
            conv: conv.clone(),
            mode: Mode::ReadOnly,
        },
    )
    .unwrap();
    say(6);
    let v = turn_ended(tx, &conv, 6);
    assert_eq!(v["turns"][5]["reply"], "deny");
    assert!(v["turns"][5]["approvals"].as_array().unwrap().is_empty());

    // 7: Run anything runs without asking.
    sreq(
        tx,
        Req::SetMode {
            conv: conv.clone(),
            mode: Mode::RunAnything,
        },
    )
    .unwrap();
    say(7);
    let v = turn_ended(tx, &conv, 7);
    assert_eq!(v["turns"][6]["reply"], "allow");

    // 8: Approved commands refuses what is not approved, without asking.
    sreq(
        tx,
        Req::SetMode {
            conv: conv.clone(),
            mode: Mode::Approved,
        },
    )
    .unwrap();
    say(8);
    let v = turn_ended(tx, &conv, 8);
    assert_eq!(v["turns"][7]["reply"], "deny");
    assert!(v["turns"][7]["approvals"].as_array().unwrap().is_empty());
    assert_eq!(v["mode"], "approved");

    // A token from no running turn is refused at the door.
    let open = sreq(
        tx,
        Req::ToolOpen {
            token: "0".repeat(64),
            pid: std::process::id(),
        },
    );
    assert!(open.is_err());

    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
