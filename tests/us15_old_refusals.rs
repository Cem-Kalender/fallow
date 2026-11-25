//! 015 (2026-11-24 amendment): a chat recorded before the amendment, with its `refused`,
//! `message_queued`-era `refusal_allowed`, `refusal_used` and `refusal_dismissed` events, still
//! loads and renders as before: an allowed refusal whose grant went with the restart reads
//! "expired", a refused one still waits in "Needs you", and nothing is sent to the agent.

mod common;

use common::*;
use fallow::studio::Req;

#[test]
fn an_old_log_with_refusals_still_loads_and_renders() {
    let root = scratch("us15-old-log");
    let dir = root.join("data/conversations/c0ld");
    std::fs::create_dir_all(&dir).unwrap();
    let lines = [
        r#"{"seq": 1, "at": "2026-11-24T17:26:32Z", "type": "conversation_created", "kind": "spec", "project": "gone", "feature": "001-old", "forked_from": null, "workdir": "/nowhere/001-old", "lead": null}"#,
        r#"{"seq": 2, "at": "2026-11-24T17:26:32Z", "type": "chat_mode", "mode": "auto"}"#,
        r#"{"seq": 3, "at": "2026-11-24T17:30:29Z", "type": "turn_started", "n": 1, "said": "look around", "edits": [], "mode": "auto", "session": "s1"}"#,
        r#"{"seq": 4, "at": "2026-11-24T17:30:48Z", "type": "refused", "id": "r1", "kind": "run", "what": "cut -f1 /tmp/cropscan/shard3.tsv", "reason": "it reaches outside the session's working copy: names a path outside the session's working copy (/tmp/cropscan/shard3.tsv)", "limit": "edit"}"#,
        r#"{"seq": 5, "at": "2026-11-24T17:30:51Z", "type": "refusal_allowed", "id": "r1"}"#,
        r#"{"seq": 6, "at": "2026-11-24T17:32:14Z", "type": "refused", "id": "r2", "kind": "edit", "what": "/tmp/cropscan/result1.md", "reason": "the assistant may not change /tmp/cropscan/result1.md: names a path outside the session's working copy (/tmp/cropscan/result1.md)", "limit": "edit"}"#,
        r#"{"seq": 7, "at": "2026-11-24T17:32:20Z", "type": "refused", "id": "r3", "kind": "run", "what": "git push origin HEAD:main", "reason": "it pushes onto main", "limit": "target"}"#,
        r#"{"seq": 8, "at": "2026-11-24T17:32:21Z", "type": "refusal_allowed", "id": "r3"}"#,
        r#"{"seq": 9, "at": "2026-11-24T17:32:22Z", "type": "refusal_used", "id": "r3"}"#,
        r#"{"seq": 10, "at": "2026-11-24T17:32:23Z", "type": "refused", "id": "r4", "kind": "edit", "what": ".claude/settings.json", "reason": "never", "limit": "control"}"#,
        r#"{"seq": 11, "at": "2026-11-24T17:32:24Z", "type": "refusal_dismissed", "id": "r4"}"#,
        r#"{"seq": 12, "at": "2026-11-24T17:33:56Z", "type": "turn_finished", "n": 1, "reply": "Done.", "result": null, "session": "s1", "documents_changed": [], "commit": null, "discarded": [], "cost_usd": null, "failure": null}"#,
    ];
    std::fs::write(dir.join("events.jsonl"), lines.join("\n") + "\n").unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Conversation { id: "c0ld".into() }).unwrap();
    let rs = v["turns"][0]["refusals"].as_array().cloned().unwrap();
    let states: Vec<(&str, &str)> = rs
        .iter()
        .map(|r| (r["id"].as_str().unwrap(), r["state"].as_str().unwrap()))
        .collect();
    assert_eq!(
        states,
        [
            ("r1", "expired"),
            ("r2", "refused"),
            ("r3", "used"),
            ("r4", "dismissed")
        ],
        "{rs:?}"
    );
    assert_eq!(rs[1]["limit"], "edit");
    assert!(v["pending_approval"].is_null());
    // The old refusal still waiting is in "Needs you", as before.
    let needs = ask(tx, fallow::run::engine::Request::Dashboard).unwrap()["needs_you"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        needs
            .iter()
            .any(|n| n["kind"] == "refused" && n["refusal"] == "r2"),
        "{needs:?}"
    );
    // Nothing is sent to the agent on load: no turn of its own.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let v = sreq(tx, Req::Conversation { id: "c0ld".into() }).unwrap();
    assert_eq!(v["turns"].as_array().unwrap().len(), 1);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
