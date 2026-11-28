//! 018 US1 (T008; FR-001 to FR-003, FR-021; contracts/web-api.md): the Pipelines tab says what a
//! pipeline checks in plain sentences, without "verifying"/"not verifying" labels; the service
//! still gives `verifying` and `missing`; a loop on Solo still ends as "Done, not verified by
//! Fallow" with what was not checked (must fail as verified).

mod common;

use common::*;
use fallow::run::events::{Event, Log};
use fallow::studio::Req;
use serde_json::Value;

const PIPELINES_JS: &str = include_str!("../src/service/web/screens/pipelines.js");

/// A pipeline whose only checker runs at the end of every phase.
const PHASE_ONLY: &str = r#"name = "phase-only"

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
when = "phase"
on_fail = "repair"
"#;

fn pipeline<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["pipelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or_else(|| panic!("no pipeline {name} in {v}"))
}

fn checks(v: &Value) -> Vec<String> {
    v["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("no checks in {v}"))
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_service_says_what_each_pipeline_checks_and_keeps_verifying() {
    let root = scratch("018-designer");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("phase-only.toml"), PHASE_ONLY).unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let v = sreq(&e.tx, Req::Pipelines { project: None }).unwrap();

    let std = pipeline(&v, "standard");
    assert_eq!(std["verifying"], true);
    assert_eq!(std["missing"], serde_json::json!([]));
    assert_eq!(
        checks(std),
        [
            "Fallow runs the gates after every task.",
            "The reviewer checks every user story's acceptance scenarios at its checkpoint.",
        ]
    );
    let solo = pipeline(&v, "solo");
    assert_eq!(solo["verifying"], false);
    assert_eq!(
        solo["missing"],
        serde_json::json!(["no review at checkpoints"])
    );
    assert_eq!(checks(solo), ["Fallow runs the gates after every task."]);
    let phase = pipeline(&v, "phase-only");
    assert_eq!(
        phase["verifying"], false,
        "a checker per phase checks no story"
    );
    assert_eq!(
        phase["missing"],
        serde_json::json!(["no review at checkpoints"])
    );
    assert_eq!(
        checks(phase)[1],
        "The reviewer checks the work at the end of every phase."
    );
    assert_eq!(pipeline(&v, "standard")["caps"]["builders"], 1);

    // The designer's check and save carry them too.
    let c = sreq(
        &e.tx,
        Req::CheckPipeline {
            project: None,
            toml: PHASE_ONLY.into(),
        },
    )
    .unwrap();
    assert_eq!(c["verifying"], false);
    assert_eq!(checks(&c).len(), 2, "{c}");
    let saved = sreq(
        &e.tx,
        Req::PutPipeline {
            name: "phase-only".into(),
            scope: "user".into(),
            toml: PHASE_ONLY.into(),
        },
    )
    .unwrap();
    assert_eq!(
        saved["missing"],
        serde_json::json!(["no review at checkpoints"])
    );
    assert_eq!(checks(&saved).len(), 2, "{saved}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_pipelines_tab_shows_no_verifying_label() {
    let js = PIPELINES_JS;
    for gone in [
        r#"pill green">verifying"#,
        r#"pill orange">not verifying"#,
        r#"" (not verified)""#,
        "<b>Not verifying.</b>",
    ] {
        assert!(!js.contains(gone), "the Pipelines tab still has {gone:?}");
    }
    assert!(!js.contains("(not verified)"), "nor in \"Based on\"");
    // It states the checks and what a result says when something is not checked.
    assert!(js.contains(".checks"), "the designer shows `checks`");
    assert!(
        js.contains(r#"A result of this pipeline ends as "Done, not verified by Fallow": "#),
        "the designer says what a result will say"
    );
    // The lock form keeps its "(not verified)" (research R-12).
    let spec = include_str!("../src/service/web/screens/spec.js");
    assert!(spec.contains("(not verified)"));
}

#[test]
fn a_loop_on_solo_ends_done_not_verified() {
    let root = scratch("018-solo");
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
    let v = wait_for(tx, id, "done, not verified", |v| v["stage"] == "ready");
    assert_eq!(
        v["feature_loop"]["outcome"],
        serde_json::json!([false, ["no review at checkpoints"]]),
        "must fail as verified"
    );
    let dir = fallow::run::engine::runs_dir(&root.join("data")).join(id.to_string());
    let (_, events, _) = Log::<Event>::open(&dir).unwrap();
    let outcome = events.iter().find_map(|l| match &l.event {
        Event::Outcome { verified, missing } => Some((*verified, missing.clone())),
        _ => None,
    });
    assert_eq!(
        outcome,
        Some((false, vec!["no review at checkpoints".to_string()]))
    );
    let why = events
        .iter()
        .rev()
        .find_map(|l| match &l.event {
            Event::Stage { why, .. } => Some(why.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        why,
        "every phase is done, not verified by Fallow: no review at checkpoints"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
