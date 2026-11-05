//! 012 FR-043 (the engineer's answer of 2026-11-05): the assistant reads in its session's
//! working copy without asking; a read of a file that may hold secrets, or one outside the
//! working copy, is put to the engineer, once at a time, never "always".

mod common;

use common::*;
use fallow::conversation::events::ApprovalAnswer;
use fallow::studio::Req;

#[test]
fn reads_in_the_copy_go_unasked_and_secrets_or_outside_reads_are_asked() {
    let root = scratch("us12-reads");
    let repo = repo(&root);
    std::fs::write(repo.join("notes.txt"), "plain notes\n").unwrap();
    std::fs::write(repo.join(".env"), "TOKEN=not-a-real-one\n").unwrap();
    git(&repo, &["add", "-f", "notes.txt", ".env"]);
    git(&repo, &["commit", "-q", "-m", "notes"]);
    let outside = root.join("elsewhere.txt");
    std::fs::write(&outside, "outside\n").unwrap();
    let outside_s = outside.display().to_string();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            "[[step]]\nkind = \"assistant_turn\"\nread = [\"notes.txt\", \".env\", {}]\noutput = {{ reply = \"read\" }}\n",
            lit(&outside_s)
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "read the notes");

    // notes.txt went unasked; .env is asked first, as a read, with its reason.
    let w = wait_conv(tx, &s, "a read waiting", |v| !v["pending_approval"].is_null());
    let p = &w["pending_approval"];
    assert_eq!(p["kind"], "read", "{p}");
    assert_eq!(p["command"], "read .env", "{p}");
    assert!(p["always"].is_null(), "never always: {p}");
    let asked = &w["turns"][0]["approvals"];
    assert_eq!(asked.as_array().unwrap().len(), 1, "notes.txt was not asked: {asked}");
    assert!(
        asked[0]["reasons"][0].as_str().is_some_and(|r| r.contains("secrets")),
        "{asked}"
    );
    let id = p["id"].as_str().unwrap().to_string();
    let refused = sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id: id.clone(),
            answer: ApprovalAnswer::Always,
            prefix: Some("read".into()),
        },
    )
    .unwrap_err();
    assert!(refused.contains("once at a time"), "{refused}");
    sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id,
            answer: ApprovalAnswer::Deny,
            prefix: None,
        },
    )
    .unwrap();

    // Then the file outside the working copy, said so; allowed once.
    let w = wait_conv(tx, &s, "the outside read waiting", |v| {
        v["pending_approval"]["command"]
            .as_str()
            .is_some_and(|c| c.contains("elsewhere.txt"))
    });
    let asked = &w["turns"][0]["approvals"][1];
    assert!(
        asked["reasons"][0].as_str().is_some_and(|r| r.contains("outside")),
        "{asked}"
    );
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Approve {
            conv: s.clone(),
            id,
            answer: ApprovalAnswer::Once,
            prefix: None,
        },
    )
    .unwrap();
    let v = wait_turn(tx, &s, 1);
    let approvals = v["turns"][0]["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 2, "{v}");
    assert_eq!(approvals[0]["answer"], "deny");
    assert_eq!(approvals[1]["answer"], "once");
}
