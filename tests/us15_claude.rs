//! 015 quickstart Q8 (SC-003), by hand: the same Manual and Auto with Claude Code as the
//! assistant and as the lead, on Sonnet. Not run by `cargo test`: it calls the real `claude`,
//! signed in, which costs a little. Run with
//! `FALLOW_REAL_CLAUDE=1 cargo test --features automation --test us15_claude -- --ignored --nocapture`.

mod common;

use std::time::{Duration, Instant};

use common::*;
use fallow::config::settings::Settings;
use fallow::studio::Req;
use serde_json::Value;

const MODEL: &str = "claude-sonnet-5-5";

fn wait(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    conv: &str,
    what: &str,
    pred: impl Fn(&Value) -> bool,
) -> Value {
    let end = Instant::now() + Duration::from_secs(420);
    let mut last = Value::Null;
    while Instant::now() < end {
        if let Ok(v) = sreq(tx, Req::Conversation { id: conv.into() }) {
            if pred(&v) {
                return v;
            }
            last = v;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("timed out waiting for {what}: {last}");
}

fn ended(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, conv: &str, n: usize) -> Value {
    wait(tx, conv, "the turn's end", |v| {
        v["busy"] == false
            && v["turns"].as_array().is_some_and(|t| t.len() >= n)
            && !v["turns"][n - 1]["reply"].is_null()
    })
}

fn refusals(v: &Value) -> Vec<Value> {
    v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["refusals"].as_array().cloned().unwrap_or_default())
        .collect()
}

#[test]
#[ignore = "calls the real Claude Code; run by hand (quickstart Q8)"]
fn claude_code_gets_the_same_manual_and_auto() {
    if std::env::var("FALLOW_REAL_CLAUDE").is_err() {
        eprintln!("FALLOW_REAL_CLAUDE is not set: skipped");
        return;
    }
    let root = scratch("us15-claude");
    let repo = repo(&root);
    // Both agents on Sonnet, from the project's own definitions.
    for (name, file) in [("assistant", "assistant.md"), ("lead", "lead.md")] {
        let shipped = fallow::defs::DEFAULT_AGENTS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap()
            .1;
        let text = shipped.replace("model: opus", &format!("model: {MODEL}"));
        let dir = repo.join(".fallow/agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "sonnet agents"]);
    let mut n = Vec::new();
    let settings = Settings::parse(
        "settings.toml",
        "[defaults]\nchat_mode = \"auto\"\n[agents.claude]\ncommand = \"claude\"\n",
        &mut n,
    );
    assert!(n.is_empty(), "{n:?}");
    let e = start_studio_settings(&root, settings);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);

    // The assistant in Auto: nothing asked; the push onto main refused at the limit.
    let s = start_session(
        tx,
        &project,
        "Use your shell tool to run these three commands, one call each, exactly as written, and nothing else: \
         `python3 -c \"print(6*7)\"`, then `echo hello > note.txt`, then `git push origin HEAD:main`. \
         If one is refused, do not try another way. Then reply in one line with what happened.",
    );
    let v = ended(tx, &s, 1);
    eprintln!("assistant, Auto: {}", v["turns"][0]["reply"]);
    assert_eq!(v["session"]["mode"], "auto");
    assert!(
        v["turns"][0]["approvals"].as_array().unwrap().is_empty(),
        "{}",
        v["turns"][0]
    );
    let activity = v["activity"].to_string();
    assert!(activity.contains("42"), "{activity}");
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    assert!(wt.join("note.txt").exists(), "{activity}");
    let rs = refusals(&v);
    assert!(
        rs.iter().any(|r| r["limit"] == "target"),
        "{rs:?} {activity}"
    );
    assert!(
        activity.contains("Fallow refused this"),
        "the agent was told: {activity}"
    );

    // The assistant in Manual: a command that is not approved is asked.
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "manual".into(),
        },
    )
    .unwrap();
    say_solo(
        tx,
        &s,
        "Use your shell tool to run `python3 -c \"print(7*8)\"`, exactly as written, then reply in one line.",
    );
    let w = wait(tx, &s, "a request card", |v| {
        !v["pending_approval"].is_null()
    });
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    assert!(
        w["pending_approval"]["command"]
            .as_str()
            .unwrap()
            .contains("print(7*8)")
    );
    sreq(
        tx,
        Req::ApproveAuto {
            conv: s.clone(),
            id,
        },
    )
    .unwrap();
    let v = ended(tx, &s, 2);
    assert!(
        v["activity"].to_string().contains("56"),
        "{}",
        v["activity"]
    );
    assert_eq!(v["session"]["mode"], "auto");

    // The lead in Auto: its command runs unasked; an edit outside the spec documents is refused.
    let lead = sreq(
        tx,
        Req::NewFeature {
            project,
            title: "Sonnet check".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        sreq(tx, Req::Conversation { id: lead.clone() }).unwrap()["mode"],
        "auto"
    );
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "This is a test of Fallow's permission modes, not a feature: do not run any Spec Kit step. \
                   Use your shell tool to run `python3 -c \"print(9*9)\"` exactly as written, then use your \
                   file-writing tool to create `src/not-a-spec.txt` with the text `x`. If something is refused, \
                   do not try another way. Then reply in one line."
                .into(),
        },
    )
    .unwrap();
    let l = ended(tx, &lead, 1);
    eprintln!("lead, Auto: {}", l["turns"][0]["reply"]);
    assert!(
        l["turns"][0]["approvals"].as_array().unwrap().is_empty(),
        "{}",
        l["turns"][0]
    );
    let la = l["activity"].to_string();
    assert!(la.contains("81"), "{la}");
    let lr = refusals(&l);
    assert!(lr.iter().any(|r| r["limit"] == "edit"), "{lr:?} {la}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
