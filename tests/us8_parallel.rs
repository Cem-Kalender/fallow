//! 008 US5 (T023; FR-014 to FR-016, SC-004; research R-7): checkers a pipeline marks to run
//! alongside each other at a story's checkpoint run at the same time, within the slot cap; the
//! checkpoint is judged when all of them have finished; their findings go to one repair, after
//! which both run again; each resumes in its own session after a restart.

mod common;

use common::*;
use fallow::run::engine::{Msg, Request};
use serde_json::Value;
use std::path::Path;

const PARALLEL: &str = r#"name = "parallel"
description = "Standard, with a security review alongside the review"

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
parallel_with = ["security-reviewer"]

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

fn reviewer(story: &str, passed: bool, extra: &str) -> String {
    check_step(
        story,
        passed,
        &format!("when = \"You are the reviewer\"\n{extra}"),
    )
}

/// The first two phases (US1's build) as scripted steps.
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

fn us2() -> String {
    [
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        reviewer("US2", true, ""),
        security("US2", "low", ""),
    ]
    .concat()
}

fn setup(root: &Path, extra_settings: Option<&str>) {
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("parallel.toml"), PARALLEL).unwrap();
    if let Some(s) = extra_settings {
        std::fs::write(root.join("settings.extra.toml"), s).unwrap();
    }
}

fn reviews<'a>(v: &'a Value, story: &str) -> Vec<&'a Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == "review" && s["part"] == story)
        .collect()
}

fn time(s: &Value, k: &str) -> String {
    s[k].as_str().unwrap_or_default().to_string()
}

