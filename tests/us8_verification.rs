//! 008 US1 (T003; FR-001 to FR-003; contracts/loop-view.md): the loop view says, per story, what
//! Fallow verified: all checked, a failed criterion with what was expected (kept in view while it
//! is repaired), a finding of another checker (never shown as checked: constitution I), or not
//! checked at all when the pipeline has no checker that answers the criteria.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

fn story<'a>(view: &'a Value, id: &str) -> &'a Value {
    view["checks"]["stories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {}", view["checks"]))
}

fn wait_view(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    id: u64,
    what: &str,
    ok: impl Fn(&Value) -> bool,
) -> Value {
    let start = std::time::Instant::now();
    loop {
        let v = ask(tx, Request::Loop { id }).unwrap();
        if ok(&v) {
            return v;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(90),
            "waited for {what}: {}",
            v["checks"]["stories"]
        );
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

#[test]
fn every_story_checked_shows_its_count_and_no_more() {
    let root = scratch("verif-ok");
    let (e, _repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    let tx = &e.tx;
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let v = ask(tx, Request::Loop { id }).unwrap();
    for id in ["US1", "US2"] {
        let s = story(&v, id);
        assert_eq!(s["state"], "checked", "{s}");
        assert_eq!(
            (s["checked"].as_u64(), s["total"].as_u64()),
            (Some(2), Some(2))
        );
        assert_eq!(s["failed"], 0);
        assert_eq!(s["findings"], false);
        assert_eq!(s["checked_by_pipeline"], true);
        assert_eq!(s["rechecking"], false);
        assert!(
            s["criteria"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["expected"].is_null())
        );
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_criterion_shows_what_was_expected_and_stays_while_it_is_repaired() {
    let root = scratch("verif-fail");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
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
        check_step("US2", false, ""),
        // The repair takes a while, so the failure can be seen while it runs.
        task_step(
            "US2",
            &[("calc/__init__.py", &format!("{CALC_INIT_SUB}# fixed\n"))],
            "sleep_ms = 4000",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_view(tx, id, "US2 failed", |v| {
        v["checks"]["stories"]
            .as_array()
            .is_some_and(|a| a.iter().any(|s| s["id"] == "US2" && s["state"] == "failed"))
    });
    let s = story(&v, "US2");
    assert_eq!(s["failed"], 1, "{s}");
    let c = &s["criteria"][0];
    assert_eq!(c["state"], "failed");
    assert_eq!(c["expected"], "the result is 2", "{c}");
    assert_eq!(story(&v, "US1")["state"], "checked");
    // While the repair runs, the failure stays in view, marked.
    let v = wait_view(tx, id, "the repair under way", |v| {
        v["checks"]["stories"].as_array().is_some_and(|a| {
            a.iter()
                .any(|s| s["id"] == "US2" && s["rechecking"] == true)
        })
    });
    assert_eq!(story(&v, "US2")["state"], "failed");
    // Repaired and checked again: the new round.
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let v = ask(tx, Request::Loop { id }).unwrap();
    let s = story(&v, "US2");
    assert_eq!(s["state"], "checked", "{s}");
    assert_eq!(s["rechecking"], false);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

const WITH_SECURITY: &str = r#"name = "with-security"
description = "Standard, plus a security review"

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
"#;

fn security(story: &str, severity: &str, extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"review\"\npart = {story:?}\nwhen = \"security-reviewer\"\n{extra}\noutput = {{ findings = [{{ severity = {severity:?}, where = \"calc/__init__.py\", ran = \"python -c 1\", result = \"eval of input\" }}], summary = \"checked\" }}\n\n"
    )
}

#[test]
fn another_checkers_finding_is_never_shown_as_checked() {
    let root = scratch("verif-finding");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("with-security.toml"), WITH_SECURITY).unwrap();
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        // Every criterion of US1 passes, but the security reviewer finds something serious.
        check_step("US1", true, "when = \"You are the reviewer\""),
        security("US1", "high", "sleep_ms = 4000"),
        task_step(
            "US1",
            &[("calc/__init__.py", &format!("{CALC_INIT}# safe\n"))],
            "sleep_ms = 4000",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
        check_step("US1", true, "when = \"You are the reviewer\""),
        security("US1", "low", ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, "when = \"You are the reviewer\""),
        security("US2", "low", ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("with-security"));
    let tx = &e.tx;
    // The reviewer passed every criterion; the security reviewer still runs: not checked yet.
    let v = wait_view(tx, id, "the reviewer's verdict", |v| {
        v["checks"]["stories"].as_array().is_some_and(|a| {
            a.iter().any(|s| {
                s["id"] == "US1"
                    && s["checked"] == 2
                    && s["checkers"].as_array().is_some_and(|c| !c.is_empty())
            })
        })
    });
    assert_eq!(
        story(&v, "US1")["state"],
        "progress",
        "never green while a checker of the round has not answered: {}",
        story(&v, "US1")
    );
    let v = wait_view(tx, id, "the finding", |v| {
        v["checks"]["stories"]
            .as_array()
            .is_some_and(|a| a.iter().any(|s| s["id"] == "US1" && s["findings"] == true))
    });
    let s = story(&v, "US1");
    assert_eq!(s["failed"], 0, "every criterion passed: {s}");
    assert_eq!(s["state"], "failed", "never green with a finding: {s}");
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let v = ask(tx, Request::Loop { id }).unwrap();
    assert_eq!(story(&v, "US1")["state"], "checked");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pipeline_without_a_checker_shows_every_story_not_checked() {
    let root = scratch("verif-solo");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("solo"));
    let tx = &e.tx;
    wait_for(tx, id, "done, not verified", |v| {
        v["stage"] == "ready" && v["feature_loop"]["outcome"][0] == false
    });
    let v = ask(tx, Request::Loop { id }).unwrap();
    for id in ["US1", "US2"] {
        let s = story(&v, id);
        assert_eq!(s["state"], "not_checked", "{s}");
        assert_eq!(s["checked_by_pipeline"], false);
        assert_ne!(s["state"], "checked", "never checked without a checker");
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A verdict that passes every criterion but is incomplete (a criterion without what was run):
/// Fallow asks again, and the story is never shown as checked meanwhile (constitution I).
#[test]
fn an_incomplete_verdict_is_never_shown_as_checked() {
    let root = scratch("verif-incomplete");
    let incomplete = "[[step]]\nkind = \"review\"\npart = \"US1\"\nsleep_ms = 3000\noutput = { answers = [{ criterion = \"US1/1\", ran = \"\", result = \"5\", passed = true }, { criterion = \"US1/2\", ran = \"python -c add('a', 1)\", result = \"TypeError\", passed = true, failing_case = \"add('a', 1) is refused\" }], summary = \"checked\" }\n\n";
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        incomplete.to_string(),
        // Asked again: this time the whole verdict, slowly, so the first can be looked at.
        check_step("US1", true, "sleep_ms = 3000"),
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
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_view(tx, id, "the incomplete verdict", |v| {
        v["checks"]["stories"].as_array().is_some_and(|a| {
            a.iter().any(|s| {
                s["id"] == "US1" && s["checkers"].as_array().is_some_and(|c| !c.is_empty())
            })
        })
    });
    let s = story(&v, "US1");
    assert_ne!(s["state"], "checked", "{s}");
    assert_eq!(
        s["criteria"][0]["state"], "not yet",
        "no `ran`: not checked: {s}"
    );
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let v = ask(tx, Request::Loop { id }).unwrap();
    assert_eq!(story(&v, "US1")["state"], "checked");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_checker_for_some_paths_that_did_not_pass_is_never_shown_as_checked() {
    // The security reviewer runs alongside the reviewer only when calc/ changed, and cannot
    // start; the reviewer passes every criterion. The story is not checked (constitution I).
    let root = scratch("verif-paths");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("with-security.toml"),
        WITH_SECURITY
            .replace(
                "agent = \"reviewer\"\nwhen = \"checkpoint\"\n",
                "agent = \"reviewer\"\nwhen = \"checkpoint\"\nparallel_with = [\"security-reviewer\"]\n",
            )
            .replace(
                "agent = \"security-reviewer\"\nwhen = \"checkpoint\"\n",
                "agent = \"security-reviewer\"\nwhen = \"checkpoint\"\npaths = [\"calc/**\"]\n",
            ),
    )
    .unwrap();
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "security-reviewer")
        .unwrap()
        .1;
    std::fs::write(
        agents.join("security-reviewer.md"),
        on_fake(shipped, "fake-1"),
    )
    .unwrap();
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", true, "when = \"You are the reviewer\""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("with-security"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let v = ask(tx, Request::Loop { id }).unwrap_or(v);
    let s = story(&v, "US1");
    assert_eq!(s["checked"], 2, "the reviewer passed every criterion: {s}");
    assert_ne!(
        s["state"], "checked",
        "never green while a checker did not pass: {s}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
