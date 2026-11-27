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

/// The most builders the statement says will work at once: "work as 1", the fewest of "up to N"
/// over its groups' largest and "at most M" (the setting).
fn stated(lines: &[String]) -> usize {
    if lines.is_empty() || lines.iter().any(|l| l.ends_with("will work as 1.")) {
        return 1;
    }
    let num = |l: &str, before: &str| -> Option<usize> {
        let (head, _) = l.split_once(before)?;
        head.rsplit(' ').next()?.parse().ok()
    };
    let groups = lines
        .iter()
        .filter_map(|l| num(l, " builders at a time."))
        .max()
        .unwrap();
    lines
        .iter()
        .filter_map(|l| num(l, " builders."))
        .fold(groups, usize::min)
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
    let tasks: Vec<&String> = f.parallel.iter().chain([&f.last]).collect();
    let mut builds = std::collections::BTreeSet::new();
    let (mut now, mut most) = (0usize, 0usize);
    for e in ev {
        let n = e["n"].as_u64();
        match e["type"].as_str() {
            Some("step_queued")
                if e["kind"] == "build"
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

/// Locks the feature with `builders` (3) under `settings`, runs it on the fake runtime and
/// checks the most builds at once against what the view said before the lock (SC-007).
fn runs_as_stated(name: &str, n_parallel: u32, settings: Option<&str>, want: &[&str]) {
    let root = scratch(name);
    setup(&root, 3, settings);
    let f = parallel_tasks_feature(&root, n_parallel, &[]);
    let (e, project, v) = prepare(&root, &f, &whole(&f, "sleep_ms = 1500"));
    let b = pipeline(&v, "builders");
    assert_eq!(b["builders"], 3);
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

#[test]
fn a_group_of_four_runs_as_many_builders_as_the_lock_said() {
    runs_as_stated(
        "018-lock-four",
        4,
        None,
        &["Phase 2 has 4 [P] tasks: up to 3 builders at a time."],
    );
}

#[test]
fn the_setting_for_agents_at_once_is_said_and_holds() {
    runs_as_stated(
        "018-lock-slots",
        4,
        Some("[defaults]\nmax_parallel = 2\n"),
        &[
            "Phase 2 has 4 [P] tasks: up to 3 builders at a time.",
            "Your setting allows 2 agents at once: at most 2 builders.",
        ],
    );
}

#[test]
fn a_single_parallel_task_is_built_alone_as_the_lock_said() {
    runs_as_stated(
        "018-lock-one",
        1,
        None,
        &["No [P] tasks: the pipeline's 3 builders will work as 1."],
    );
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