#[test]
fn checkers_alongside_each_other_overlap_and_are_judged_together() {
    let root = scratch("parallel-overlap");
    setup(&root, None);
    let steps = [
        us1_build(),
        reviewer("US1", true, "sleep_ms = 4000"),
        security("US1", "low", "sleep_ms = 4000"),
        us2(),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let tx = &e.tx;
    // Both run at once: the loop says so.
    let v = wait_for(tx, id, "both checkers running", |v| {
        v["busy"].as_str().is_some_and(|b| b.starts_with("steps "))
    });
    let running = reviews(&v, "US1");
    assert_eq!(running.len(), 2, "{running:?}");
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    let r = reviews(&v, "US1");
    assert_eq!(r.len(), 2, "each ran once");
    let (a, b) = (r[0], r[1]);
    assert!(
        time(a, "started") < time(b, "finished") && time(b, "started") < time(a, "finished"),
        "they overlap in time: {a} {b}"
    );
    // SC-004: the checkpoint took the slower checker's time plus at most 10%, not both
    // (8 s); one second more for the timestamps' resolution.
    let secs = |t: String| fallow::run::time::parse_rfc3339(&t).unwrap() as f64;
    let span = secs(time(a, "finished").max(time(b, "finished")))
        - secs(time(a, "started").min(time(b, "started")));
    let slower = [a, b]
        .iter()
        .map(|s| s["duration_ms"].as_f64().unwrap() / 1000.0)
        .fold(0.0, f64::max);
    assert!(
        span <= slower * 1.1 + 1.0,
        "the checkpoint took {span}s for two checkers, the slower {slower}s"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn both_failing_lead_to_one_repair_with_both_findings() {
    let root = scratch("parallel-repair");
    setup(&root, None);
    let steps = [
        us1_build(),
        reviewer("US1", false, "sleep_ms = 2000"),
        security("US1", "high", "sleep_ms = 2000"),
        task_step(
            "US1",
            &[("calc/__init__.py", &format!("{CALC_INIT}# safe\n"))],
            "",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
        reviewer("US1", true, ""),
        security("US1", "low", ""),
        us2(),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let repairs: Vec<&Value> = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == "repair" && s["part"] == "US1")
        .collect();
    assert_eq!(repairs.len(), 1, "one repair for both: {repairs:?}");
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", repairs[0]["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("US1/1") && prompt.contains("eval of input"),
        "the repair holds both checkers' findings: {prompt}"
    );
    assert_eq!(
        reviews(&v, "US1").len(),
        4,
        "both ran again after the repair"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn one_slot_runs_them_one_after_the_other_with_the_same_outcome() {
    let root = scratch("parallel-one-slot");
    setup(&root, Some("[defaults]\nmax_parallel = 1\n"));
    let steps = [
        us1_build(),
        reviewer("US1", true, "sleep_ms = 2000"),
        security("US1", "low", "sleep_ms = 2000"),
        us2(),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    let r = reviews(&v, "US1");
    let (a, b) = (r[0], r[1]);
    // Must fail: with one slot they never overlap.
    assert!(
        time(b, "started") >= time(a, "finished") || time(a, "started") >= time(b, "finished"),
        "one after the other: {a} {b}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_restart_while_both_run_resumes_each_in_its_own_session() {
    let root = scratch("parallel-restart");
    setup(&root, None);
    let again = "when = \"You were interrupted\"";
    let steps = [
        us1_build(),
        // At work when Fallow stops: each has said something, so its program surely ran (one
        // whose program had not run yet is given its instruction again instead).
        reviewer("US1", true, "say = \"Checking\"\nsleep_ms = 30000"),
        security("US1", "low", "say = \"Checking\"\nsleep_ms = 30000"),
        // Either checker may take either resumed step: each answers both ways.
        check_step("US1", true, again).replace(
            "summary = \"checked\"",
            "findings = [], summary = \"checked\"",
        ),
        check_step("US1", true, again).replace(
            "summary = \"checked\"",
            "findings = [], summary = \"checked\"",
        ),
        us2(),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let v = wait_for(&e.tx, id, "both checkers running", |v| {
        v["busy"].as_str().is_some_and(|b| b.starts_with("steps "))
    });
    for s in reviews(&v, "US1") {
        let n = s["n"].as_u64().unwrap() as u32;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while ask(&e.tx, Request::Activity { id, n })
            .ok()
            .and_then(|a| a.as_array().map(|a| a.is_empty()))
            .unwrap_or(true)
        {
            assert!(
                std::time::Instant::now() < end,
                "step {n} never got to work"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    assert_eq!(
        v["feature_loop"]["outcome"][0], true,
        "{}",
        v["feature_loop"]["outcome"]
    );
    let r = reviews(&v, "US1");
    assert_eq!(r.len(), 4, "two interrupted, two resumed: {r:?}");
    for agent in ["reviewer", "security-reviewer"] {
        let mine: Vec<&&Value> = r.iter().filter(|s| s["def"] == agent).collect();
        assert_eq!(mine.len(), 2, "{agent}: {mine:?}");
        assert_eq!(
            mine[1]["resume"], mine[0]["session"],
            "{agent} resumed in its own session"
        );
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_question_from_one_checker_is_answered_while_the_other_runs() {
    let root = scratch("parallel-ask");
    setup(&root, None);
    let lead = "[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = { outcome = \"answer\", answer = \"Ints only\", basis = \"spec.md US1 scenario 2\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false }\n\n";
    let steps = [
        us1_build(),
        reviewer("US1", true, "ask = \"Floats too?\""),
        lead.to_string(),
        security("US1", "low", "sleep_ms = 5000"),
        us2(),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    assert_eq!(
        v["feature_loop"]["outcome"][0], true,
        "{}",
        v["feature_loop"]["outcome"]
    );
    let q = &v["feature_loop"]["questions"]["q1"];
    assert_eq!(q["by"], "lead", "{q}");
    assert_eq!(q["state"], "answered");
    // The reviewer got its answer within its step: one review each.
    let r = reviews(&v, "US1");
    assert_eq!(r.len(), 2);
    // ... while the security reviewer was still running.
    let sec = r.iter().find(|s| s["def"] == "security-reviewer").unwrap();
    let answered = q["history"].as_array().unwrap().last().unwrap()["at"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        answered < time(sec, "finished") && answered >= time(sec, "started"),
        "answered at {answered}, while {sec}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_checker_that_cannot_start_is_asked_again_and_the_other_goes_on() {
    let root = scratch("parallel-no-start");
    setup(&root, None);
    // The security reviewer is on a runtime the settings do not configure.
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "security-reviewer")
        .unwrap()
        .1;
    std::fs::write(
        agents.join("security-reviewer.md"),
        shipped.replace("runtime: claude", "runtime: fake"),
    )
    .unwrap();
    let steps = [us1_build(), reviewer("US1", true, "sleep_ms = 2000"), us2()].concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("parallel"));
    let v = wait_for(&e.tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.contains("security-reviewer gave no usable verdict") && why.contains("cannot start"),
        "{why}"
    );
    let r = reviews(&v, "US1");
    // The reviewer ran to its end; the security reviewer was asked again, never started.
    let ran: Vec<&&Value> = r.iter().filter(|s| s["def"] == "reviewer").collect();
    assert_eq!(ran.len(), 1, "{r:?}");
    assert!(!time(ran[0], "finished").is_empty(), "{r:?}");
    let tried: Vec<&&Value> = r
        .iter()
        .filter(|s| s["def"] == "security-reviewer")
        .collect();
    // Tried as often as a checker that did not finish (CHECK_TRIES), then the engineer.
    assert_eq!(tried.len(), 3, "asked again: {r:?}");
    assert!(tried.iter().all(|s| time(s, "started").is_empty()), "{r:?}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_checker_on_its_own_that_cannot_start_stops_the_loop_as_before() {
    // Without parallel_with, 007 FR-029 holds: a runtime that is not there stops the loop at
    // the step that needs it, naming it, even when another checker passed before it.
    let root = scratch("sequential-no-start");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("sequential.toml"),
        PARALLEL
            .replace("name = \"parallel\"", "name = \"sequential\"")
            .replace("parallel_with = [\"security-reviewer\"]\n", ""),
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
        shipped.replace("runtime: claude", "runtime: fake"),
    )
    .unwrap();
    let steps = [us1_build(), reviewer("US1", true, ""), us2()].concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("sequential"));
    let v = wait_for(&e.tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.contains("security-reviewer") && why.contains("\\\"fake\\\""),
        "{why}"
    );
    assert!(!why.contains("no usable verdict"), "{why}");
    let r = reviews(&v, "US1");
    let tried: Vec<&&Value> = r
        .iter()
        .filter(|s| s["def"] == "security-reviewer")
        .collect();
    assert_eq!(tried.len(), 1, "not asked again: {r:?}");
    assert!(time(tried[0], "finished").is_empty(), "{r:?}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
