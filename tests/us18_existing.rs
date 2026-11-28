//! 018 US5 (T051, T052; FR-025 to FR-027): pipelines and loops as Fallow 0.4.0 left them, after
//! the update. Every pipeline file loads with the same steps at the same points, one builder and
//! the same result rule; nothing is rewritten; the new `parallel` default is added; a file 0.4.0
//! refused for a reason 018 keeps is still reported. A loop on 0.4.0's Standard stopped
//! mid-phase resumes without repeating a step and ends verified; a finished Solo loop shows as
//! before; a pipeline saved from the designer without 018's fields names none of them.

mod common;

use common::*;
use fallow::defs::{DEFAULT_AGENTS, DEFAULT_PIPELINES, fingerprint};
use fallow::run::engine::{Msg, Request};
use fallow::run::events::{Event, Log};
use fallow::studio::Req;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// The fingerprints `.shipped.json` held for the shipped pipelines in Fallow 0.4.0 (`git show
/// v0.4.0:assets/pipelines/<name>.toml`, FNV-1a 64 as `defs::fingerprint`).
const V040: [(&str, &str, &str); 3] = [
    ("standard", "486a2eb76024921d", PIPELINE_040_STANDARD),
    ("test-first", "35220359c3a4d8cc", PIPELINE_040_TEST_FIRST),
    ("solo", "a66a736061d70f6a", PIPELINE_040_SOLO),
];

/// The engineer's edited copy of Test first, valid in 0.4.0.
const TEST_FIRST_EDITED_040: &str = r#"name = "test-first"
description = "Our test first: acceptance tests first, two review rounds"

[caps]
task_repairs = 5
review_rounds = 2

[[step]]
agent = "test-writer"
when = "checkpoint"
at = "start"
on_fail = "engineer"

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
"#;

/// The engineer's own pipeline: a path-limited security reviewer alongside the reviewer.
const SECURE_040: &str = r#"name = "secure"
description = "Standard, plus a security review when api/ or auth/ changes"

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
paths = ["api/**", "auth/**"]
parallel_with = ["reviewer"]
on_fail = "repair"
"#;

/// A project's pipeline: a docs-writer once at the end.
const WITH_DOCS_040: &str = r#"name = "with-docs"
description = "Standard, and the docs written once at the end"

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
agent = "docs-writer"
when = "once"
at = "end"
on_fail = "engineer"
"#;

/// Invalid in 0.4.0 for a reason 018 keeps: an agent that does not exist.
const GHOST_040: &str = r#"name = "ghost-review"

[[step]]
agent = "builder"
when = "task"
on_fail = "repair"

[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"

[[step]]
agent = "ghost"
when = "checkpoint"
on_fail = "repair"
"#;

/// The steps v0.4.0's `parse` gave for each pipeline, as the service lists them.
fn steps_040(name: &str) -> Value {
    let builder = json!({"agent": "builder", "when": "task", "on_fail": "repair"});
    let gates = json!({"agent": "fallow:gates", "when": "task", "on_fail": "repair"});
    let reviewer = json!({"agent": "reviewer", "when": "checkpoint", "on_fail": "repair"});
    let test_writer =
        json!({"agent": "test-writer", "when": "checkpoint", "at": "start", "on_fail": "engineer"});
    match name {
        "standard" => json!([builder, gates, reviewer]),
        "solo" => json!([builder, gates]),
        "test-first" => json!([test_writer, builder, gates, reviewer]),
        "secure" => json!([builder, gates, reviewer, {
            "agent": "security-reviewer", "when": "checkpoint", "paths": ["api/**", "auth/**"],
            "on_fail": "repair", "parallel_with": ["reviewer"],
        }]),
        "with-docs" => json!([builder, gates, reviewer, {
            "agent": "docs-writer", "when": "once", "at": "end", "on_fail": "engineer",
        }]),
        _ => unreachable!("{name}"),
    }
}

/// Every file under `dirs`, with its bytes.
fn files(dirs: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for d in dirs {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            if e.path().is_file() {
                out.insert(e.path(), std::fs::read(e.path()).unwrap());
            }
        }
    }
    out
}

fn listed<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["pipelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no pipeline {name} in {v}"))
}

