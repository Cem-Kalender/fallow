//! 018 US4 (T046; FR-022, SC-007; research R-11): at the lock, Fallow says what the builders
//! will do. The feature view's `pipelines[]` carry `builders` and `statement`, computed from the
//! same groups the runner forms; a loop locked with that pipeline then runs as many builders at
//! once as the statement said. The lock form shows the chosen pipeline's statement.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;
use std::path::Path;

/// `pipelines/builders.toml` (Standard with `builders` builders) and settings.
fn setup(root: &Path, builders: u32, settings: Option<&str>) {
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("builders.toml"), pipeline_with(builders, "")).unwrap();
    if let Some(s) = settings {
        std::fs::write(root.join("settings.extra.toml"), s).unwrap();
    }
}

/// The scripted steps of a whole loop over the feature (`extra` for each build of phase 2).
fn whole(f: &ParallelFeature, extra: &str) -> String {
    let mut s = f.phase_1();
    for t in &f.parallel {
        s.push_str(&f.build(t, extra));
    }
    s.push_str(&f.build(&f.last, extra));
    s.push_str(&handover_step(2, ""));
    s.push_str(&check_step("US1", true, ""));
    s
}

/// Starts the engine on `steps` (after the lead's turns), works the feature out with the lead
/// and returns the engine, the project and the feature view before the lock.
fn prepare(
    root: &Path,
    f: &ParallelFeature,
    steps: &str,
) -> (fallow::service::ToolEngine, String, Value) {
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(root, &scenario, None);
    let project = sreq(
        &e.tx,
        Req::AddProject {
            path: f.repo.clone(),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-q", "-m", "spec kit"]);
    prepare_feature_in(&e.tx, &project, "Calc", "001-calc");
    let v = sreq(
        &e.tx,
        Req::Feature {
            project: project.clone(),
            feature: "001-calc".into(),
        },
    )
    .unwrap();
    (e, project, v)
}

/// The entry of `pipelines` named `name`.
fn pipeline<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["pipelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no pipeline {name} in {}", v["pipelines"]))
}

fn statement(p: &Value) -> Vec<String> {
    p["statement"]
        .as_array()
        .unwrap_or_else(|| panic!("no statement in {p}"))
        .iter()
        .map(|l| l.as_str().unwrap().to_string())
        .collect()
}

/// The most builders the statement says will work at once: 1 with no statement or "work as 1",
/// else the most "up to N builders at a time" of its lines. Each line is taken as true on its own
/// (US4 review 1): no line may give a number that does not happen.
fn stated(lines: &[String]) -> usize {
    if lines.is_empty() || lines.iter().any(|l| l.ends_with("will work as 1.")) {
        return 1;
    }
    lines
        .iter()
        .filter_map(|l| {
            let (head, _) = l.split_once(" builders at a time")?;
            head.rsplit(' ').next()?.parse().ok()
        })
        .max()
        .unwrap_or_else(|| panic!("no number of builders in {lines:?}"))
}

fn events(root: &Path, id: u64) -> Vec<Value> {
    std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("events.jsonl"),
    )
    .unwrap()
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect()
}

/// The most build steps of phase 2's tasks running at once, from the log.
fn most_at_once(ev: &[Value], f: &ParallelFeature) -> usize {
    most_at_once_after(ev, f, 0)
}

/// As `most_at_once`, over the builds queued after event `after`.
fn most_at_once_after(ev: &[Value], f: &ParallelFeature, after: u64) -> usize {
    let tasks: Vec<&String> = f.parallel.iter().chain([&f.last]).collect();
    let mut builds = std::collections::BTreeSet::new();
    let (mut now, mut most) = (0usize, 0usize);
    for e in ev {
        let n = e["n"].as_u64();
        match e["type"].as_str() {
            Some("step_queued")
                if e["kind"] == "build"
                    && e["seq"].as_u64().is_some_and(|s| s > after)
                    && tasks.iter().any(|t| e["task"].as_str() == Some(t.as_str())) =>
            {
                builds.insert(n.unwrap());
            }
            Some("step_started") if n.is_some_and(|n| builds.contains(&n)) => {
                now += 1;
                most = most.max(now);
            }
            Some("step_finished") if n.is_some_and(|n| builds.contains(&n)) => now -= 1,
            _ => {}
        }
    }
    most
}

