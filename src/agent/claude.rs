//! The Claude Code runtime: `claude -p` without a terminal (006 research R-3, R-4; 007
//! research S-5, S-7, verified 2026-10-30).

use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde_json::Value;

use super::rights::{Edits, Rights};
use super::{Agent, Availability, Call, Event, Outcome, Runtime, Session, ToolKind, clip};

/// The runtime for an agent program that speaks `claude -p`: Claude Code itself, or
/// `fallow-fake-agent` in tests.
#[derive(Debug, Clone)]
pub struct Claude {
    pub agent: Agent,
}

impl Claude {
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Claude {
        Claude {
            agent: Agent {
                command: command.into(),
                args,
            },
        }
    }
}

impl Runtime for Claude {
    fn name(&self) -> &str {
        "claude"
    }

    fn available(&self) -> Availability {
        let Some(program) = crate::platform::find_program(&self.agent.command) else {
            return Availability {
                installed: false,
                version: None,
            };
        };
        let version = crate::platform::command(program)
            .args(&self.agent.args)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        Availability {
            installed: true,
            version,
        }
    }

    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
        run(&self.agent, call, sink, stop)
    }
}

/// `prefix` as an allow rule for both shell tools (verified on a real run: a command refused as
/// Bash is tried as PowerShell).
fn shell_rules(prefix: &str) -> [String; 2] {
    [
        format!("Bash({prefix}:*)"),
        format!("PowerShell({prefix}:*)"),
    ]
}

fn push(v: &mut Vec<String>, items: impl IntoIterator<Item = String>) {
    for i in items {
        if !v.contains(&i) {
            v.push(i);
        }
    }
}

/// The allow rules, the deny rules and the permission mode for `rights` (research S-7).
pub fn rules(r: &Rights) -> (Vec<String>, Vec<String>, &'static str) {
    let mut allow: Vec<String> = ["Read", "Grep", "Glob", "mcp__fallow__*"]
        .map(String::from)
        .to_vec();
    let mut deny = Vec::new();
    match &r.edits {
        Edits::None => {}
        Edits::Anywhere => push(&mut allow, ["Edit", "Write"].map(String::from)),
        Edits::Only(globs) => {
            for g in globs {
                push(&mut allow, [format!("Edit({g})"), format!("Write({g})")]);
            }
        }
    }
    for p in &r.protect {
        push(&mut deny, [format!("Edit({p})"), format!("Write({p})")]);
    }
    if r.no_commands {
        push(&mut deny, ["Bash", "PowerShell"].map(String::from));
    } else {
        // Reading commands are Claude Code's own read-only allowance (T001), which judges each
        // command; a prefix rule would allow `sort -o FILE` or `git diff --output=FILE` too
        // (Phase 2 review). Moving around the worktree is allowed.
        push(&mut allow, shell_rules("cd"));
        for c in &r.commands {
            push(&mut allow, shell_rules(c.trim()));
        }
        if r.any_command {
            push(&mut allow, ["Bash", "PowerShell"].map(String::from));
        }
    }
    // The prompt tool is called only in the default mode; dontAsk denies without asking (T001).
    let mode = if r.prompt_tool.is_some() {
        "default"
    } else {
        "dontAsk"
    };
    (allow, deny, mode)
}

