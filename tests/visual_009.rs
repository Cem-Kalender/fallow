//! 009's page checks (T012, T020, T022, T025, T035, T038, T041): a demo project on the fake runtime,
//! served on a port of its own, so the checks never touch the engineer's Fallow. Not a test: run
//! it by hand with `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_009 -- --ignored
//! --nocapture`; it writes the page's address to `<root>/url.txt` and serves for
//! `FALLOW_VISUAL_SECS` seconds (default 1800).

mod common;

use common::*;
use fallow::studio::Req;
use std::path::PathBuf;

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
#[ignore]
fn serve_the_009_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    // The loop stays at work on T003 for the whole check, so its drawer shows a running loop.
    let loop_steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            &format!("sleep_ms = {}", secs * 1000),
        ),
    ]
    .concat();
    let chat = r##"
[[step]]
kind = "lead_turn"
when = "speckit-plan/SKILL.md"
output = { reply = "I ran **plan** from the menu.", step = "plan" }

[[step]]
kind = "lead_turn"
when = "Slow answer"
sleep_ms = 8000
say = "Thinking it over"
output = { reply = "Done, slowly.", step = "none" }

[[step]]
kind = "lead_turn"
when = "review-copy"
output = { reply = "Reviewed the copy.", step = "none" }

[[step]]
kind = "lead_turn"
when = "refers to these files"
output = { reply = "About that file: it adds two numbers.", step = "none" }

[[step]]
kind = "lead_turn"
when = "Propose e-mail"
output = { reply = "Here is one.", step = "none", proposed_feature = { title = "Receipt by e-mail", summary = "Send the printed receipt to an address given at checkout; the order is unchanged." } }

[[step]]
kind = "lead_turn"
when = "Propose stock"
output = { reply = "And another.", step = "none", proposed_feature = { title = "Low-stock warning", summary = "Warn when a product's stock falls under a threshold set per product.", after = "Receipt by e-mail" } }

[[step]]
kind = "lead_turn"
when = "Propose receipts"
output = { reply = "This one exists already.", step = "none", proposed_feature = { title = "Receipts", summary = "Already a feature." } }
"##;
    let catch_all =
        "[[step]]\nkind = \"lead_turn\"\noutput = { reply = \"OK.\", step = \"none\" }\n\n"
            .repeat(20);
    std::fs::write(
        &scenario,
        format!("{}{loop_steps}{chat}{catch_all}", lead_writes_calc()),
    )
    .unwrap();
    second_lead(&repo, "careful-lead", "claude", "opus", "CAREFUL-MARKER");
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    lock_calc_in(tx, &repo, None, Some((50.0, 600)), None);
    // The project's own command and a secret, not committed.
    put(
        &repo,
        ".claude/commands/review-copy.md",
        "---\ndescription: Review the page's copy\n---\nReview the copy of $ARGUMENTS.\n",
    );
    put(&repo, ".env", "TOKEN=never-shown\n");
    // A second feature with an open spec session.
    sreq(
        tx,
        Req::NewFeature {
            project: "repo".into(),
            title: "Receipts".into(),
            lead: None,
        },
    )
    .unwrap();
    let web = fallow::service::http::Web::start(tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
}