#[test]
fn without_parallel_tasks_the_builders_work_as_one() {
    let root = scratch("018-lock-none");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 0, &[]);
    let (e, _project, v) = prepare(&root, &f, "");
    let b = pipeline(&v, "builders");
    assert_eq!(b["builders"], 3);
    assert_eq!(
        statement(b),
        ["No [P] tasks: the pipeline's 3 builders will work as 1."]
    );
    // One builder: nothing about builders (AS3).
    let s = pipeline(&v, "standard");
    assert_eq!(s["builders"], 1);
    assert!(statement(s).is_empty(), "{s}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// Locks the feature with a pipeline of `builders` builders under `settings`, runs it on the fake
/// runtime and checks the most builds at once against what the view said before the lock
/// (SC-007): no statement, or "work as 1", is 1.
fn runs_as_stated(
    name: &str,
    n_parallel: u32,
    builders: u32,
    settings: Option<&str>,
    want: &[&str],
) {
    let root = scratch(name);
    setup(&root, builders, settings);
    let f = parallel_tasks_feature(&root, n_parallel, &[]);
    let (e, project, v) = prepare(&root, &f, &whole(&f, "sleep_ms = 1500"));
    // A first lock: nothing about a loop's recorded pipeline.
    assert!(v["relock"].is_null(), "{}", v["relock"]);
    let b = pipeline(&v, "builders");
    assert_eq!(b["builders"], builders);
    let lines = statement(b);
    assert_eq!(lines, want);
    let id = lock_prepared(
        &e.tx,
        &project,
        "001-calc",
        Some("builders"),
        Some((5.0, 60)),
        None,
        LockExtra::default(),
    );
    wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    assert_eq!(
        most_at_once(&ev, &f),
        stated(&lines),
        "the statement {lines:?} against the run"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

// SC-007 (US4 review 1): every combination of 0, 1 and 4 [P] tasks with 1 and 3 builders runs as
// the lock said, and so does the setting for agents at once.

#[test]
fn no_parallel_task_and_one_builder_runs_as_the_lock_said() {
    runs_as_stated("018-lock-0x1", 0, 1, None, &[]);
}

#[test]
fn a_single_parallel_task_and_one_builder_runs_as_the_lock_said() {
    runs_as_stated("018-lock-1x1", 1, 1, None, &[]);
}

#[test]
fn four_parallel_tasks_and_one_builder_run_as_the_lock_said() {
    runs_as_stated("018-lock-4x1", 4, 1, None, &[]);
}

#[test]
fn no_parallel_task_and_three_builders_run_as_the_lock_said() {
    runs_as_stated(
        "018-lock-0x3",
        0,
        3,
        None,
        &["No [P] tasks: the pipeline's 3 builders will work as 1."],
    );
}

#[test]
fn a_single_parallel_task_is_built_alone_as_the_lock_said() {
    runs_as_stated(
        "018-lock-1x3",
        1,
        3,
        None,
        &["No two [P] tasks in a row: the pipeline's 3 builders will work as 1."],
    );
}

#[test]
fn a_group_of_four_runs_as_many_builders_as_the_lock_said() {
    runs_as_stated(
        "018-lock-4x3",
        4,
        3,
        None,
        &["Phase 2 has 4 [P] tasks: up to 3 builders at a time."],
    );
}

#[test]
fn the_setting_for_agents_at_once_is_said_and_holds() {
    runs_as_stated(
        "018-lock-slots",
        4,
        3,
        Some("[defaults]\nmax_parallel = 2\n"),
        &[
            "Phase 2 has 4 [P] tasks: up to 2 builders at a time, as your setting allows 2 agents at once.",
        ],
    );
}

/// US4 review 1 (decision 1): a new lock of a feature whose loop is live keeps the loop's
/// pipeline. The view gives that recorded pipeline (not the library's current definition) and
/// what it will do for the phases still to come, only the tasks still to build counted; the loop
/// then runs as many builders at once as it said.
#[test]
fn a_new_lock_states_what_the_loops_own_pipeline_will_do_for_what_is_left() {
    use fallow::run::engine::Request;
    let root = scratch("018-lock-again");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    let mut steps = whole(&f, "");
    // After the new lock: the story's repair, T005 and T006 again (their text changed), the
    // handover and the checkpoint.
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    for t in ["T005", "T006"] {
        let file = format!("calc/{}.py", t.to_lowercase());
        let text = format!("NAME = {t:?}\nAGAIN = True\n");
        steps.push_str(&task_step(t, &[(&file, &text)], "sleep_ms = 1500"));
    }
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, project, v) = prepare(&root, &f, &steps);
    assert!(v["relock"].is_null(), "{}", v["relock"]);
    let tx = &e.tx;
    let id = lock_prepared(
        tx,
        &project,
        "001-calc",
        Some("builders"),
        Some((5.0, 60)),
        None,
        LockExtra::default(),
    );
    wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let feature = "001-calc".to_string();
    ask(
        tx,
        Request::Unlock {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    // The library's pipeline changes after the lock: the loop keeps the copy it recorded.
    std::fs::write(
        root.join("config/pipelines/builders.toml"),
        pipeline_with(1, ""),
    )
    .unwrap();
    let path = "specs/001-calc/tasks.md".to_string();
    let doc = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: path.clone(),
        },
    )
    .unwrap();
    let text = doc["text"]
        .as_str()
        .unwrap()
        .replace("Write calc/t005.py", "Write calc/t005.py again")
        .replace("Write calc/t006.py", "Write calc/t006.py again");
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path,
            text,
            version: doc["version"].as_str().unwrap().to_string(),
        },
    )
    .unwrap();
    analyze_again(tx, &project, &feature);
    let v = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    // The library's definition now has one builder: its own statement is empty.
    assert!(statement(pipeline(&v, "builders")).is_empty());
    // The new lock keeps the loop's pipeline, with its 3 builders; phase 1 is done and T004 and
    // T007 are on the build branch: two tasks are left, built two at a time.
    let r = &v["relock"];
    assert_eq!(r["name"], "builders", "{r}");
    assert_eq!(r["builders"], 3, "{r}");
    let lines = statement(r);
    assert_eq!(
        lines,
        ["Phase 2 has 2 [P] tasks still to build: up to 2 builders at a time."]
    );
    // Locked again (any pipeline asked for is not taken: the loop keeps its own).
    ask(
        tx,
        Request::Lock {
            project: project.clone(),
            feature: feature.clone(),
            pipeline: Some("standard".into()),
            level: None,
            budget_usd: None,
            budget_minutes: None,
            when_verified: None,
            update_limit: None,
            run_after: None,
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready again", |v| {
        v["stage"] == "ready" && v["feature_loop"]["lock"] == 2 || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let relocked = ev
        .iter()
        .find(|e| e["type"] == "relocked")
        .and_then(|e| e["seq"].as_u64())
        .expect("the new lock");
    assert_eq!(most_at_once_after(&ev, &f, relocked), stated(&lines));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T049: the lock form shows the chosen pipeline's statement under the Pipeline field, as a
/// status, and follows the select; the option text keeps " (not verified)" (decision 10).
#[test]
fn the_lock_form_shows_the_chosen_pipelines_statement() {
    let js = include_str!("../src/service/web/screens/spec.js");
    assert!(js.contains(r#"<select id="lk-pipeline" data-change="lk-pipeline""#));
    // The statement's lines are a status (008 FR-018), under the select.
    let at = js
        .find(r#"<div id="lk-statement""#)
        .expect("the statement's element");
    let tag = &js[at..at + js[at..].find('>').unwrap()];
    assert!(tag.contains(r#"role="status""#), "{tag}");
    assert!(
        js.contains(".statement"),
        "it reads the pipeline's statement"
    );
    assert!(
        js.contains(r#"if (name === "lk-pipeline")"#),
        "it re-renders on a change of the select"
    );
    assert!(js.contains(r#"p.verifying ? "" : " (not verified)""#));
}

/// US4 review 1 (decision 1): at a new lock of a feature with a live loop, the form shows the
/// loop's recorded pipeline as text, labelled, not as a choice, says a new lock keeps it, and
/// shows its statement for what is left.
#[test]
fn the_lock_form_shows_the_loops_pipeline_at_a_new_lock() {
    let js = include_str!("../src/service/web/screens/spec.js");
    assert!(js.contains("f.relock"), "it reads the recorded pipeline");
    assert!(
        js.contains(r#"id="lk-pipeline-kept""#),
        "the recorded pipeline's element"
    );
    // Labelled: the field is a group named by its "Pipeline" label.
    assert!(js.contains(
        r#"<div class="field" role="group" aria-labelledby="lk-pipeline-label"><span id="lk-pipeline-label">Pipeline</span>"#
    ));
    assert!(js.contains("A new lock keeps the loop's pipeline"));
    assert!(js.contains(r#"r.verifying ? "" : " (not verified)""#));
}