/// The arguments after the agent's own leading arguments (research R-3, S-5, S-7). The
/// instruction goes on stdin, which also keeps it off the command line.
pub fn args(c: &Call) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if c.partial {
        a.push("--include-partial-messages".into());
    }
    if let Some(s) = &c.schema {
        a.push("--json-schema".into());
        a.push(s.to_string());
    }
    let (allow, deny, mode) = rules(&c.rights);
    a.push("--permission-mode".into());
    a.push(mode.into());
    a.push("--allowedTools".into());
    a.extend(allow);
    if !deny.is_empty() {
        a.push("--disallowedTools".into());
        a.extend(deny);
    }
    if let Some(t) = &c.rights.prompt_tool {
        a.push("--permission-prompt-tool".into());
        a.push(t.clone());
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
        Session::Fork(id) => {
            a.push("--resume".into());
            a.push(id.clone());
            a.push("--fork-session".into());
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

/// An edit's diff, as `-` and `+` lines.
fn diff(old: &str, new: &str) -> String {
    let mut out = String::new();
    for l in old.lines() {
        out.push('-');
        out.push_str(l);
        out.push('\n');
    }
    for l in new.lines() {
        out.push('+');
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// What a `tool_use` block becomes.
fn tool_call(b: &Value) -> Event {
    let name = b["name"].as_str().unwrap_or_default();
    let input = &b["input"];
    let path = input["file_path"]
        .as_str()
        .or_else(|| input["notebook_path"].as_str())
        .or_else(|| input["path"].as_str())
        .unwrap_or_default()
        .to_string();
    let (kind, summary, detail) = match name {
        "Read" => (ToolKind::Read, path, String::new()),
        "Edit" => (
            ToolKind::Edit,
            path,
            diff(
                input["old_string"].as_str().unwrap_or_default(),
                input["new_string"].as_str().unwrap_or_default(),
            ),
        ),
        "MultiEdit" => (
            ToolKind::Edit,
            path,
            input["edits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| {
                    diff(
                        e["old_string"].as_str().unwrap_or_default(),
                        e["new_string"].as_str().unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>()
                .join("…\n"),
        ),
        "Write" => (
            ToolKind::Write,
            path,
            diff("", input["content"].as_str().unwrap_or_default()),
        ),
        "Bash" | "PowerShell" => {
            let cmd = input["command"].as_str().unwrap_or_default().to_string();
            (ToolKind::Run, cmd.clone(), cmd)
        }
        "Grep" | "Glob" => (
            ToolKind::Search,
            input["pattern"].as_str().unwrap_or_default().to_string(),
            String::new(),
        ),
        n if n.starts_with("mcp__fallow__") => (
            ToolKind::Mcp,
            n.trim_start_matches("mcp__fallow__").to_string(),
            input.to_string(),
        ),
        n => (ToolKind::Other, n.to_string(), String::new()),
    };
    Event::ToolCall {
        id: b["id"].as_str().unwrap_or_default().to_string(),
        kind,
        summary,
        detail,
    }
}

/// Whether a tool result is a refusal by the permission rules or the prompt tool.
fn refusal(text: &str) -> bool {
    text.starts_with("Permission to use")
        || text.contains("has been denied")
        || text.contains("requires approval")
        || text.starts_with("The engineer refused")
}

/// The events one stream-json line becomes. Thinking is dropped (research S-5): `thinking`
/// blocks in messages and `thinking_delta` in partial messages.
pub fn events(v: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    match v["type"].as_str() {
        Some("assistant") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => out.push(Event::Message {
                        text: b["text"].as_str().unwrap_or_default().to_string(),
                    }),
                    Some("tool_use") => out.push(tool_call(b)),
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
                            .join("\n"),
                        _ => String::new(),
                    };
                    let error = b["is_error"].as_bool() == Some(true);
                    out.push(Event::ToolResult {
                        id: b["tool_use_id"].as_str().unwrap_or_default().to_string(),
                        ok: !error,
                        refused: error && refusal(&text),
                        output: text,
                    });
                }
            }
        }
        Some("stream_event") => {
            let e = &v["event"];
            if e["type"].as_str() == Some("content_block_delta")
                && e["delta"]["type"].as_str() == Some("text_delta")
            {
                out.push(Event::MessageDelta {
                    text: e["delta"]["text"].as_str().unwrap_or_default().to_string(),
                });
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

/// Runs a call to its end or its time limit.
pub fn run(agent: &Agent, c: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
    let started = Instant::now();
    let fail = |why: String, started: Instant| Outcome {
        result: None,
        failure: Some(why),
        cost_usd: None,
        session: None,
        denials: Vec::new(),
        duration: started.elapsed(),
    };
    let program = crate::platform::find_program(&agent.command)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| agent.command.clone());
    let mut cmd = Command::new(&program);
    cmd.args(&agent.args)
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
    let group = match &c.group {
        Some(g) => std::sync::Arc::clone(g),
        None => match crate::platform::job::Group::new() {
            Ok(g) => std::sync::Arc::new(g),
            Err(e) => return fail(format!("cannot create a process group: {e}"), started),
        },
    };
    let mut child = match cmd.spawn() {
        Ok(ch) => ch,
        Err(e) => return fail(format!("cannot start {}: {e}", agent.command), started),
    };
    let _ = group.adopt(child.id());
    if let Some(mut stdin) = child.stdin.take() {
        let text = c.instruction.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let stderr = child.stderr.take();
    let err_tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(e) = stderr {
        let tail = std::sync::Arc::clone(&err_tail);
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(e).lines().map_while(Result::ok) {
                if let Ok(mut t) = tail.lock() {
                    t.push_str(&line);
                    t.push('\n');
                    if t.len() > 4096 {
                        let mut cut = t.len() - 2048;
                        while !t.is_char_boundary(cut) {
                            cut += 1;
                        }
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
    let mut session_seen: Option<String> = None;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(v) => {
                if session_seen.is_none() {
                    session_seen = v["session_id"].as_str().map(str::to_string);
                }
                for e in events(&v) {
                    sink(e);
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
    // A fork's new id comes in the stream; a new or resumed session keeps the id given.
    let session_now = match &c.session {
        Session::Fork(_) => session_seen.clone(),
        s => Some(s.id().to_string()),
    };
    if let Some(why) = ended_by {
        return Outcome {
            session: session_now,
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
                session: session.or(session_now),
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
                session: session_now,
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
    use crate::agent::rights::{self, Mode};
    use serde_json::json;
    use std::time::Duration;

    fn call() -> Call {
        Call {
            instruction: "x".into(),
            schema: Some(json!({"type": "object"})),
            rights: rights::maker(
                vec!["python -m unittest".into()],
                vec!["specs/007-x/spec.md".into()],
            ),
            mcp_config: None,
            session: Session::New("u-1".into()),
            model: Some("sonnet".into()),
            budget_usd: Some(4.5),
            cwd: ".".into(),
            time_limit: Duration::from_secs(60),
            system: Some("rules".into()),
            partial: false,
            group: None,
        }
    }

    fn has(a: &[String], s: &str) -> bool {
        a.iter().any(|x| x == s)
    }

    #[test]
    fn the_command_line_matches_research_r3_and_s7() {
        let a = args(&call());
        let s = a.join(" ");
        assert!(s.starts_with("-p --output-format stream-json --verbose --json-schema"));
        assert!(s.contains("--permission-mode dontAsk --allowedTools Read"));
        for r in [
            "Edit",
            "Write",
            "Bash(python -m unittest:*)",
            "PowerShell(python -m unittest:*)",
            "mcp__fallow__*",
        ] {
            assert!(has(&a, r), "lacks {r}");
        }
        assert!(
            s.contains("--disallowedTools Edit(specs/007-x/spec.md) Write(specs/007-x/spec.md)")
        );
        assert!(s.contains("--session-id u-1"));
        assert!(s.contains("--model sonnet"));
        assert!(s.contains("--max-budget-usd 4.50"));
        assert!(s.contains("--append-system-prompt rules"));
        assert!(!s.contains("dangerously"));
        assert!(!s.contains("--include-partial-messages"));
        let mut c = call();
        c.session = Session::Fork("u-1".into());
        c.partial = true;
        let s = args(&c).join(" ");
        assert!(s.contains("--resume u-1 --fork-session"));
        assert!(s.contains("--include-partial-messages"));
    }

    #[test]
    fn chat_modes_become_claude_options() {
        let mut c = call();
        c.rights = rights::lead(Mode::ReadOnly, vec!["python -m shop".into()]);
        let a = args(&c);
        let s = a.join(" ");
        assert!(s.contains("--permission-mode dontAsk"));
        let (_, deny, _) = rules(&c.rights);
        assert!(has(&deny, "Bash") && has(&deny, "PowerShell"), "{deny:?}");
        assert!(has(&deny, "Write(.fallow/squad.toml)"));
        assert!(has(&a, "Edit(specs/**)") && !has(&a, "Edit"));
        assert!(!s.contains("python -m shop"));
        c.rights = rights::lead(Mode::Ask, vec!["python -m shop".into()]);
        let s = args(&c).join(" ");
        assert!(s.contains("--permission-mode default"));
        assert!(s.contains("--permission-prompt-tool mcp__fallow__approve"));
        assert!(s.contains("Bash(python -m shop:*)"));
        c.rights = rights::lead(Mode::RunAnything, Vec::new());
        let a = args(&c);
        assert!(has(&a, "Bash") && has(&a, "PowerShell"));
        assert!(
            !has(&a, "Edit")
                && has(&a, "Write(.fallow/drafts/**)")
                && has(&a, "Edit(.fallow/pipelines/**)")
        );
        c.rights = rights::checker(vec!["python -m unittest".into()]);
        let a = args(&c);
        assert!(!has(&a, "Edit") && !has(&a, "Write"));
    }

    #[test]
    fn stream_events_become_events_without_thinking() {
        let e = events(&json!({"type":"assistant","message":{"content":[
            {"type":"thinking","thinking":"secret"},
            {"type":"text","text":"Looking."},
            {"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"shop/cart.py","old_string":"a","new_string":"b"}},
            {"type":"tool_use","id":"t2","name":"PowerShell","input":{"command":"python -m unittest"}}]}}));
        assert_eq!(e.len(), 3);
        assert_eq!(
            e[0],
            Event::Message {
                text: "Looking.".into()
            }
        );
        assert_eq!(
            e[1],
            Event::ToolCall {
                id: "t1".into(),
                kind: ToolKind::Edit,
                summary: "shop/cart.py".into(),
                detail: "-a\n+b\n".into()
            }
        );
        assert!(
            matches!(&e[2], Event::ToolCall { kind: ToolKind::Run, summary, .. } if summary == "python -m unittest")
        );
        let r = events(&json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"t2","is_error":true,"content":"Permission to use Bash has been denied because Claude Code is running in don't ask mode."}]}}));
        assert!(matches!(
            &r[0],
            Event::ToolResult {
                refused: true,
                ok: false,
                ..
            }
        ));
        let d = events(
            &json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}),
        );
        assert_eq!(d, [Event::MessageDelta { text: "Hel".into() }]);
        let t = events(
            &json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}}),
        );
        assert!(t.is_empty());
    }

    #[test]
    fn the_result_event_is_read() {
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
