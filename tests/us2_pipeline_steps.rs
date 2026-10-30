//! 007 US2 (T045, T048; contracts/pipeline.md, step-results.md): the pipeline's steps around
//! the tasks run. A security reviewer at checkpoints answers in findings, not scenarios; a
//! docs-writer once at the end works on the whole feature before it is ready.

mod common;

use common::*;
use serde_json::Value;

const PIPELINE: &str = r#"name = "with-docs"
description = "Standard, plus a security review and the docs at the end"

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
on_fail = "repair"

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

fn security(story: &str) -> String {
    format!(
        "[[step]]\nkind = \"review\"\npart = {story:?}\nwhen = \"security-reviewer\"\noutput = {{ findings = [{{ severity = \"low\", where = \"calc/__init__.py\", ran = \"python -c 1\", result = \"fine\" }}], summary = \"nothing serious\" }}\n\n"
    )
}

fn steps_of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect()
}

#[test]
fn a_security_review_and_a_once_step_run_where_the_pipeline_says() {
    let root = scratch("pipeline-steps");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("with-docs.toml"), PIPELINE).unwrap();
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
        check_step("US1", true, "when = \"You are the reviewer\""),
        security("US1"),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
        check_step("US2", true, "when = \"You are the reviewer\""),
        security("US2"),
        "[[step]]\nkind = \"make\"\npart = \"docs-writer@once-end\"\nfiles = [{ path = \"README.md\", text = \"# Calc\\n\\nadd() and sub().\\n\" }]\noutput = { status = \"done\", task = \"docs-writer\", files = [\"README.md\"], summary = \"the docs\" }\n\n".to_string(),
    ]
    .concat();
    let (e, repo, id) = locked_calc(&root, &steps, Some("with-docs"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["outcome"][0], true, "{}", v["feature_loop"]["outcome"]);
    // Two checkers per story; the security reviewer's verdict needs no scenario answers.
    for story in ["US1", "US2"] {
        let reviews = steps_of(&v, "review", story);
        assert_eq!(reviews.len(), 2, "{story}");
        assert!(reviews.iter().all(|r| r["verified"][0] == true), "{story}: {reviews:?}");
    }
    // The docs-writer ran once, at the end, and its work is what is merged.
    let docs = steps_of(&v, "make", "docs-writer@once-end");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["verified"][0], true);
    let readme = git(&repo, &["show", &format!("{}:README.md", v["integrated"].as_str().unwrap())]);
    assert!(readme.contains("add() and sub()"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
