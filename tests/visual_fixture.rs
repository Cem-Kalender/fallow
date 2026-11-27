//! 008 T032: the demo loops of the visual check, on the fake runtime. Not a test: run it by
//! hand with `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_fixture -- --ignored`,
//! then serve that folder (`fallow serve --config-dir <root>/config --data-dir <root>/data`).
//! One loop is ready to merge with every story verified; the other waits for the engineer after
//! a criterion failed.
//!
//! 017 T001: with `FALLOW_VISUAL_AGENTS=<n>`, it also writes `<n>` valid checker agents
//! (`agent-01` … `agent-<n>`) to the engineer's `agents/` folder, for the Agents page's list.

mod common;

use common::*;
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
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
    ]
    .concat()
}

/// 017 T001: `n` valid checker agents, `agent-01` … `agent-<n>`, in `dir`.
fn many_agents(dir: &std::path::Path, n: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for i in 1..=n {
        let name = format!("agent-{i:02}");
        std::fs::write(
            dir.join(format!("{name}.md")),
            format!(
                "---\nname: {name}\ndescription: Checks one part of the story ({i} of {n})\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nCheck the part numbered {i}.\n"
            ),
        )
        .unwrap();
    }
}

#[test]
#[ignore]
fn visual_check_loops() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    std::fs::create_dir_all(root.join("config").join("pipelines")).unwrap();
    std::fs::write(root.join("config/pipelines/ask-me.toml"), ASK_ME).unwrap();
    if let Some(n) = std::env::var("FALLOW_VISUAL_AGENTS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        many_agents(&root.join("config").join("agents"), n);
    }
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        [
            lead_writes_calc(),
            calc_loop_ok(),
            lead_writes_calc(),
            us1_build(),
            // What a broken add() returns: the evidence reads like a real failure.
            check_step("US1", false, "").replacen("result = \"5\"", "result = \"6\"", 1),
        ]
        .concat(),
    )
    .unwrap();
    // `fallow serve` uses the same fake agent afterwards.
    let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
    std::fs::write(
        root.join("config/settings.toml"),
        format!(
            "[agents.claude]\ncommand = {exe:?}\nargs = [\"--scenario\", {:?}]\n",
            scenario.display().to_string()
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let shop = repo_named(&root, "shop");
    let a = lock_calc_in(&e.tx, &shop, None, Some((5.0, 60)), None);
    wait_for(&e.tx, a, "ready", |v| v["stage"] == "ready");
    let lab = repo_named(&root, "lab");
    let b = lock_calc_in(&e.tx, &lab, Some("ask-me"), Some((5.0, 60)), None);
    wait_for(&e.tx, b, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    drop(e);
    println!(
        "loops {a} (ready) and {b} (asks the engineer) in {}",
        root.display()
    );
}
