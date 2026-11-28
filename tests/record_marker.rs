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
    // The engineer's builder names a model with the marker in it, in a form the agent file
    // check accepts (017 review 1, FR-022): the file is in force and the loop runs with it,
    // but the run record keeps no model for it, as the name can carry the engineer's words.
    std::fs::create_dir_all(repo.join(".fallow/agents")).unwrap();
    let builder = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "builder")
        .unwrap()
        .1;
    let marked_model = format!("claude-sonnet-{MARKER}");
    std::fs::write(
        repo.join(".fallow/agents/builder.md"),
        replace_found(
            builder,
            "model: claude-sonnet-5-5",
            &format!("model: {marked_model}"),
        ),
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
    let lib = fallow::defs::Library::load(Some(&root.join("config")), Some(&repo));
    assert!(lib.unusable.is_empty(), "{:?}", lib.problems);
    assert_eq!(lib.agents["builder"].model, marked_model);
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
    merge_when_read(tx, id).unwrap();
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
    // The builder in force names the marked model: its row keeps no model.
    let builder_row = r["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent"] == "builder")
        .unwrap();
    assert_eq!(builder_row["model"], serde_json::Value::Null, "{r}");
    // Another agent's shipped model, a recognisable provider name, is kept.
    let reviewer_row = r["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent"] == "reviewer")
        .unwrap();
    assert_eq!(reviewer_row["model"], "claude-sonnet-5-5", "{r}");
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

/// 018 (T033; FR-026, research R-15): the record of a loop with a group of [P] tasks counts its
/// builders and overlaps in numbers, and still holds none of the loop's text; a record written
/// before 018 reads.
#[test]
fn a_record_of_a_loop_with_builders_holds_numbers_only() {
    let root = scratch("record-builders");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("builders.toml"), pipeline_with(3, "")).unwrap();
    let mut f = parallel_tasks_feature(
        &root,
        3,
        &[("T004", "calc/shared.py"), ("T006", "calc/shared.py")],
    );
    f.lead = f.lead.replace("Join them", &format!("Join them {MARKER}"));
    let mut steps = f.phase_1();
    for t in &f.parallel {
        let extra = if t == "T006" {
            "sleep_ms = 1500"
        } else {
            "sleep_ms = 500"
        };
        steps.push_str(&f.build(t, extra).replacen(
            "summary = \"did",
            &format!("summary = \"{MARKER} did"),
            1,
        ));
    }
    steps.push_str(
        &task_step(
            "T006",
            &[("calc/shared.py", &format!("# {MARKER}\nBY = 'both'\n"))],
            "",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &f.repo, Some("builders"), Some((5.0, 60)), None);
    wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    merge_when_read(tx, id).unwrap();
    wait_for(tx, id, "merged", |v| v["stage"] == "done");
    let record = root.join("data/records/v1").join(format!("loop-{id}.json"));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !record.exists() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(&record).unwrap();
    assert!(!text.to_lowercase().contains(MARKER), "{text}");
    // No task id (T001…): matched as `T` and three digits, so a time such as "…T00:08:47Z"
    // (a run just after midnight UTC) is not taken for one.
    let task_id = regex::Regex::new(r"T\d{3}").unwrap();
    assert!(!text.contains("calc/") && !task_id.is_match(&text), "{text}");
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["builders_cap"], 3, "{r}");
    assert_eq!(r["builders_most"], 3, "{r}");
    assert_eq!(r["groups_parallel"], 1, "{r}");
    assert_eq!(r["overlap_repairs"], 1, "{r}");
    // A record written before 018, without these, still reads (and lists).
    let mut old = r.clone();
    for k in [
        "builders_cap",
        "builders_most",
        "groups_parallel",
        "overlap_repairs",
    ] {
        old.as_object_mut().unwrap().remove(k);
    }
    std::fs::write(
        root.join("data/records/v1/loop-999.json"),
        serde_json::to_string(&old).unwrap(),
    )
    .unwrap();
    let listed = fallow::measure::record::list(&root.join("data"));
    let before = listed
        .iter()
        .find(|(n, _)| *n == 999)
        .expect("the old record reads");
    assert_eq!(before.1.builders_cap, 0);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
