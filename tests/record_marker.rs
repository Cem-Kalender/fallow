//! 007 T081 (research S-20; constitution V): marker text planted in a loop's documents, its
//! task, the agents' output, a question and its answer, and the names the engineer chose never
//! appears in the loop's run record, which holds numbers, identifiers and fixed values only.

mod common;

use common::*;
use fallow::run::engine::Request;

/// Lower case, so that it also fits where only names fit (a pipeline's name).
const MARKER: &str = "mk5f0c2dx";

#[test]
fn a_record_holds_no_text_of_the_loop() {
    let root = scratch("record-marker");
    let lead = lead_writes_calc()
        .replace(
            "# Feature Specification: Calc",
            &format!("# Feature Specification: Calc {MARKER}"),
        )
        .replace(
            "Create the calc package",
            &format!("Create the calc package {MARKER}"),
        );
    let question = format!("Should add() accept {MARKER} floats?");
    let steps = [
        lead,
        task_step("T001", &[("calc/__init__.py", &format!("# {MARKER}\n"))], &format!("say = \"Setting up {MARKER}\"")),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], &format!("ask = {question:?}"))
            .replacen("status = \"done\"", "status = \"blocked\", question = \"$ANSWER\"", 1)
            .replacen("summary = \"did", &format!("summary = \"{MARKER} did"), 1),
        format!("[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = {{ outcome = \"relay\", answer = \"{MARKER}\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{{ label = \"{MARKER} no\", recommended = true }}] }}\n\n"),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], "when = \"The engineer answers\""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
        check_step("US1", true, ""),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, steps).unwrap();
    // The project and the pipeline carry the marker in their names too.
    let repo = repo_named(&root, &format!("shop-{MARKER}"));
    std::fs::create_dir_all(repo.join(".fallow/pipelines")).unwrap();
    // The builder runs on a model alias the engineer chose, with the marker in it.
    std::fs::create_dir_all(repo.join(".fallow/agents")).unwrap();
    let builder = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "builder")
        .unwrap()
        .1;
    std::fs::write(
        repo.join(".fallow/agents/builder.md"),
        builder.replace("model: sonnet", &format!("model: sonnet-{MARKER}")),
    )
    .unwrap();
    let name = format!("std-{MARKER}");
    std::fs::write(
        repo.join(".fallow/pipelines").join(format!("{name}.toml")),
        fallow::defs::DEFAULT_PIPELINES[0]
            .1
            .replace("name = \"standard\"", &format!("name = \"{name}\"")),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo, Some(&name), Some((5.0, 60)), None);
    wait_for(tx, id, "the question", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
    });
    ask(
        tx,
        Request::QuestionAnswer {
            id,
            q: "q1".into(),
            answer: format!("No {MARKER}, ints only"),
        },
    )
    .unwrap();
    ask(
        tx,
        Request::Note {
            id,
            text: format!("Mind {MARKER}"),
            agent: None,
        },
    )
    .unwrap();
    wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let record = root.join("data/records/v1").join(format!("loop-{id}.json"));
    assert!(!record.exists(), "a loop is recorded when it ends");
    ask(tx, Request::Merge { id }).unwrap();
    wait_for(tx, id, "merged", |v| v["stage"] == "done");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !record.exists() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(&record).unwrap();
    assert!(
        !text.to_lowercase().contains(MARKER),
        "the record holds no text of the loop: {text}"
    );
    assert!(
        !text.contains(&root.display().to_string()) && !text.contains("calc/"),
        "nor paths: {text}"
    );
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["pipeline"], "custom");
    assert_eq!(r["outcome"], "merged");
    assert_eq!(r["tasks"], 4);
    assert_eq!(r["tasks_accepted"], 4);
    assert_eq!(r["phases"], 3);
    assert_eq!(r["questions"]["relayed"], 1);
    assert_eq!(r["verifying"], true);
    assert!(
        r["interventions"].as_u64().unwrap() >= 2,
        "the answer and the note: {r}"
    );
    assert!(
        r["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["agent"] == "fallow:gates")
    );
    // Written once: the record of an ended loop does not change, whatever comes after.
    let _ = ask(
        tx,
        Request::Note {
            id,
            text: "after the end".into(),
            agent: None,
        },
    );
    std::thread::sleep(std::time::Duration::from_secs(2));
    let again = std::fs::read_to_string(&record).unwrap();
    assert_eq!(again, text);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