/// The config folder as 0.4.0 left it: every shipped agent and pipeline copied and recorded,
/// Test first edited, the engineer's own `secure` and an invalid `ghost-review`.
fn config_040(config: &Path) {
    let mut record = BTreeMap::new();
    std::fs::create_dir_all(config.join("agents")).unwrap();
    std::fs::create_dir_all(config.join("pipelines")).unwrap();
    // The shipped agents did not change in 018.
    for (name, text) in DEFAULT_AGENTS {
        std::fs::write(config.join(format!("agents/{name}.md")), text).unwrap();
        record.insert(format!("agents/{name}.md"), fingerprint(text));
    }
    for (name, print, text) in V040 {
        assert_eq!(fingerprint(text), print, "{name} as 0.4.0 shipped it");
        std::fs::write(config.join(format!("pipelines/{name}.toml")), text).unwrap();
        record.insert(format!("pipelines/{name}.toml"), print.to_string());
    }
    std::fs::write(
        config.join(".shipped.json"),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();
    std::fs::write(
        config.join("pipelines/test-first.toml"),
        TEST_FIRST_EDITED_040,
    )
    .unwrap();
    std::fs::write(config.join("pipelines/secure.toml"), SECURE_040).unwrap();
    std::fs::write(config.join("pipelines/ghost-review.toml"), GHOST_040).unwrap();
}

#[test]
fn a_0_4_0_data_folder_loads_without_change() {
    let root = scratch("018-existing");
    let config = root.join("config");
    config_040(&config);
    let repo = repo_named(&root, "shop");
    std::fs::create_dir_all(repo.join(".fallow/pipelines")).unwrap();
    std::fs::write(repo.join(".fallow/pipelines/with-docs.toml"), WITH_DOCS_040).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "the project's pipeline"]);
    let dirs = [
        config.join("agents"),
        config.join("pipelines"),
        repo.join(".fallow/pipelines"),
    ];
    let before = files(&dirs);

    // The update: Fallow starts (installing the defaults, as `serve` does).
    let news = fallow::defs::install_defaults(&config).unwrap();
    assert!(
        news.is_empty(),
        "no .new for an unchanged default: {news:?}"
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    let v = sreq(
        tx,
        Req::Pipelines {
            project: Some(project.clone()),
        },
    )
    .unwrap();

    // AS1: nothing rewritten, and `parallel.toml` added beside them.
    let mut after = files(&dirs);
    let parallel = config.join("pipelines/parallel.toml");
    assert_eq!(
        after.remove(&parallel).as_deref(),
        DEFAULT_PIPELINES
            .iter()
            .find(|(n, _)| *n == "parallel")
            .map(|(_, t)| t.as_bytes()),
        "the new default is added"
    );
    assert_eq!(after, before, "no file of the engineer's is rewritten");
    let record: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(config.join(".shipped.json")).unwrap())
            .unwrap();
    for (name, print, _) in V040 {
        assert_eq!(record[&format!("pipelines/{name}.toml")], print, "{name}");
    }

    // AS2: each loads without a problem, the same steps, one builder, the same result rule.
    for (name, verifying, source) in [
        ("standard", true, "default"),
        ("test-first", true, "user"),
        ("solo", false, "default"),
        ("secure", true, "user"),
        ("with-docs", true, "project"),
    ] {
        let p = listed(&v, name);
        assert_eq!(p["steps"], steps_040(name), "{name}: the same steps");
        assert_eq!(p["caps"]["builders"], 1, "{name}: one builder");
        assert_eq!(p["verifying"], verifying, "{name}");
        let missing = if verifying {
            json!([])
        } else {
            json!(["no review at checkpoints"])
        };
        assert_eq!(p["missing"], missing, "{name}");
        // An unedited copy is the shipped default, as in 0.4.0.
        assert_eq!(p["source"], source, "{name}: {p}");
    }
    assert_eq!(listed(&v, "test-first")["text"], TEST_FIRST_EDITED_040);
    assert_eq!(listed(&v, "test-first")["caps"]["task_repairs"], 5);
    assert_eq!(listed(&v, "test-first")["caps"]["review_rounds"], 2);
    assert_eq!(listed(&v, "parallel")["caps"]["builders"], 3);

    // AS3: the file 0.4.0 refused is still reported, with its problem; no other problem.
    let problems: Vec<&str> = v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("ghost-review.toml") && problems[0].contains("\"ghost\""),
        "{problems:?}"
    );
    assert!(
        v["pipelines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["name"] != "ghost-review"),
        "{v}"
    );

    // Started again: still nothing rewritten.
    stop(e);
    assert!(fallow::defs::install_defaults(&config).unwrap().is_empty());
    let e = start_studio(&root, &scenario, None);
    let mut again = files(&dirs);
    again.remove(&parallel);
    assert_eq!(again, before, "a second start rewrites nothing either");
    stop(e);
    let _ = std::fs::remove_dir_all(&root);
}

fn steps_of(v: &Value, kind: &str, part: &str) -> Vec<Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .cloned()
        .collect()
}

