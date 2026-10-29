//! `fallow-fake-agent`: a scripted stand-in for `claude -p` (006 research R-15, tasks T007).
//!
//! It takes the same flags, reads its instruction on stdin, and writes the same stream-json,
//! ending with a `result` event carrying `structured_output`, `total_cost_usd`, `session_id`
//! and `permission_denials`. What it does is scripted in a TOML file passed with `--scenario`
//! (an agent profile's leading argument): each `[[step]]` answers one instruction whose header
//! line `Fallow step: <kind> (part <p>)` matches, once, in order.
//!
//! ```toml
//! [[step]]
//! kind = "build"                 # plan_turn, build, repair, review, ...
//! part = "main"                  # optional
//! when = "text in the instruction"  # optional
//! files = [{ path = "shop/x.py", text = "..." }]
//! commit = "message"             # git add -A; git commit -m
//! output = { status = "done", commit = "$HEAD", summary = "..." }
//! cost = 0.05
//! sleep_ms = 0
//! exit = 3                       # crash with this exit code instead of a result
//! denied = ["git push"]          # reported as permission denials
//! ```
//!
//! Test-only tooling; never started by Fallow unless an agent profile names it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct File {
    path: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct Step {
    kind: String,
    part: Option<String>,
    when: Option<String>,
    #[serde(default)]
    files: Vec<File>,
    commit: Option<String>,
    output: Option<toml::Value>,
    #[serde(default = "cost")]
    cost: f64,
    #[serde(default)]
    sleep_ms: u64,
    exit: Option<i32>,
    #[serde(default)]
    denied: Vec<String>,
    #[serde(default)]
    is_error: bool,
}

fn cost() -> f64 {
    0.01
}

#[derive(Debug, Deserialize)]
struct Scenario {
    #[serde(default)]
    step: Vec<Step>,
}

fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn git(dir: &Path, args: &[&str]) -> String {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn substitute(v: Value, head: &str, session: &str) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace("$HEAD", head).replace("$SESSION", session)),
        Value::Array(a) => Value::Array(
            a.into_iter()
                .map(|x| substitute(x, head, session))
                .collect(),
        ),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| (k, substitute(x, head, session)))
                .collect(),
        ),
        other => other,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario: Option<PathBuf> = None;
    let mut session = String::from("fake-session");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" => {
                scenario = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--session-id" | "--resume" => {
                if let Some(s) = args.get(i + 1) {
                    session = s.clone();
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let Some(scenario) = scenario else {
        eprintln!("fallow-fake-agent: --scenario is required");
        return ExitCode::from(2);
    };
    let mut instruction = String::new();
    let _ = std::io::stdin().read_to_string(&mut instruction);
    let header = instruction.lines().next().unwrap_or_default();
    let rest = header.strip_prefix("Fallow step: ").unwrap_or_default();
    let kind = rest
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let part = rest
        .split_once("(part ")
        .and_then(|(_, p)| p.strip_suffix(')'))
        .map(str::to_string);
    let text = match std::fs::read_to_string(&scenario) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("fallow-fake-agent: {}: {e}", scenario.display());
            return ExitCode::from(2);
        }
    };
    let sc: Scenario = match toml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fallow-fake-agent: {}: {e}", scenario.display());
            return ExitCode::from(2);
        }
    };
    let used_path = scenario.with_extension("used");
    let mut used: Vec<usize> = std::fs::read_to_string(&used_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    emit(
        json!({"type": "system", "subtype": "init", "session_id": session, "model": "fake", "cwd": cwd}),
    );
    let pick = sc.step.iter().enumerate().find(|(n, s)| {
        !used.contains(n)
            && s.kind == kind
            && (s.part.is_none() || s.part == part)
            && s.when
                .as_ref()
                .is_none_or(|w| instruction.contains(w.as_str()))
    });
    let Some((n, step)) = pick else {
        emit(
            json!({"type": "result", "subtype": "error", "is_error": true, "session_id": session,
            "total_cost_usd": 0.0, "result": format!("no scripted step for {kind} {part:?}"), "permission_denials": []}),
        );
        return ExitCode::SUCCESS;
    };
    used.push(n);
    let _ = std::fs::write(
        &used_path,
        used.iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    emit(
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": format!("Working on {kind}.")}]}}),
    );
    if step.sleep_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(step.sleep_ms));
    }
    for f in &step.files {
        let p = cwd.join(&f.path);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&p, &f.text);
        emit(
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Write", "input": {"file_path": f.path}}]}}),
        );
        emit(
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "written"}]}}),
        );
    }
    if let Some(msg) = &step.commit {
        git(&cwd, &["add", "-A"]);
        let out = git(&cwd, &["commit", "-q", "-m", msg]);
        emit(
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": format!("git commit -m {msg:?}")}}]}}),
        );
        emit(
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": out}]}}),
        );
    }
    if let Some(code) = step.exit {
        return ExitCode::from(code as u8);
    }
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    let output = step
        .output
        .as_ref()
        .and_then(|o| serde_json::to_value(o).ok())
        .map(|v| substitute(v, &head, &session));
    let denials: Vec<Value> = step
        .denied
        .iter()
        .map(|c| json!({"tool_name": "Bash", "tool_input": {"command": c}}))
        .collect();
    let mut result = json!({"type": "result", "subtype": "success", "is_error": step.is_error,
        "session_id": session, "total_cost_usd": step.cost, "num_turns": 1,
        "result": "done", "permission_denials": denials});
    if let Some(o) = output {
        result["structured_output"] = o;
    }
    emit(result);
    ExitCode::SUCCESS
}
