//! Helpers for runs with the fake agent (006 research R-15): a scratch repository, settings that
//! point every role at `fallow-fake-agent` with a scenario, and an engine to drive.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use fallow::config::Settings;
use fallow::run::engine::{Create, Msg, Reply, Request};
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
    let r = root.join("repo");
    std::fs::create_dir_all(r.join(".fallow")).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    git(&r, &["config", "user.email", "t@example.invalid"]);
    git(&r, &["config", "user.name", "t"]);
    git(&r, &["config", "commit.gpgsign", "false"]);
    git(&r, &["config", "core.autocrlf", "false"]);
    std::fs::write(
        r.join(".fallow/squad.toml"),
        "repair_cap = 2\n[[gate]]\nname = \"tests\"\ncommand = \"python -m unittest\"\nbaseline = true\n",
    )
    .unwrap();
    std::fs::write(r.join("README.md"), "calc\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "seed"]);
    r
}

/// Settings where every role is the fake agent playing `scenario`.
pub fn settings(scenario: &Path) -> Settings {
    let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
    let text = format!(
        "[defaults]\nautonomy = \"high\"\n[roles.planner]\nagent = \"fake\"\n[roles.builder]\nagent = \"fake\"\n[roles.reviewer]\nagent = \"fake\"\n[agents.fake]\ncommand = {exe:?}\nargs = [\"--scenario\", {:?}]\n",
        scenario.display().to_string()
    );
    let mut n = Vec::new();
    let s = Settings::parse("settings.toml", &text, &mut n);
    assert!(n.is_empty(), "{n:?}");
    s
}

pub fn ask(tx: &Sender<Msg>, r: Request) -> Result<Value, String> {
    let (rtx, rrx) = std::sync::mpsc::channel();
    tx.send(Msg::Request(r, rtx)).unwrap();
    match rrx.recv_timeout(Duration::from_secs(30)).unwrap() {
        Reply::Ok(v) => Ok(v),
        Reply::Err(e) => Err(e),
    }
}

pub fn create(tx: &Sender<Msg>, repo: &Path, task: &str) -> u64 {
    let v = ask(
        tx,
        Request::Create(Create {
            repo: repo.to_path_buf(),
            task: task.into(),
            ..Create::default()
        }),
    )
    .unwrap();
    v["id"].as_u64().unwrap()
}

/// Waits until `pred` holds for the run's view, or fails with the last view.
pub fn wait_for(tx: &Sender<Msg>, id: u64, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    wait_up_to(tx, id, what, Duration::from_secs(90), pred)
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

pub const PLAN: &str = r#"criteria = [
  { id = "add", text = "add(2, 3) is 5" },
  { id = "bad", text = "add('a', 1) raises TypeError", must_fail = true },
]
"#;

pub const CALC: &str = "def add(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a + b\n";

pub const CALC_TEST: &str = "import unittest\nfrom calc import add\n\n\nclass Add(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n\n    def test_bad(self):\n        with self.assertRaises(TypeError):\n            add('a', 1)\n";

/// TOML string literal (multi-line, literal).
pub fn lit(s: &str) -> String {
    format!("'''\n{s}'''")
}

/// A plan step that commits `plan` and is ready.
pub fn plan_step(plan: &str) -> String {
    format!(
        "[[step]]\nkind = \"plan_turn\"\nfiles = [{{ path = \".fallow/work.toml\", text = {} }}]\ncommit = \"Plan\"\noutput = {{ reply = \"The plan.\", commit = \"$HEAD\", ready = true }}\n",
        lit(plan)
    )
}

/// A build or repair step that writes `files` (path, text), commits and reports done.
pub fn build_step(kind: &str, files: &[(&str, &str)], extra: &str) -> String {
    let fs: Vec<String> = files
        .iter()
        .map(|(p, t)| format!("{{ path = {p:?}, text = {} }}", lit(t)))
        .collect();
    format!(
        "[[step]]\nkind = {kind:?}\n{extra}\nfiles = [{}]\ncommit = \"Work\"\noutput = {{ status = \"done\", commit = \"$HEAD\", summary = \"done\" }}\n",
        fs.join(", ")
    )
}

/// A complete, approving review of `PLAN`.
pub fn review_ok(extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"review\"\n{extra}\noutput = {{ answers = [{{ criterion = \"main/add\", ran = \"python -c add(2,3)\", result = \"5\", passed = true }}, {{ criterion = \"main/bad\", ran = \"python -c add('a',1)\", result = \"TypeError\", passed = true, failing_case = \"add('a', 1) raises\" }}] }}\n"
    )
}

pub fn start_engine(root: &Path, scenario: &Path) -> (Sender<Msg>, std::thread::JoinHandle<()>) {
    fallow::service::engine_for_tests(root.join("data"), settings(scenario)).unwrap()
}

pub fn good_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("calc.py", CALC),
        ("tests/__init__.py", ""),
        ("tests/test_calc.py", CALC_TEST),
    ]
}