/// Waits until the running step of `kind` for `part` has said something.
fn working(tx: &Sender<Msg>, id: u64, kind: &str, part: &str) {
    let v = wait_for(tx, id, kind, |v| {
        steps_of(v, kind, part)
            .iter()
            .any(|s| !s["started"].is_null() && s["end"].is_null())
    });
    let n = steps_of(&v, kind, part)
        .iter()
        .find(|s| s["end"].is_null())
        .and_then(|s| s["n"].as_u64())
        .unwrap() as u32;
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if ask(tx, Request::Activity { id, n })
            .ok()
            .and_then(|a| a.as_array().map(|a| !a.is_empty()))
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("step {n} never got to work");
}

fn restart(e: fallow::service::ToolEngine, root: &Path) -> fallow::service::ToolEngine {
    stop(e);
    start_studio(root, &root.join("scenario.toml"), None)
}

/// The pipeline a loop recorded at its lock.
fn recorded(root: &Path, id: u64) -> String {
    let dir = fallow::run::engine::runs_dir(&root.join("data")).join(id.to_string());
    let (_, events, _) = Log::<Event>::open(&dir).unwrap();
    events
        .iter()
        .find_map(|l| match &l.event {
            Event::LoopCreated { pipeline, .. } => Some(pipeline.clone()),
            _ => None,
        })
        .unwrap()
}

/// What the designer writes for each of `pipelines` (the service's listing), through
/// `pipelines.js`'s own `toToml`; `None` without node.
fn designer_toml(pipelines: &[Value], agents: &Value) -> Option<BTreeMap<String, String>> {
    let harness = r#"
const fs = require("fs");
global.document = { addEventListener() {} };
global.window = { addEventListener() {} };
const src = fs.readFileSync(process.argv[1], "utf8").replace(
  'Fallow.register("pipelines", {',
  'Fallow.__t = { toToml, draftOf, set: (x) => { s = x; } };\n  Fallow.register("pipelines", {');
const Fallow = { register() {} };
new Function("Fallow", src)(Fallow);
const input = JSON.parse(process.argv[2]);
Fallow.__t.set({ agents: input.agents });
const out = {};
for (const p of input.pipelines) out[p.name] = Fallow.__t.toToml(Fallow.__t.draftOf(p));
process.stdout.write(JSON.stringify(out));
"#;
    let js = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/service/web/screens/pipelines.js"
    );
    let input = json!({ "pipelines": pipelines, "agents": agents }).to_string();
    let Ok(out) = Command::new("node")
        .args(["-e", harness, js, &input])
        .output()
    else {
        eprintln!("node is not installed: the designer's TOML is not checked");
        return None;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(serde_json::from_slice(&out.stdout).unwrap())
}

