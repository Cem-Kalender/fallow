//! 013 T001: the demo of dark mode's visual checks, on the fake runtime. Not a test: run it by
//! hand with `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_013 -- --ignored
//! --nocapture`. It builds:
//! - a loop ready to merge (project `shop`);
//! - a loop asking the engineer (project `lab`);
//! - a solo session (project `week`) with two turns, a suggested feature and a command waiting for
//!   approval.
//!
//! Then it serves the page at a fixed address (`url.txt`, ids in `ids.json`) for
//! `FALLOW_VISUAL_SECS` seconds (default 1800). `specs/013-dark-mode/checks/shots.js` drives it.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::json;
use std::path::PathBuf;

const ASK_ME: &str = r#"name = "ask-me"
description = "Standard, asking me when a review fails"

[caps]
task_repairs = 3
review_rounds = 3

[[step]]
agent = "builder"
when = "task"
on_fail = "repair"

[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"

[[step]]
agent = "reviewer"
when = "checkpoint"
on_fail = "engineer"
"#;

fn us1_build() -> String {
    [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
    ]
    .concat()
}

fn session_steps() -> String {
    [
        assistant_step(
            "I fixed **weekDays()**: it now returns all seven days.\n\n- `week.js`: added `\"Sun\"`\n- no tests yet\n\n```js\nweekDays(); // Mon … Sun\n```",
            "when = \"fix the week\"\nfiles = [{ path = \"week.js\", text = \"module.exports = () => ['Mon','Tue','Wed','Thu','Fri','Sat','Sun'];\\n\" }]",
        ),
        "[[step]]\nkind = \"assistant_turn\"\nwhen = \"make it a feature\"\noutput = { reply = \"This is clear enough to specify.\", proposed_feature = { title = \"First day of the week\", brief = \"weekDays() takes an optional first day.\" } }\n\n".to_string(),
        assistant_step("Done.", "when = \"delete the old branch\"\nexec = [\"git branch -D old\"]"),
    ]
    .concat()
}

#[test]
#[ignore]
fn serve_the_013_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(1800);
    std::fs::create_dir_all(root.join("config").join("pipelines")).unwrap();
    std::fs::write(root.join("config/pipelines/ask-me.toml"), ASK_ME).unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        [
            lead_writes_calc(),
            calc_loop_ok(),
            lead_writes_calc(),
            us1_build(),
            check_step("US1", false, "").replacen("result = \"5\"", "result = \"6\"", 1),
            session_steps(),
        ]
        .concat(),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let shop = repo_named(&root, "shop");
    let a = lock_calc_in(tx, &shop, None, Some((5.0, 60)), None);
    wait_for(tx, a, "ready", |v| v["stage"] == "ready");
    let lab = repo_named(&root, "lab");
    let b = lock_calc_in(tx, &lab, Some("ask-me"), Some((5.0, 60)), None);
    wait_for(tx, b, "the engineer asked", |v| !v["needs_engineer"].is_null());

    let week = repo_named(&root, "week");
    std::fs::write(week.join("week.js"), "module.exports = () => ['Mon','Tue','Wed','Thu','Fri','Sat'];\n").unwrap();
    git(&week, &["add", "."]);
    git(&week, &["commit", "-q", "-m", "week"]);
    git(&week, &["branch", "old"]);
    let project = sreq(tx, Req::AddProject { path: week.clone() }).unwrap()["project"].as_str().unwrap().to_string();
    git(&week, &["add", "."]);
    git(&week, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "fix the week");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "make it a feature");
    wait_turn(tx, &s, 2);
    say_solo(tx, &s, "delete the old branch");
    wait_conv(tx, &s, "a command waiting", |v| !v["pending_approval"].is_null());
    sreq(tx, Req::OpenSession { session: s.clone() }).unwrap();

    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    let projects = ask(tx, fallow::run::engine::Request::Data).unwrap()["projects"].clone();
    let ids = json!({ "ready": a, "asks": b, "session": s, "week": project, "projects": projects });
    std::fs::write(root.join("ids.json"), ids.to_string()).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
