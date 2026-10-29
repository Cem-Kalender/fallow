//! Claude Code without a terminal: `claude -p` (006 research R-3, verified 2026-10-29).

use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde_json::Value;

use super::{Activity, Call, Outcome, Session};

/// The arguments after the agent's own leading arguments (research R-3). The instruction goes
/// on stdin, which also keeps it off the command line.
pub fn args(c: &Call) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if let Some(s) = &c.schema {
        a.push("--json-schema".into());
        a.push(s.to_string());
    }
    if c.unrestricted {
        a.push("--dangerously-skip-permissions".into());
    } else {
        a.push("--permission-mode".into());
        a.push("dontAsk".into());
        if !c.allowed.is_empty() {
            a.push("--allowedTools".into());
            a.extend(c.allowed.iter().cloned());
        }
    }
    if let Some(m) = &c.mcp_config {
        a.push("--mcp-config".into());
        a.push(m.display().to_string());
        a.push("--strict-mcp-config".into());
    }
    match &c.session {
        Session::New(id) => {
            a.push("--session-id".into());
            a.push(id.clone());
        }
        Session::Resume(id) => {
            a.push("--resume".into());
            a.push(id.clone());
        }
    }
    if let Some(m) = c.model.as_ref().filter(|m| !m.is_empty()) {
        a.push("--model".into());
        a.push(m.clone());
    }
    if let Some(b) = c.budget_usd {
        a.push("--max-budget-usd".into());
        a.push(format!("{:.2}", b.max(0.01)));
    }
    if let Some(s) = c.system.as_ref().filter(|s| !s.is_empty()) {
        a.push("--append-system-prompt".into());
        a.push(s.clone());
    }
    a
}

fn clip(s: &str, n: usize) -> String {
    let one = s.replace(['\r', '\n'], " ");
    if one.chars().count() <= n {
        one
    } else {
        let mut t: String = one.chars().take(n).collect();
        t.push('…');
        t
    }
}

/// The activity lines one stream event becomes.
pub fn activity(v: &Value, t: u64) -> Vec<Activity> {
    let mut out = Vec::new();
    let line = |kind: &str, text: String, detail: String| Activity {
        t,
        kind: kind.into(),
        text,
        detail,
    };
    match v["type"].as_str() {
        Some("assistant") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => {
                        let text = b["text"].as_str().unwrap_or_default();
                        if !text.trim().is_empty() {
                            out.push(line("say", clip(text, 300), String::new()));
                        }
                    }
                    Some("tool_use") => {
                        let name = b["name"].as_str().unwrap_or_default();
                        let input = &b["input"];
                        let path = input["file_path"]
                            .as_str()
                            .or_else(|| input["path"].as_str())
                            .unwrap_or_default();
                        let (kind, text) = match name {
                            "Read" => ("read", path.to_string()),
                            "Edit" | "MultiEdit" => ("edit", path.to_string()),
                            "Write" => ("write", path.to_string()),
                            "Bash" => (
                                "run",
                                input["command"].as_str().unwrap_or_default().to_string(),
                            ),
                            "Grep" | "Glob" => (
                                "search",
                                input["pattern"].as_str().unwrap_or_default().to_string(),
                            ),
                            n if n.starts_with("mcp__fallow__") => {
                                ("tool", n.trim_start_matches("mcp__fallow__").to_string())
                            }
                            n => ("tool", n.to_string()),
                        };
                        out.push(line(kind, clip(&text, 300), String::new()));
                    }
                    _ => {}
                }
            }
        }
        Some("user") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                if b["type"].as_str() == Some("tool_result") {
                    let text = match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a
                            .iter()
                            .filter_map(|x| x["text"].as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                        _ => String::new(),
                    };
                    let kind = if b["is_error"].as_bool() == Some(true) {
                        "error"
                    } else {
                        "result"
                    };
                    out.push(line(kind, clip(&text, 200), String::new()));
                }
            }
        }
        _ => {}
    }
    out
}

/// The final `result` event: the structured result, why there is none, the cost, the session
/// and the refused commands.
pub type Final = (
    Option<Value>,
    Option<String>,
    Option<f64>,
    Option<String>,
    Vec<String>,
);

/// What the final `result` event says.
pub fn result_of(v: &Value) -> Final {
    let denials = v["permission_denials"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            d["tool_input"]["command"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| d["tool_name"].as_str().unwrap_or("a tool").to_string())
        })
        .collect();
    let cost = v["total_cost_usd"].as_f64();
    let session = v["session_id"].as_str().map(str::to_string);
    let structured = v
        .get("structured_output")
        .filter(|s| s.is_object())
        .cloned();
    let failure = if v["is_error"].as_bool() == Some(true) {
        Some(format!(
            "the agent ended with an error: {}",
            clip(v["result"].as_str().unwrap_or("no message"), 300)
        ))
    } else if structured.is_none() {
        Some("the agent gave no structured result".into())
    } else {
        None
    };
    (structured, failure, cost, session, denials)
}