#[test]
fn a_loop_locked_before_resumes_and_a_finished_one_shows_as_before() {
    // AS4: a loop on 0.4.0's Standard, stopped in the middle of phase 2 (T002 built, T003
    // building), resumes as one builder without repeating a step and ends verified.
    let root = scratch("018-existing-loop");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "sleep_ms = 30000",
        ),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "when = \"You were interrupted\"",
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
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    assert_eq!(
        recorded(&root, id),
        PIPELINE_040_STANDARD,
        "the loop records 0.4.0's Standard, byte for byte"
    );
    working(&e.tx, id, "build", "T003");
    let e = restart(e, &root);
    let v = wait_for(&e.tx, id, "ready to merge", |v| v["stage"] == "ready");
    for t in ["T001", "T002", "T004"] {
        assert_eq!(steps_of(&v, "build", t).len(), 1, "{t} is built once");
    }
    let t3 = steps_of(&v, "build", "T003");
    assert_eq!(t3.len(), 2, "T003: the interrupted step and its resumption");
    assert_eq!(t3[1]["resume"], t3[0]["session"], "in its session");
    for us in ["US1", "US2"] {
        assert_eq!(steps_of(&v, "review", us).len(), 1, "{us} reviewed once");
    }
    assert_eq!(v["feature_loop"]["outcome"], json!([true, []]), "verified");
    let l = ask(&e.tx, Request::Loop { id }).unwrap();
    assert_eq!(l["pipeline"]["name"], "standard");
    assert_eq!(l["pipeline"]["caps"]["builders"], 1, "one builder");
    assert_eq!(l["groups"], json!([]), "no group of [P] tasks");
    assert_eq!(recorded(&root, id), PIPELINE_040_STANDARD, "still recorded");
    stop(e);
    let _ = std::fs::remove_dir_all(&root);

    // AS5: a finished loop on Solo shows its outcome and what was not checked as before, also
    // once Fallow starts again.
    let root = scratch("018-existing-solo");
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
    assert_eq!(recorded(&root, id), PIPELINE_040_SOLO);
    wait_for(&e.tx, id, "done, not verified", |v| v["stage"] == "ready");
    let shown = |tx: &Sender<Msg>| {
        let l = ask(tx, Request::Loop { id }).unwrap();
        (
            l["stage"].clone(),
            l["outcome"].clone(),
            l["pipeline"]["missing"].clone(),
        )
    };
    let first = shown(&e.tx);
    assert_eq!(
        first,
        (
            json!("done_unverified"),
            json!([false, ["no review at checkpoints"]]),
            json!(["no review at checkpoints"]),
        )
    );
    let e = restart(e, &root);
    assert_eq!(shown(&e.tx), first, "as before, once started again");
    // The loop page shows its pipeline without a verifying label (018 FR-001).
    let loop_js = include_str!("../src/service/web/screens/loop.js");
    assert!(!loop_js.contains("pipeline.verifying"));

    // AS6: saved from the designer with one builder and no new points, a pipeline holds no
    // field 0.4.0 does not know; one with three builders names them.
    let v = sreq(&e.tx, Req::Pipelines { project: None }).unwrap();
    let agents = sreq(&e.tx, Req::Agents { project: None }).unwrap()["agents"].clone();
    let mut chosen: Vec<Value> = ["standard", "test-first", "solo"]
        .iter()
        .map(|n| listed(&v, n).clone())
        .collect();
    let mut three = listed(&v, "standard").clone();
    three["name"] = json!("three");
    three["caps"]["builders"] = json!(3);
    chosen.push(three);
    if let Some(out) = designer_toml(&chosen, &agents) {
        for (name, text) in [
            ("standard", PIPELINE_040_STANDARD),
            ("test-first", PIPELINE_040_TEST_FIRST),
            ("solo", PIPELINE_040_SOLO),
        ] {
            let saved = &out[name];
            assert!(!saved.contains("builders"), "{name}: {saved}");
            let raw: toml::Value = toml::from_str(saved).unwrap();
            for st in raw["step"].as_array().unwrap() {
                assert!(
                    !(st["when"].as_str() == Some("task") && st.get("at").is_some()),
                    "{name}: no `at` on a task step: {saved}"
                );
            }
            // As 0.4.0 reads it: the same pipeline (its text, comments aside).
            assert_eq!(
                toml::from_str::<toml::Value>(saved).unwrap(),
                toml::from_str::<toml::Value>(text).unwrap(),
                "{name}"
            );
            sreq(
                &e.tx,
                Req::PutPipeline {
                    name: name.into(),
                    scope: "user".into(),
                    toml: saved.clone(),
                },
            )
            .unwrap();
        }
        assert!(
            out["three"].contains("builders = 3"),
            "a pipeline using 018's fields names them: {}",
            out["three"]
        );
    }
    stop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// The engineer's own pipeline named `parallel`, written under 0.4.0 (before Fallow shipped one).
const OWN_PARALLEL_040: &str = r#"name = "parallel"
description = "Ours: the reviewer and the security reviewer side by side at each checkpoint"

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
parallel_with = ["reviewer"]
"#;

