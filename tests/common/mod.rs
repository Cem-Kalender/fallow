//! Helpers for loops with the fake agent (006 research R-15): a scratch repository, settings that
//! point the `claude` runtime at `fallow-fake-agent` with a scenario, and a studio engine.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use fallow::config::Settings;
use fallow::run::engine::{Msg, Reply, Request};
use serde_json::Value;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A fresh folder under the system temp folder.
pub fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A repository on `main` with a Python package and the squad file's gate.
pub fn repo(root: &Path) -> PathBuf {
    repo_named(root, "repo")
}

/// PEP 394 lets `python` be missing outside Windows.
pub const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

/// `repo` in the folder `name` (a project is named after its folder).
pub fn repo_named(root: &Path, name: &str) -> PathBuf {
    let r = root.join(name);
    std::fs::create_dir_all(r.join(".fallow")).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    git(&r, &["config", "user.email", "t@example.invalid"]);
    git(&r, &["config", "user.name", "t"]);
    git(&r, &["config", "commit.gpgsign", "false"]);
    git(&r, &["config", "core.autocrlf", "false"]);
    std::fs::write(
        r.join(".fallow/squad.toml"),
        format!(
            "repair_cap = 2\n[[gate]]\nname = \"tests\"\ncommand = \"{PYTHON} -m unittest\"\nbaseline = true\n"
        ),
    )
    .unwrap();
    std::fs::write(r.join("README.md"), "calc\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "seed"]);
    r
}

pub fn ask(tx: &Sender<Msg>, r: Request) -> Result<Value, String> {
    let (rtx, rrx) = std::sync::mpsc::channel();
    tx.send(Msg::Request(r, rtx)).unwrap();
    match rrx.recv_timeout(Duration::from_secs(30)).unwrap() {
        Reply::Ok(v) => Ok(v),
        Reply::Err(e) => Err(e),
    }
}

/// Waits until `pred` holds for the run's view, or fails with the last view.
pub fn wait_for(tx: &Sender<Msg>, id: u64, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    wait_up_to(tx, id, what, Duration::from_secs(180), pred)
}

/// As `wait_for`, with a longer wait for real agents.
pub fn wait_up_to(
    tx: &Sender<Msg>,
    id: u64,
    what: &str,
    limit: Duration,
    pred: impl Fn(&Value) -> bool,
) -> Value {
    let end = Instant::now() + limit;
    let mut last = Value::Null;
    while Instant::now() < end {
        if let Ok(v) = ask(tx, Request::Get { id }) {
            if pred(&v) {
                return v;
            }
            last = v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timed out waiting for {what}; stage {}, needs {}, notes {}, steps {}",
        last["stage"], last["needs_engineer"], last["notes"], last["steps"]
    );
}

/// TOML string literal (multi-line, literal).
pub fn lit(s: &str) -> String {
    format!("'''\n{s}'''")
}

// ---- 007: the studio --------------------------------------------------------------------

use fallow::studio::Req;

/// Settings where the `claude` runtime (the lead's) is the fake agent playing `scenario`.
pub fn studio_settings(scenario: &Path) -> Settings {
    let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
    let text = format!(
        "[agents.claude]\ncommand = {exe:?}\nargs = [\"--scenario\", {:?}]\n",
        scenario.display().to_string()
    );
    let mut n = Vec::new();
    let s = Settings::parse("settings.toml", &text, &mut n);
    assert!(n.is_empty(), "{n:?}");
    s
}

/// An engine that serves MCP bridges, with Fallow's own binary for `mcp-bridge`.
pub fn start_studio(
    root: &Path,
    scenario: &Path,
    approval_wait: Option<Duration>,
) -> fallow::service::ToolEngine {
    start_studio_notify(
        root,
        scenario,
        approval_wait,
        std::sync::Arc::new(fallow::platform::notify::Off),
    )
}

/// `start_studio` whose desktop notifications go to `notify`.
pub fn start_studio_notify(
    root: &Path,
    scenario: &Path,
    approval_wait: Option<Duration>,
    notify: std::sync::Arc<dyn fallow::platform::notify::Notify>,
) -> fallow::service::ToolEngine {
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    // Settings a test adds, such as a smaller cap.
    let settings = match std::fs::read_to_string(root.join("settings.extra.toml")) {
        Ok(extra) => {
            let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
            let text = format!(
                "{extra}
[agents.claude]
command = {exe:?}
args = [\"--scenario\", {:?}]
",
                scenario.display().to_string()
            );
            let mut n = Vec::new();
            let s = Settings::parse("settings.toml", &text, &mut n);
            assert!(n.is_empty(), "{n:?}");
            s
        }
        Err(_) => studio_settings(scenario),
    };
    fallow::service::engine_with_notify(
        root.join("data"),
        settings,
        Some(config),
        PathBuf::from(env!("CARGO_BIN_EXE_fallow")),
        approval_wait,
        notify,
    )
    .unwrap()
}

pub fn sreq(tx: &Sender<Msg>, r: Req) -> Result<Value, String> {
    ask(tx, Request::Studio(r))
}

/// Waits until `pred` holds for the conversation's view.
pub fn wait_conv(tx: &Sender<Msg>, conv: &str, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(90);
    let mut last = Value::Null;
    while Instant::now() < end {
        if let Ok(v) = sreq(tx, Req::Conversation { id: conv.into() }) {
            if pred(&v) {
                return v;
            }
            last = v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}: {last}");
}

/// Waits until the conversation's latest turn has ended, and returns the view.
pub fn turn_ended(tx: &Sender<Msg>, conv: &str, n: usize) -> Value {
    wait_conv(tx, conv, "the turn's end", |v| {
        v["busy"] == false
            && v["turns"].as_array().is_some_and(|t| t.len() >= n)
            && !v["turns"][n - 1]["reply"].is_null()
    })
}

// ---- 007 US2: a locked two-story feature ------------------------------------------------

pub const CALC_SPEC: &str = "# Feature Specification: Calc\n\n## User Scenarios & Testing\n\n### User Story 1 - Add (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** two numbers, **When** adding 2 and 3, **Then** the result is 5.\n2. **Given** a string, **When** adding 'a' and 1, **Then** a TypeError is raised (must fail)\n\n### User Story 2 - Subtract (Priority: P2)\n\n**Acceptance Scenarios**:\n\n1. **Given** two numbers, **When** subtracting 3 from 5, **Then** the result is 2.\n2. **Given** a string, **When** subtracting 1 from 'a', **Then** a TypeError is raised (must fail)\n";
pub const CALC_PLAN: &str = "# Plan\n\n## Commands\n\n- `python -m calc`\n";
pub const CALC_TASKS: &str = "# Tasks: Calc\n\n## Phase 1: Setup\n\n- [ ] T001 Create the calc package\n\n## Phase 2: User Story 1 - Add\n\n- [ ] T002 [US1] add() in calc/__init__.py\n- [ ] T003 [US1] Tests for add\n\n## Phase 3: User Story 2 - Subtract\n\n- [ ] T004 [US2] sub() with its tests\n";

/// The lead's two turns: the documents, then analyze.
pub fn lead_writes_calc() -> String {
    format!(
        "[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/001-calc/spec.md\", text = {} }}, {{ path = \"specs/001-calc/plan.md\", text = {} }}, {{ path = \"specs/001-calc/tasks.md\", text = {} }}]\noutput = {{ reply = \"Written.\", step = \"tasks\" }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed.\", step = \"analyze\", findings = [] }}\n\n",
        lit(CALC_SPEC),
        lit(CALC_PLAN),
        lit(CALC_TASKS)
    )
}

/// A builder step for `task` that writes `files` and reports them.
pub fn task_step(task: &str, files: &[(&str, &str)], extra: &str) -> String {
    let fs: Vec<String> = files
        .iter()
        .map(|(p, t)| format!("{{ path = {p:?}, text = {} }}", lit(t)))
        .collect();
    let names: Vec<String> = files.iter().map(|(p, _)| format!("{p:?}")).collect();
    format!(
        "[[step]]\nkind = \"build\"\npart = {task:?}\n{extra}\nfiles = [{}]\noutput = {{ status = \"done\", task = {task:?}, files = [{}], summary = \"did {task}\" }}\n\n",
        fs.join(", "),
        names.join(", ")
    )
}

/// The handover of phase `n`.
pub fn handover_step(n: u32, extra: &str) -> String {
    let path = format!("specs/001-calc/handover/phase-{n}.md");
    format!(
        "[[step]]\nkind = \"handover\"\npart = \"phase-{n}\"\n{extra}\nfiles = [{{ path = {path:?}, text = {} }}]\noutput = {{ status = \"done\", path = {path:?}, summary = \"handed over\" }}\n\n",
        lit(&format!(
            "## Built\nPhase {n}.\n\n## Decisions\nNone.\n\n## For the next phase\nGo on.\n"
        ))
    )
}

/// A reviewer's verdict on `story` (US1 or US2): both scenarios checked, the second refused.
pub fn check_step(story: &str, passed: bool, extra: &str) -> String {
    let (op, a, b) = if story == "US1" {
        ("add", "2, 3", "5")
    } else {
        ("sub", "5, 3", "2")
    };
    format!(
        "[[step]]\nkind = \"review\"\npart = {story:?}\n{extra}\noutput = {{ answers = [{{ criterion = \"{story}/1\", ran = \"python -c {op}({a})\", result = \"{b}\", passed = {passed} }}, {{ criterion = \"{story}/2\", ran = \"python -c {op}('a', 1)\", result = \"TypeError\", passed = true, failing_case = \"{op}('a', 1) is refused\" }}], summary = \"checked\" }}\n\n"
    )
}

pub const CALC_INIT: &str = "def add(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a + b\n";
pub const CALC_INIT_SUB: &str = "def add(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a + b\n\n\ndef sub(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a - b\n";
pub const TEST_ADD: &str = "import unittest\nfrom calc import add\n\n\nclass Add(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n\n    def test_bad(self):\n        with self.assertRaises(TypeError):\n            add('a', 1)\n";
pub const TEST_SUB: &str = "import unittest\nfrom calc import sub\n\n\nclass Sub(unittest.TestCase):\n    def test_sub(self):\n        self.assertEqual(sub(5, 3), 2)\n";

/// Every step of a loop that goes well, phase by phase.
pub fn calc_loop_ok() -> String {
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
    .concat()
}

/// A project with the Calc feature locked through the studio; returns the engine, the project
/// folder, the loop's id. `loop_steps` are the loop's scenario steps, after the lead's.
pub fn locked_calc(
    root: &Path,
    loop_steps: &str,
    pipeline: Option<&str>,
) -> (fallow::service::ToolEngine, PathBuf, u64) {
    locked_calc_with(root, loop_steps, pipeline, Some((5.0, 60)))
}

/// `locked_calc` with the budget given at the lock, or the suggested one when `None`.
pub fn locked_calc_with(
    root: &Path,
    loop_steps: &str,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
) -> (fallow::service::ToolEngine, PathBuf, u64) {
    locked_calc_full(root, loop_steps, pipeline, budget, None)
}

/// `locked_calc` with the autonomy level set for the feature at the lock.
#[allow(dead_code)]
pub fn locked_calc_level(
    root: &Path,
    loop_steps: &str,
    level: fallow::run::events::Level,
) -> (fallow::service::ToolEngine, PathBuf, u64) {
    locked_calc_full(root, loop_steps, None, Some((5.0, 60)), Some(level))
}

fn locked_calc_full(
    root: &Path,
    loop_steps: &str,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
    level: Option<fallow::run::events::Level>,
) -> (fallow::service::ToolEngine, PathBuf, u64) {
    let repo = repo(root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{loop_steps}", lead_writes_calc())).unwrap();
    let e = start_studio(root, &scenario, None);
    let id = lock_calc_in(&e.tx, &repo, pipeline, budget, level);
    (e, repo, id)
}

/// Adds `repo` as a project, works out the Calc feature with the lead (two of the scenario's
/// lead turns) and locks it; returns the loop's id.
#[allow(dead_code)]
pub fn lock_calc_in(
    tx: &Sender<Msg>,
    repo: &Path,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
    level: Option<fallow::run::events::Level>,
) -> u64 {
    let repo = repo.to_path_buf();
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    lock_feature_in(tx, &project, "Calc", "001-calc", pipeline, budget, level)
}

/// Works out a feature of a project already added (the scenario's next two lead turns: the
/// documents, then analyze) and locks it; returns the loop's id (011: two features in one
/// project).
#[allow(dead_code)]
pub fn lock_feature_in(
    tx: &Sender<Msg>,
    project: &str,
    title: &str,
    feature: &str,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
    level: Option<fallow::run::events::Level>,
) -> u64 {
    lock_feature_with(
        tx,
        project,
        title,
        feature,
        pipeline,
        budget,
        level,
        LockExtra::default(),
    )
}

/// 011: the lock's other settings (`update_limit`, `run_after`) for `lock_feature_with`.
#[derive(Default, Clone, Copy)]
pub struct LockExtra {
    pub update_limit: Option<u32>,
    pub run_after: Option<u64>,
}

/// `lock_feature_in` with the lock's other settings.
#[allow(dead_code, clippy::too_many_arguments)]
pub fn lock_feature_with(
    tx: &Sender<Msg>,
    project: &str,
    title: &str,
    feature: &str,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
    level: Option<fallow::run::events::Level>,
    extra: LockExtra,
) -> u64 {
    prepare_feature_in(tx, project, title, feature);
    lock_prepared(tx, project, feature, pipeline, budget, level, extra)
}

/// Works out a feature with the lead (the scenario's next two lead turns: the documents, then
/// analyze), without locking it.
#[allow(dead_code)]
pub fn prepare_feature_in(tx: &Sender<Msg>, project: &str, title: &str, feature: &str) {
    let project = project.to_string();
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: title.into(),
            lead: None,
        },
    )
    .unwrap();
    assert_eq!(f["feature"], feature);
    let conv = f["conversation"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Write it.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 1);
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Analyze.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 2);
}

/// Locks a feature already worked out; returns its loop's id.
#[allow(dead_code)]
pub fn lock_prepared(
    tx: &Sender<Msg>,
    project: &str,
    feature: &str,
    pipeline: Option<&str>,
    budget: Option<(f64, u64)>,
    level: Option<fallow::run::events::Level>,
    extra: LockExtra,
) -> u64 {
    let l = ask(
        tx,
        Request::Lock {
            project: project.to_string(),
            feature: feature.into(),
            pipeline: pipeline.map(str::to_string),
            level,
            budget_usd: budget.map(|b| b.0),
            budget_minutes: budget.map(|b| b.1),
            when_verified: None,
            update_limit: extra.update_limit,
            run_after: extra.run_after,
        },
    )
    .unwrap();
    l["loop"].as_u64().unwrap()
}

// ---- 011: a second feature on the same file, and merge-conflict repairs -------------------

pub const CALC2_SPEC: &str = "# Feature Specification: Calc2\n\n## User Scenarios & Testing\n\n### User Story 1 - Multiply (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** two numbers, **When** multiplying 2 and 3, **Then** the result is 6.\n2. **Given** a string, **When** multiplying 'a' and 1, **Then** a TypeError is raised (must fail)\n";
pub const CALC2_PLAN: &str = "# Plan\n\n## Commands\n\n- `python -m calc`\n";
pub const CALC2_TASKS: &str = "# Tasks: Calc2\n\n## Phase 1: User Story 1 - Multiply\n\n- [ ] T001 [US1] mul() in calc/__init__.py\n- [ ] T002 [US1] Tests for mul\n";

/// The lead's two turns for `002-calc2`: the documents, then analyze.
pub fn lead_writes_calc2() -> String {
    format!(
        "[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/002-calc2/spec.md\", text = {} }}, {{ path = \"specs/002-calc2/plan.md\", text = {} }}, {{ path = \"specs/002-calc2/tasks.md\", text = {} }}]\noutput = {{ reply = \"Written.\", step = \"tasks\" }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed.\", step = \"analyze\", findings = [] }}\n\n",
        lit(CALC2_SPEC),
        lit(CALC2_PLAN),
        lit(CALC2_TASKS)
    )
}

/// A commit on `branch` of `repo` (checked out there) from "outside Fallow".
pub fn commit_on(repo: &Path, branch: &str, path: &str, text: &str, msg: &str) -> String {
    let current = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(current, branch, "commit_on expects {branch} checked out");
    let p = repo.join(path);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(&p, text).unwrap();
    git(repo, &["add", path]);
    git(repo, &["commit", "-q", "-m", msg]);
    git(repo, &["rev-parse", "HEAD"])
}

/// The builder's repair of an update's conflicts (part `update-<n>`): writes `file` with `text`,
/// with its own commit or not.
pub fn conflict_repair_step(n: u32, file: &str, text: &str, commit: bool) -> String {
    let c = if commit {
        "commit = \"resolve the conflicts\"\n".to_string()
    } else {
        String::new()
    };
    format!(
        "[[step]]\nkind = \"repair\"\npart = \"update-{n}\"\nfiles = [{{ path = {file:?}, text = {} }}]\n{c}output = {{ status = \"done\", files = [{file:?}], summary = \"resolved\" }}\n\n",
        lit(text)
    )
}

/// The builder's repair of a non-text conflict: deletes `file` or writes it with `text`, and
/// reports how in `resolutions` (011 research R-2).
pub fn nontext_repair_step(n: u32, file: &str, text: Option<&str>, how: &str) -> String {
    let act = match text {
        Some(t) => format!("files = [{{ path = {file:?}, text = {} }}]\n", lit(t)),
        None => format!("delete = [{file:?}]\n"),
    };
    format!(
        "[[step]]\nkind = \"repair\"\npart = \"update-{n}\"\n{act}output = {{ status = \"done\", files = [{file:?}], summary = \"resolved\", resolutions = [{{ path = {file:?}, how = {how:?} }}] }}\n\n"
    )
}

/// A repair that resolves `file`'s markers but leaves git's `<path>~<side>` file behind.
pub fn leave_side_file_step(n: u32, file: &str, text: &str) -> String {
    conflict_repair_step(n, file, text, false)
}

/// Asks to merge once the loop's remote has been read (010: nothing is offered before it is,
/// and a merge asked for meanwhile is refused with "reading the remote first").
pub fn merge_when_read(tx: &Sender<Msg>, id: u64) -> Result<Value, String> {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        if let Ok(v) = ask(tx, Request::Loop { id }) {
            let r = &v["publish"]["remote"];
            if r["state"] != "checking" && r["protected"] != "checking" {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    ask(
        tx,
        Request::Merge {
            id,
            without_latest_check: false,
        },
    )
}

/// A lead turn that runs analyze again, with no finding: for a scenario whose documents change
/// after the first analyze (an unlock), which must be analyzed again before the lock (T086).
#[allow(dead_code)]
pub const LEAD_ANALYZES_AGAIN: &str = "[[step]]\nkind = \"lead_turn\"\nwhen = \"Analyze again\"\noutput = { reply = \"Analyzed again.\", step = \"analyze\", findings = [] }\n\n";

/// Runs analyze again in the feature's spec session (the scenario holds
/// `LEAD_ANALYZES_AGAIN`), and waits for the turn's end.
#[allow(dead_code)]
pub fn analyze_again(tx: &Sender<Msg>, project: &str, feature: &str) {
    let f = sreq(
        tx,
        Req::Feature {
            project: project.into(),
            feature: feature.into(),
        },
    )
    .unwrap();
    let conv = f["conversation"]["id"].as_str().unwrap().to_string();
    let n = f["conversation"]["turns"].as_array().map_or(0, |t| t.len()) + 1;
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Analyze again.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, n);
}

// ---- 009: a second lead, and an editor that records how it was started -------------------

/// Writes the project override `.fallow/agents/<name>.md`: the shipped lead under another name,
/// on `runtime` with `model`, with `marker` in its instructions (scenario steps tell it apart by
/// `system = "<marker>"` or `model = "<model>"`).
pub fn second_lead(repo: &Path, name: &str, runtime: &str, model: &str, marker: &str) {
    let dir = repo.join(".fallow/agents");
    std::fs::create_dir_all(&dir).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "lead")
        .unwrap()
        .1;
    let text = shipped
        .replacen("name: lead", &format!("name: {name}"), 1)
        .replacen("runtime: claude", &format!("runtime: {runtime}"), 1)
        .replacen("model: opus", &format!("model: {model}"), 1)
        // The first "\n---" closes the frontmatter (the opening "---" starts the file).
        .replacen("\n---", &format!("\n---\n\n{marker}\n"), 1);
    std::fs::write(dir.join(format!("{name}.md")), text).unwrap();
}

/// An editor for "Open in my editor": the fake agent's `--record` mode. Returns the program and
/// the fixed arguments to save, and the file where each start's arguments end up.
pub fn fake_editor(root: &Path) -> (String, Vec<String>, PathBuf) {
    let log = root.join("editor.log");
    (
        env!("CARGO_BIN_EXE_fallow-fake-agent").to_string(),
        vec!["--record".into(), log.display().to_string()],
        log,
    )
}

// ---- 010: a remote and a hosting service, without network --------------------------------

/// A bare repository at `<root>/remote.git` standing in for `address` (git's `insteadOf`, 010
/// research R-1): `origin` is `address` as written, every push goes to the bare repository, and
/// `main` is pushed with its upstream set.
pub fn bare_remote(root: &Path, repo: &Path, address: &str) -> PathBuf {
    bare_remote_in(root, repo, address, "remote.git")
}

/// `bare_remote` in the folder `folder` under `root` (several remotes in one test).
pub fn bare_remote_in(root: &Path, repo: &Path, address: &str, folder: &str) -> PathBuf {
    let bare = root.join(folder);
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let local = bare.display().to_string().replace('\\', "/");
    git(repo, &["remote", "add", "origin", address]);
    git(
        repo,
        &["config", &format!("url.{local}.insteadOf"), address],
    );
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    bare
}

/// The commit `branch` is at in the bare remote of `bare_remote`, if it is there.
pub fn remote_head(root: &Path, branch: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(root.join("remote.git"))
        .args([
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `fallow-fake-host` in place of `gh` (010 contracts/host-adapter.md): writes its state file
/// at `<root>/host.json` and adds `[hosting.github]` to the settings `start_studio` reads.
pub fn fake_host(root: &Path, state: Value) -> PathBuf {
    let path = root.join("host.json");
    std::fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let extra = root.join("settings.extra.toml");
    let before = std::fs::read_to_string(&extra).unwrap_or_default();
    if before.contains("[hosting.github]") {
        return path;
    }
    let exe = env!("CARGO_BIN_EXE_fallow-fake-host");
    std::fs::write(
        &extra,
        format!(
            "{before}\n[hosting.github]\ncommand = {exe:?}\nargs = [\"--state\", {:?}]\n",
            path.display().to_string()
        ),
    )
    .unwrap();
    path
}

/// The fake host's state, to read or change between calls.
pub fn host_state(state: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(state).unwrap()).unwrap()
}

pub fn set_host_state(state: &Path, v: &Value) {
    std::fs::write(state, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// Every command line the fake host was given, in order.
pub fn fake_host_log(state: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(format!("{}.log", state.display()))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// `locked_calc` with `prepare(root, repo)` run before Fallow starts (a remote, a fake host),
/// and the level given at the lock.
pub fn locked_calc_prepared(
    root: &Path,
    loop_steps: &str,
    pipeline: Option<&str>,
    level: Option<fallow::run::events::Level>,
    prepare: impl FnOnce(&Path, &Path),
) -> (fallow::service::ToolEngine, PathBuf, u64) {
    let repo = repo(root);
    prepare(root, &repo);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{loop_steps}", lead_writes_calc())).unwrap();
    let e = start_studio(root, &scenario, None);
    let id = lock_calc_in(&e.tx, &repo, pipeline, Some((5.0, 60)), level);
    (e, repo, id)
}

// ---- 012: solo sessions ----

/// The git hooks a solo session's repository carries (012 T003): each writes its name to
/// `<root>/hooks.log`, so a test sees whether any Fallow git command ran one (research R-2, R-3).
pub const MARKER_HOOKS: [&str; 5] = [
    "pre-commit",
    "prepare-commit-msg",
    "post-commit",
    "post-checkout",
    "reference-transaction",
];

/// `repo` with `MARKER_HOOKS` installed in `.git/hooks`.
pub fn repo_with_markers(root: &Path) -> PathBuf {
    let r = repo(root);
    let log = root
        .join("hooks.log")
        .display()
        .to_string()
        .replace('\\', "/");
    for h in MARKER_HOOKS {
        let p = r.join(".git/hooks").join(h);
        std::fs::write(&p, format!("#!/bin/sh\necho {h} >> \"{log}\"\nexit 0\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    // The log starts empty: only what runs after this counts.
    let _ = std::fs::remove_file(root.join("hooks.log"));
    r
}

/// The hooks that ran since `repo_with_markers`, in order.
pub fn hooks_ran(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("hooks.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Settings with the `claude` runtime playing `scenario` (as `studio_settings`), and a `fake`
/// runtime playing it too, whose profile declares `caps` (`--images`, `--web-off`,
/// `--control-file <path>`), for an assistant defined on `runtime: fake` (`assistant_on_fake`).
pub fn studio_settings_caps(scenario: &Path, caps: &[&str]) -> Settings {
    let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
    let sc = scenario.display().to_string();
    let caps: Vec<String> = caps.iter().map(|c| format!("{c:?}")).collect();
    let text = format!(
        "[agents.claude]\ncommand = {exe:?}\nargs = [\"--scenario\", {sc:?}]\n[agents.fake]\ncommand = {exe:?}\nargs = [\"--scenario\", {sc:?}{}{}]\n",
        if caps.is_empty() { "" } else { ", " },
        caps.join(", ")
    );
    let mut n = Vec::new();
    let s = Settings::parse("settings.toml", &text, &mut n);
    assert!(n.is_empty(), "{n:?}");
    s
}

/// The project's `assistant` overridden to run on the `fake` runtime (its capabilities from
/// `studio_settings_caps`), committed so the project folder stays clean.
pub fn assistant_on_fake(repo: &Path) {
    let dir = repo.join(".fallow/agents");
    std::fs::create_dir_all(&dir).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "assistant")
        .unwrap()
        .1;
    std::fs::write(
        dir.join("assistant.md"),
        shipped.replacen("runtime: claude", "runtime: fake", 1),
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "assistant on fake"]);
}

/// One scripted assistant turn (`Fallow step: assistant_turn`), with `extra` TOML lines.
pub fn assistant_step(reply: &str, extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"assistant_turn\"\n{extra}\noutput = {{ reply = {} }}\n\n",
        lit(reply)
    )
}

/// 012: a session from its first message; returns its id (the turn has started).
pub fn start_session(tx: &Sender<Msg>, project: &str, text: &str) -> String {
    let v = sreq(
        tx,
        Req::NewSession {
            project: project.to_string(),
            agent: None,
            text: text.to_string(),
        },
    )
    .unwrap();
    v["session"].as_str().unwrap().to_string()
}

/// 012: a message in a session.
pub fn say_solo(tx: &Sender<Msg>, session: &str, text: &str) -> Value {
    sreq(
        tx,
        Req::Say {
            conv: session.to_string(),
            text: text.to_string(),
        },
    )
    .unwrap()
}

/// 012: the session's view once turn `n` has ended.
pub fn wait_turn(tx: &Sender<Msg>, session: &str, n: usize) -> Value {
    turn_ended(tx, session, n)
}

/// Stops an engine and waits for its thread, as a restart does: dropping it alone leaves its
/// thread ticking beside the next one on the same data.
#[allow(dead_code)]
pub fn stop(e: fallow::service::ToolEngine) {
    let _ = e.tx.send(Msg::Shutdown);
    let _ = e.handle.join();
}

/// 012: a studio on `settings` (as `studio_settings_caps` makes them).
#[allow(dead_code)]
pub fn start_studio_settings(root: &Path, settings: Settings) -> fallow::service::ToolEngine {
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    fallow::service::engine_with_notify(
        root.join("data"),
        settings,
        Some(config),
        PathBuf::from(env!("CARGO_BIN_EXE_fallow")),
        None,
        std::sync::Arc::new(fallow::platform::notify::Off),
    )
    .unwrap()
}