/// Runs a call to its end or its time limit. `on_activity` gets each line as it comes; `stop`
/// ends the call early (the engineer took over or paused, or Fallow is stopping).
pub fn run(c: &Call, on_activity: &mut dyn FnMut(Activity), stop: &Arc<AtomicBool>) -> Outcome {
    let started = Instant::now();
    let fail = |why: String, started: Instant| Outcome {
        result: None,
        failure: Some(why),
        cost_usd: None,
        session: None,
        denials: Vec::new(),
        duration: started.elapsed(),
    };
    let program = crate::platform::find_program(&c.agent.command)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| c.agent.command.clone());
    let mut cmd = Command::new(&program);
    cmd.args(&c.agent.args)
        .args(args(c))
        .current_dir(&c.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window for the service's children.
        cmd.creation_flags(0x0800_0000);
    }
    let mut group = match crate::platform::job::Group::new() {
        Ok(g) => g,
        Err(e) => return fail(format!("cannot create a process group: {e}"), started),
    };
    let mut child = match cmd.spawn() {
        Ok(ch) => ch,
        Err(e) => return fail(format!("cannot start {}: {e}", c.agent.command), started),
    };
    let _ = group.adopt(child.id());
    if let Some(mut stdin) = child.stdin.take() {
        let text = c.instruction.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let stderr = child.stderr.take();
    let err_tail = Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(e) = stderr {
        let tail = Arc::clone(&err_tail);
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(e).lines().map_while(Result::ok) {
                if let Ok(mut t) = tail.lock() {
                    t.push_str(&line);
                    t.push('\n');
                    if t.len() > 4096 {
                        let cut = t.len() - 2048;
                        *t = t[cut..].to_string();
                    }
                }
            }
        });
    }
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
    }
    let mut last: Option<Value> = None;
    let mut ended_by: Option<String> = None;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(v) => {
                let t = started.elapsed().as_millis() as u64;
                for a in activity(&v, t) {
                    on_activity(a);
                }
                if v["type"].as_str() == Some("result") {
                    last = Some(v);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if started.elapsed() >= c.time_limit {
            ended_by = Some(format!(
                "the step reached its time limit of {} min",
                c.time_limit.as_secs() / 60
            ));
            group.kill();
            break;
        }
        if stop.load(Ordering::Relaxed) {
            ended_by = Some("the step was ended by Fallow".into());
            group.kill();
            break;
        }
    }
    let status = child.wait().ok();
    let duration = started.elapsed();
    if let Some(why) = ended_by {
        return Outcome {
            session: Some(c.session.id().to_string()),
            ..fail(why, started)
        };
    }
    match last {
        Some(v) => {
            let (result, failure, cost_usd, session, denials) = result_of(&v);
            Outcome {
                result,
                failure,
                cost_usd,
                session: session.or_else(|| Some(c.session.id().to_string())),
                denials,
                duration,
            }
        }
        None => {
            let tail = err_tail
                .lock()
                .map(|t| t.trim().to_string())
                .unwrap_or_default();
            let code = status.and_then(|s| s.code());
            Outcome {
                session: Some(c.session.id().to_string()),
                ..fail(
                    format!(
                        "the agent ended without a result (exit {}){}",
                        code.map_or("unknown".to_string(), |c| c.to_string()),
                        if tail.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", clip(&tail, 300))
                        }
                    ),
                    started,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use serde_json::json;
    use std::time::Duration;

    fn call() -> Call {
        Call {
            agent: Agent {
                command: "claude".into(),
                args: Vec::new(),
            },
            instruction: "x".into(),
            schema: Some(json!({"type": "object"})),
            allowed: vec!["Read".into(), "Bash(git status:*)".into()],
            mcp_config: None,
            session: Session::New("u-1".into()),
            model: Some("sonnet".into()),
            budget_usd: Some(4.5),
            cwd: ".".into(),
            time_limit: Duration::from_secs(60),
            system: Some("rules".into()),
            unrestricted: false,
        }
    }

    #[test]
    fn the_command_line_matches_research_r3() {
        let a = args(&call());
        let s = a.join(" ");
        assert!(s.starts_with("-p --output-format stream-json --verbose --json-schema"));
        assert!(s.contains("--permission-mode dontAsk --allowedTools Read Bash(git status:*)"));
        assert!(s.contains("--session-id u-1"));
        assert!(s.contains("--model sonnet"));
        assert!(s.contains("--max-budget-usd 4.50"));
        assert!(s.contains("--append-system-prompt rules"));
        assert!(!s.contains("dangerously"));
        let mut c = call();
        c.session = Session::Resume("u-1".into());
        c.unrestricted = true;
        let s = args(&c).join(" ");
        assert!(s.contains("--resume u-1"));
        assert!(s.contains("--dangerously-skip-permissions"));
        assert!(!s.contains("dontAsk"));
    }

    #[test]
    fn stream_events_become_activity_and_a_result() {
        let a = activity(
            &json!({"type":"assistant","message":{"content":[
                {"type":"text","text":"Looking."},
                {"type":"tool_use","name":"Edit","input":{"file_path":"shop/cart.py"}},
                {"type":"tool_use","name":"Bash","input":{"command":"python -m unittest"}}]}}),
            5,
        );
        let kinds: Vec<&str> = a.iter().map(|x| x.kind.as_str()).collect();
        assert_eq!(kinds, ["say", "edit", "run"]);
        assert_eq!(a[1].text, "shop/cart.py");
        let (r, f, cost, sess, den) = result_of(&json!({
            "type":"result","is_error":false,"total_cost_usd":0.25,"session_id":"s",
            "structured_output":{"status":"done"},
            "permission_denials":[{"tool_name":"Bash","tool_input":{"command":"git push"}}]}));
        assert_eq!(r.unwrap()["status"], "done");
        assert!(f.is_none());
        assert_eq!(cost, Some(0.25));
        assert_eq!(sess.as_deref(), Some("s"));
        assert_eq!(den, ["git push"]);
        let (r, f, ..) = result_of(&json!({"type":"result","is_error":false}));
        assert!(r.is_none() && f.unwrap().contains("no structured result"));
    }
}