#[test]
fn an_own_parallel_and_an_edited_agent_are_kept() {
    // T051 (US5 review 1): a 0.4.0 data folder with the engineer's own `parallel.toml` (no
    // record of it: Fallow 0.4.0 shipped none) and an edited copy of the shipped reviewer.
    let root = scratch("018-own-par");
    let config = root.join("config");
    config_040(&config);
    let own = config.join("pipelines/parallel.toml");
    std::fs::write(&own, OWN_PARALLEL_040).unwrap();
    let reviewer = config.join("agents/reviewer.md");
    let shipped_reviewer = std::fs::read_to_string(&reviewer).unwrap();
    let edited = shipped_reviewer.replacen("description: ", "description: Ours. ", 1);
    assert_ne!(edited, shipped_reviewer);
    std::fs::write(&reviewer, &edited).unwrap();
    let dirs = [config.join("agents"), config.join("pipelines")];
    let before = files(&dirs);

    // The update, as `fallow serve` starts: what it says.
    let said = fallow::defs::install_defaults_said(&config).unwrap();
    assert_eq!(
        said,
        [
            "Fallow now ships a pipeline named parallel; yours (pipelines/parallel.toml) keeps the name and is the one used, and the shipped one is beside it as parallel.toml.new"
        ],
        "not \"an update changed the default\": the engineer's file is not an edited default"
    );
    let mut after = files(&dirs);
    let new = config.join("pipelines/parallel.toml.new");
    assert_eq!(
        after.remove(&new).as_deref(),
        DEFAULT_PIPELINES
            .iter()
            .find(|(n, _)| *n == "parallel")
            .map(|(_, t)| t.as_bytes()),
        "the shipped `parallel` is beside it"
    );
    assert_eq!(
        after, before,
        "the own parallel.toml and the edited reviewer are kept byte for byte"
    );
    assert!(!config.join("agents/reviewer.md.new").exists());

    // Theirs is the `parallel` used: its text, one builder, its steps.
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let v = sreq(&e.tx, Req::Pipelines { project: None }).unwrap();
    let p = listed(&v, "parallel");
    assert_eq!(p["text"], OWN_PARALLEL_040, "{p}");
    assert_eq!(p["source"], "user", "{p}");
    assert_eq!(p["caps"]["builders"], 1);
    assert_eq!(p["steps"].as_array().unwrap().len(), 4);
    assert_eq!(
        v["pipelines"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["name"] == "parallel")
            .count(),
        1
    );
    let a = sreq(&e.tx, Req::Agents { project: None }).unwrap();
    let r = a["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "reviewer")
        .unwrap()
        .clone();
    assert!(
        r["description"].as_str().unwrap().starts_with("Ours. "),
        "the edited reviewer is the one used: {r}"
    );

    // Started again: nothing more is said or written.
    stop(e);
    assert!(
        fallow::defs::install_defaults_said(&config)
            .unwrap()
            .is_empty()
    );
    let mut again = files(&dirs);
    again.remove(&new);
    assert_eq!(again, before);
    let _ = std::fs::remove_dir_all(&root);
}

/// The fixtures' place for the folder a 0.4.0 loop ran in (the reviewer's scratch root); the
/// logs hold it only in the repository and worktree paths, which the fold keeps as text.
const ROOT_MARK: &str = "@ROOT@";

#[test]
fn finished_0_4_0_logs_fold_to_the_same_outcome() {
    // US5 review 1: event logs written by Fallow 0.4.0's own engine (v0.4.0, the fake runtime:
    // the calc feature finished on Solo and on Standard), folded by 018. Expected: what 0.4.0's
    // state view showed for them (`<name>.040.json`, taken from it when the logs were written).
    for name in ["solo-finished", "standard-finished"] {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v040");
        let root = scratch(&format!("018-fold-{name}"));
        let dir = root.join("runs/1");
        std::fs::create_dir_all(&dir).unwrap();
        // The root goes inside JSON strings, so it is put in JSON-escaped (a Windows path's `\`).
        let quoted = serde_json::to_string(&root.display().to_string()).unwrap();
        let root_json = &quoted[1..quoted.len() - 1];
        let log = std::fs::read_to_string(fixtures.join(format!("{name}.events.jsonl")))
            .unwrap()
            .replace(ROOT_MARK, root_json);
        std::fs::write(dir.join("events.jsonl"), log).unwrap();
        let (_, events, notes) = Log::<Event>::open(&dir).unwrap();
        assert!(notes.is_empty(), "{name}: {notes:?}");
        let s = fallow::run::state::RunState::fold(1, &events).unwrap();
        let s = serde_json::to_value(&s).unwrap();
        let l = &s["feature_loop"];
        let got = json!({
            "stage": s["stage"],
            "integrated": s["integrated"],
            "outcome": l["outcome"],
            "phases": l["phases"].as_array().unwrap().iter().map(|p| p["state"].clone()).collect::<Vec<_>>(),
            "tasks": l["tasks"].as_object().unwrap().iter().map(|(k, t)| (k.clone(), json!({
                "state": t["state"], "commit": t["commit"], "repairs": t["repairs"],
            }))).collect::<serde_json::Map<_, _>>(),
            "verdicts": s["verdicts"].as_array().unwrap().iter().map(|v| json!({
                "approved": v["approved"], "complete": v["complete"], "missing": v["missing"],
            })).collect::<Vec<_>>(),
        });
        let want: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join(format!("{name}.040.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(got, want, "{name}");
        let _ = std::fs::remove_dir_all(&root);
    }
    // The outcome and `missing` as 0.4.0 ended them, said outright.
    let outcome = |name: &str| -> Value {
        serde_json::from_str::<Value>(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/fixtures/v040/{name}.040.json")),
            )
            .unwrap(),
        )
        .unwrap()["outcome"]
            .clone()
    };
    assert_eq!(
        outcome("solo-finished"),
        json!([false, ["no review at checkpoints"]])
    );
    assert_eq!(outcome("standard-finished"), json!([true, []]));
}
