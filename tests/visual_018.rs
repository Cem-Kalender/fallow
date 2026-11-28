//! 018 T056 (quickstart 2): the Pipelines tab, for
//! `specs/018-pipeline-orchestration/checks/pipelines.js`. Not a test: run it by hand with
//! `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_018 --features automation --
//! --ignored --nocapture`. It installs the shipped pipelines (Standard, Test first, Solo and
//! `parallel`), adds two checkers of its own and a pipeline `every-point` with a step at every
//! point (checkers alongside each other at several of them), adds a project, then serves the
//! page at a fixed address (`url.txt`) for `FALLOW_VISUAL_SECS` seconds (default 1800).

mod common;

use common::*;
use fallow::studio::Req;
use std::path::PathBuf;

/// A step at every point of FR-004: a checker once at the start and at a phase's and a
/// checkpoint's start (each asking the engineer), a maker before each task, the builder and the
/// gates, two checkers alongside each other after each task and at each phase's end, the
/// checkpoint's review alongside the security review, and a maker once at the end.
const EVERY_POINT: &str = r#"name = "every-point"
description = "A step at every point, checkers alongside each other at several"

[caps]
builders = 3

[[step]]
agent = "ui-checker"
when = "once"
at = "start"
on_fail = "engineer"

[[step]]
agent = "ui-checker"
when = "phase"
at = "start"
on_fail = "engineer"

[[step]]
agent = "test-writer"
when = "task"
at = "start"
on_fail = "repair"

[[step]]
agent = "builder"
when = "task"
on_fail = "repair"

[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"

[[step]]
agent = "perf-checker"
when = "task"
at = "end"
on_fail = "repair"
parallel_with = ["security-reviewer"]

[[step]]
agent = "security-reviewer"
when = "task"
at = "end"
on_fail = "repair"

[[step]]
agent = "perf-checker"
when = "phase"
at = "end"
on_fail = "repair"
parallel_with = ["ui-checker"]

[[step]]
agent = "ui-checker"
when = "phase"
at = "end"
on_fail = "repair"

[[step]]
agent = "perf-checker"
when = "checkpoint"
at = "start"
on_fail = "engineer"

[[step]]
agent = "reviewer"
when = "checkpoint"
on_fail = "repair"
parallel_with = ["security-reviewer"]

[[step]]
agent = "security-reviewer"
when = "checkpoint"
on_fail = "repair"

[[step]]
agent = "docs-writer"
when = "once"
at = "end"
on_fail = "engineer"
"#;

fn checker(name: &str, what: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {what}\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\n{what}.\n"
    )
}

#[test]
#[ignore]
fn serve_the_018_pipelines() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    fallow::defs::install_defaults(&config).unwrap();
    for (name, what) in [
        ("ui-checker", "Checks the pages by eye"),
        ("perf-checker", "Checks the timings stay within budget"),
    ] {
        std::fs::write(
            config.join(format!("agents/{name}.md")),
            checker(name, what),
        )
        .unwrap();
    }
    std::fs::write(config.join("pipelines/every-point.toml"), EVERY_POINT).unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let shop = repo_named(&root, "shop");
    sreq(&e.tx, Req::AddProject { path: shop }).unwrap();
    // Every pipeline loads, `every-point` too.
    let v = sreq(&e.tx, Req::Pipelines { project: None }).unwrap();
    assert!(
        v["problems"].as_array().is_some_and(|p| p.is_empty()),
        "{}",
        v["problems"]
    );
    for name in ["standard", "solo", "parallel", "every-point"] {
        assert!(
            v["pipelines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == name),
            "{name}"
        );
    }
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
