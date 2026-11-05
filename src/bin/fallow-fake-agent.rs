//! `fallow-fake-agent`: a scripted stand-in for `claude -p` (006 research R-15; 007 research
//! S-13, the `fake` runtime).
//!
//! It takes the same flags, reads its instruction on stdin, and writes the same stream-json,
//! ending with a `result` event carrying `structured_output`, `total_cost_usd`, `session_id`
//! and `permission_denials`. What it does is scripted in a TOML file passed with `--scenario`
//! (an agent profile's leading argument): each `[[step]]` answers one instruction whose header
//! line `Fallow step: <kind> (part <p>)` matches, once, in order.
//!
//! ```toml
//! [[step]]
//! kind = "build"                 # build, repair, review, handover, lead_turn, ...
//! part = "main"                  # optional
//! when = "text in the instruction"  # optional, e.g. a task id "T014"
//! model = "opus"                # optional: only a call with this --model
//! system = "text"                # optional: only a call whose --append-system-prompt has it
//! forked = false                # optional: only a call that forks (true) or does not (false)
//! files = [{ path = "shop/x.py", text = "..." }]   # with old = "..." it is an Edit
//! delete = ["shop/old.py"]       # files removed (after `files`, before the commit)
//! commit = "message"             # git add -A; git commit -m
//! output = { status = "done", commit = "$HEAD", summary = "..." }
//! cost = 0.05
//! sleep_ms = 0
//! linger_ms = 0                  # waits after its files and commit, before its result
//! exit = 3                       # crash with this exit code instead of a result
//! denied = ["git push"]          # reported as permission denials
//! is_error = false
//! say = "a reply"                # an assistant message, streamed as deltas with
//!                                # --include-partial-messages
//! ask = "a question"             # calls Fallow's MCP `ask` tool; its answer replaces $ANSWER
//! also_ask = "another question"  # a second `ask` right after; its answer replaces $ANSWER2
//! run = [{ command = "python -m unittest", output = "OK", ok = true }]
//!                                # commands it "ran" (scripted output), refused like Claude
//!                                # Code when the rules do not allow them
//! think = "..."                  # a thinking block, which Fallow must never show
//! approve = "python -c 1"        # a command outside the rules: asks the --permission-prompt-tool
//!                                # in the default mode, or is refused under dontAsk;
//!                                # $APPROVED becomes "allow" or "deny"
//! exec = ["git branch x"]        # 012: commands really run (sh -c) in the working directory,
//!                                # each going through the permission rules first, as Claude
//!                                # Code does: deny wins, then an ask rule (from --settings)
//!                                # goes to the prompt tool, then an allow rule runs it, and
//!                                # what none covers is asked (default) or refused (dontAsk)
//! expect_images = 1              # 012: the step fails unless the instruction came as a
//!                                # stream-json message (--input-format stream-json) with this
//!                                # many image blocks
//! web = ["https://example.com"]  # 012: WebFetch tool calls; refused when --disallowedTools or
//!                                # --tools leaves WebFetch out
//! lock_index = true              # 012: leaves git's index.lock in the working copy's git
//!                                # folder, so Fallow's commit after the turn fails
//! read = ["notes.txt"]           # 012: Read tool calls through the same rules as `exec`: a
//!                                # `Read` ask rule sends each to the prompt tool; an allowed
//!                                # read returns the file's text
//! ```
//!
//! 012: with `FALLOW_FAKE_ARGS_LOG` set, each invocation appends its arguments to that file, one
//! per line, then a line `---`, so tests can check the flags Fallow passed.
//!
//! A resumed call (`--resume`) with no step left for it answers as Claude Code does for a
//! session it does not know: "No conversation found with session ID: …". With `--fork-session`
//! the call runs in a new session id, which its events report.
//!
//! Test-only tooling; never started by Fallow unless an agent profile names it.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct File {
    path: String,
    text: String,
    old: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Step {
    kind: String,
    part: Option<String>,
    when: Option<String>,
    model: Option<String>,
    system: Option<String>,
    forked: Option<bool>,
    #[serde(default)]
    files: Vec<File>,
    #[serde(default)]
    delete: Vec<String>,
    commit: Option<String>,
    output: Option<toml::Value>,
    #[serde(default = "cost")]
    cost: f64,
    #[serde(default)]
    sleep_ms: u64,
    #[serde(default)]
    linger_ms: u64,
    exit: Option<i32>,
    #[serde(default)]
    denied: Vec<String>,
    #[serde(default)]
    is_error: bool,
    say: Option<String>,
    ask: Option<String>,
    also_ask: Option<String>,
    approve: Option<String>,
    #[serde(default)]
    run: Vec<Ran>,
    think: Option<String>,
    #[serde(default)]
    exec: Vec<String>,
    expect_images: Option<usize>,
    #[serde(default)]
    web: Vec<String>,
    #[serde(default)]
    read: Vec<String>,
    #[serde(default)]
    lock_index: bool,
}

#[derive(Debug, Deserialize)]
struct Ran {
    command: String,
    #[serde(default)]
    output: String,
    #[serde(default = "yes")]
    ok: bool,
}

fn yes() -> bool {
    true
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

fn substitute(v: Value, vars: &[(&str, &str)]) -> Value {
    match v {
        Value::String(mut s) => {
            for (k, x) in vars {
                s = s.replace(k, x);
            }
            Value::String(s)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|x| substitute(x, vars)).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| (k, substitute(x, vars)))
                .collect(),
        ),
        other => other,
    }
}

/// Calls one tool of the `fallow` MCP server named in `--mcp-config`, as Claude Code would:
/// start it, initialize, call, and return the text of the result.
fn mcp_call(config: &Path, tool: &str, arguments: Value) -> Result<String, String> {
    let cfg: Value = serde_json::from_str(
        &std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?,
    )
    .map_err(|e| e.to_string())?;
    let server = &cfg["mcpServers"]["fallow"];
    let command = server["command"].as_str().ok_or("no fallow server")?;
    let args: Vec<String> = server["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str().map(str::to_string))
        .collect();
    let mut child = Command::new(command)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {command}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut out = std::io::BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut request = |id: u64, method: &str, params: Value| -> Result<Value, String> {
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(stdin, "{msg}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            if out.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("the MCP server closed".into());
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"] == json!(id) {
                if let Some(e) = v.get("error") {
                    return Err(e.to_string());
                }
                return Ok(v["result"].clone());
            }
        }
    };
    request(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "fallow-fake-agent", "version": "0"}}),
    )?;
    let r = request(
        2,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )?;
    let _ = child.kill();
    Ok(r["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(log) = std::env::var("FALLOW_FAKE_ARGS_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            // One argument per line; a newline inside one is written as `\n`.
            for a in &args {
                let _ = writeln!(f, "{}", a.replace('\n', "\\n"));
            }
            let _ = writeln!(f, "---");
        }
    }
    // 009: playing an editor. `--record <file> <args...>` appends each argument, one per line,
    // and exits: tests see what "Open in my editor" started it with.
    if args.first().is_some_and(|a| a == "--record") {
        let Some(log) = args.get(1) else {
            return ExitCode::from(2);
        };
        use std::io::Write;
        let mut f = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            Ok(f) => f,
            Err(_) => return ExitCode::from(2),
        };
        for a in &args[2..] {
            let _ = writeln!(f, "{a}");
        }
        return ExitCode::SUCCESS;
    }
    let mut scenario: Option<PathBuf> = None;
    let mut session = String::from("fake-session");
    let mut resumed = false;
    let mut fork = false;
    let mut partial = false;
    let mut mcp_config: Option<PathBuf> = None;
    let mut prompt_tool: Option<String> = None;
    let mut mode = String::from("default");
    let mut model = String::new();
    let mut system = String::new();
    let mut allowed: Vec<String> = Vec::new();
    let mut disallowed: Vec<String> = Vec::new();
    let mut asked: Vec<String> = Vec::new();
    let mut tools: Option<Vec<String>> = None;
    let mut stream_input = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" => {
                scenario = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--session-id" | "--resume" => {
                resumed |= args[i] == "--resume";
                if let Some(s) = args.get(i + 1) {
                    session = s.clone();
                }
                i += 1;
            }
            "--fork-session" => fork = true,
            "--model" | "--append-system-prompt" => {
                if let Some(v) = args.get(i + 1) {
                    if args[i] == "--model" {
                        model = v.clone();
                    } else {
                        system = v.clone();
                    }
                }
                i += 1;
            }
            "--include-partial-messages" => partial = true,
            "--mcp-config" => {
                mcp_config = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--permission-prompt-tool" => {
                prompt_tool = args.get(i + 1).cloned();
                i += 1;
            }
            "--permission-mode" => {
                if let Some(m) = args.get(i + 1) {
                    mode = m.clone();
                }
                i += 1;
            }
            "--settings" => {
                // Only the ask rules matter to the fake (012 research R-6).
                if let Some(v) = args.get(i + 1) {
                    if let Ok(j) = serde_json::from_str::<Value>(v) {
                        for r in j["permissions"]["ask"].as_array().into_iter().flatten() {
                            if let Some(r) = r.as_str() {
                                asked.push(r.to_string());
                            }
                        }
                    }
                }
                i += 1;
            }
            "--input-format" => {
                stream_input = args.get(i + 1).is_some_and(|v| v == "stream-json");
                i += 1;
            }
            "--tools" => {
                let mut t = Vec::new();
                while let Some(v) = args.get(i + 1).filter(|v| !v.starts_with("--")) {
                    t.extend(v.split(',').map(|x| x.trim().to_string()));
                    i += 1;
                }
                tools = Some(t);
            }
            "--allowedTools" | "--disallowedTools" => {
                let into = if args[i] == "--allowedTools" {
                    &mut allowed
                } else {
                    &mut disallowed
                };
                while let Some(v) = args.get(i + 1).filter(|v| !v.starts_with("--")) {
                    into.push(v.clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if fork {
        // A fork runs in a new session that remembers the old one.
        session = format!("{session}-fork-{}", std::process::id());
    }
    let Some(scenario) = scenario else {
        eprintln!("fallow-fake-agent: --scenario is required");
        return ExitCode::from(2);
    };
    let mut instruction = String::new();
    let _ = std::io::stdin().read_to_string(&mut instruction);
    // 012: a stream-json user message carries the text and the images (research R-8).
    let mut images = 0usize;
    if stream_input {
        let mut text = String::new();
        for line in instruction.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for c in v["message"]["content"].as_array().into_iter().flatten() {
                match c["type"].as_str() {
                    Some("text") => text.push_str(c["text"].as_str().unwrap_or_default()),
                    Some("image") => images += 1,
                    _ => {}
                }
            }
        }
        instruction = text;
    }
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
    // Two agents of one loop can start at once (008: checkers running alongside each other):
    // the record of used steps is read and written under a lock, so they never pick the same.
    let lock = Lock::take(&scenario.with_extension("lock"));
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
            && s.model.as_ref().is_none_or(|m| *m == model)
            && s.forked.is_none_or(|f| f == fork)
            && s.system
                .as_ref()
                .is_none_or(|w| system.contains(w.as_str()))
    });
    let Some((n, step)) = pick else {
        drop(lock);
        if resumed && !fork {
            // What Claude Code 2.1 does for a session it does not know: an error result with no
            // message, the reason on stderr only (T086).
            eprintln!("No conversation found with session ID: {session}");
            emit(
                json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
                "session_id": session, "total_cost_usd": 0.0, "permission_denials": []}),
            );
        } else {
            emit(
                json!({"type": "result", "subtype": "error", "is_error": true, "session_id": session,
                "total_cost_usd": 0.0, "result": format!("no scripted step for {kind} {part:?}"),
                "permission_denials": []}),
            );
        }
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
    drop(lock);
    emit(
        json!({"type": "assistant", "session_id": session, "message": {"content": [{"type": "text", "text": format!("Working on {kind}.")}]}}),
    );
    let mut tool_n = 0;
    let mut tool_id = || {
        tool_n += 1;
        format!("toolu_{tool_n}")
    };
    if let Some(say) = &step.say {
        if partial {
            for chunk in say.split_inclusive(' ') {
                emit(
                    json!({"type": "stream_event", "session_id": session, "event": {
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": chunk}}}),
                );
            }
        }
        emit(
            json!({"type": "assistant", "session_id": session, "message": {"content": [{"type": "text", "text": say}]}}),
        );
    }
    if let Some(t) = &step.think {
        emit(
            json!({"type": "assistant", "session_id": session, "message": {"content": [
            {"type": "thinking", "thinking": t, "signature": "x"}]}}),
        );
    }
    for r in &step.run {
        let id = tool_id();
        let cmd = r.command.as_str();
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": cmd}}]}}));
        let covers = |rules: &[String]| {
            rules.iter().any(|x| {
                x == "Bash"
                    || x.strip_prefix("Bash(")
                        .and_then(|y| y.strip_suffix(":*)"))
                        .is_some_and(|p| cmd == p || cmd.starts_with(&format!("{p} ")))
            })
        };
        if covers(&disallowed) || !covers(&allowed) {
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": true,
                 "content": "Permission to use Bash has been denied because Claude Code is running in don't ask mode."}]}}));
        } else {
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": !r.ok, "content": r.output}]}}));
        }
    }
    let mut ask = |q: &str| -> String {
        let id = tool_id();
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "mcp__fallow__ask", "input": {"question": q}}]}}));
        let answer = match &mcp_config {
            Some(c) => {
                mcp_call(c, "ask", json!({"question": q})).unwrap_or_else(|e| format!("error: {e}"))
            }
            None => "error: no --mcp-config".into(),
        };
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": answer}]}}));
        answer
    };
    let answer = step.ask.as_deref().map(&mut ask).unwrap_or_default();
    let answer2 = step.also_ask.as_deref().map(&mut ask).unwrap_or_default();
    let mut approved = String::new();
    let mut prompt_denials: Vec<Value> = Vec::new();
    if let Some(cmd) = &step.approve {
        let id = tool_id();
        let input = json!({"command": cmd, "description": "scripted"});
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": input}]}}));
        // As Claude Code does (T001): the prompt tool is asked only in the default mode.
        // As Claude Code does: a deny rule wins, an allow rule runs it without asking, and only
        // what neither covers is asked about (default mode) or refused (dontAsk).
        let covers = |rules: &[String]| {
            rules.iter().any(|r| {
                r == "Bash"
                    || r.strip_prefix("Bash(")
                        .and_then(|x| x.strip_suffix(":*)"))
                        .is_some_and(|p| cmd == p || cmd.starts_with(&format!("{p} ")))
            })
        };
        let verdict = match (&prompt_tool, mode.as_str(), &mcp_config) {
            _ if covers(&disallowed) => json!({"behavior": "deny",
                "message": "Permission to use Bash has been denied."}),
            _ if covers(&allowed) => json!({"behavior": "allow"}),
            (Some(t), "default", Some(c)) => {
                let tool = t.trim_start_matches("mcp__fallow__");
                mcp_call(
                    c,
                    tool,
                    json!({"tool_name": "Bash", "input": input, "tool_use_id": id}),
                )
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .unwrap_or_else(|| json!({"behavior": "deny", "message": "the prompt tool failed"}))
            }
            _ => json!({"behavior": "deny",
                "message": "Permission to use Bash has been denied because Claude Code is running in don't ask mode."}),
        };
        if verdict["behavior"] == "allow" {
            approved = "allow".into();
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "content": "ran"}]}}));
        } else {
            approved = "deny".into();
            let msg = verdict["message"].as_str().unwrap_or("denied").to_string();
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": true, "content": msg}]}}));
            prompt_denials
                .push(json!({"tool_name": "Bash", "tool_use_id": id, "tool_input": input}));
        }
    }
    if let Some(n) = step.expect_images {
        if n != images {
            emit(json!({"type": "result", "subtype": "error", "is_error": true,
                "session_id": session, "total_cost_usd": 0.0,
                "result": format!("expected {n} images, got {images}"), "permission_denials": []}));
            return ExitCode::SUCCESS;
        }
    }
    let tool_on = |t: &str| tools.as_ref().is_none_or(|l| l.iter().any(|x| x == t));
    for url in &step.web {
        let id = tool_id();
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "WebFetch", "input": {"url": url, "prompt": "read it"}}]}}));
        let off = !tool_on("WebFetch") || disallowed.iter().any(|d| d == "WebFetch");
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "is_error": off,
             "content": if off { "No such tool available: WebFetch" } else { "fetched" }}]}}));
    }
    // 012 (FR-043): files read, through the rules as Claude Code applies them.
    for path in &step.read {
        let id = tool_id();
        let input = json!({"file_path": path});
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Read", "input": input}]}}));
        let whole = |rules: &[String]| rules.iter().any(|r| r == "Read" || r.starts_with("Read("));
        let verdict = if !tool_on("Read") || whole(&disallowed) {
            json!({"behavior": "deny", "message": "Permission to use Read has been denied."})
        } else if whole(&asked) {
            match (&prompt_tool, mode.as_str(), &mcp_config) {
                (Some(t), "default", Some(c)) => {
                    let tool = t.trim_start_matches("mcp__fallow__");
                    mcp_call(c, tool, json!({"tool_name": "Read", "input": input, "tool_use_id": id}))
                        .ok()
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                        .unwrap_or_else(|| json!({"behavior": "deny", "message": "the prompt tool failed"}))
                }
                _ => json!({"behavior": "deny", "message": "Permission to use Read has been denied."}),
            }
        } else {
            json!({"behavior": "allow"})
        };
        let (bad, text) = if verdict["behavior"] == "allow" {
            match std::fs::read_to_string(cwd.join(path)) {
                Ok(t) => (false, t),
                Err(e) => (true, e.to_string()),
            }
        } else {
            prompt_denials.push(json!({"tool_name": "Read", "tool_use_id": id, "tool_input": input}));
            (true, verdict["message"].as_str().unwrap_or("denied").to_string())
        };
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "is_error": bad, "content": text}]}}));
    }
    // 012: commands really run, through the rules as Claude Code applies them (T001's check).
    for cmd in &step.exec {
        let id = tool_id();
        let input = json!({"command": cmd, "description": "scripted"});
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": input}]}}));
        let covers = |rules: &[String]| {
            rules.iter().any(|r| {
                r == "Bash"
                    || r.strip_prefix("Bash(")
                        .and_then(|x| x.strip_suffix(":*)"))
                        .is_some_and(|p| cmd == p || cmd.starts_with(&format!("{p} ")))
            })
        };
        let ask_tool = || -> Value {
            match (&prompt_tool, mode.as_str(), &mcp_config) {
                (Some(t), "default", Some(c)) => {
                    let tool = t.trim_start_matches("mcp__fallow__");
                    mcp_call(c, tool, json!({"tool_name": "Bash", "input": input, "tool_use_id": id}))
                        .ok()
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                        .unwrap_or_else(|| json!({"behavior": "deny", "message": "the prompt tool failed"}))
                }
                _ => json!({"behavior": "deny", "message": "Permission to use Bash has been denied."}),
            }
        };
        let verdict = if !tool_on("Bash") || covers(&disallowed) {
            json!({"behavior": "deny", "message": "Permission to use Bash has been denied."})
        } else if covers(&asked) {
            ask_tool()
        } else if covers(&allowed) {
            json!({"behavior": "allow"})
        } else {
            ask_tool()
        };
        if verdict["behavior"] == "allow" {
            let out = Command::new(shell())
                .current_dir(&cwd)
                .args(["-c", cmd])
                .output();
            let (ok, text) = match out {
                Ok(o) => (
                    o.status.success(),
                    format!(
                        "{}{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    ),
                ),
                Err(e) => (false, e.to_string()),
            };
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": !ok, "content": text}]}}));
        } else {
            let msg = verdict["message"].as_str().unwrap_or("denied").to_string();
            emit(json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": true, "content": msg}]}}));
            prompt_denials.push(json!({"tool_name": "Bash", "tool_use_id": id, "tool_input": input}));
        }
    }
    if step.sleep_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(step.sleep_ms));
    }
    if step.lock_index {
        // A worktree's `.git` is a file naming its git folder.
        let link = std::fs::read_to_string(cwd.join(".git")).unwrap_or_default();
        if let Some(dir) = link.trim().strip_prefix("gitdir: ") {
            let _ = std::fs::write(Path::new(dir.trim()).join("index.lock"), "");
        }
    }
    for f in &step.files {
        let p = cwd.join(&f.path);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&p, &f.text);
        let id = tool_id();
        let use_ = match &f.old {
            Some(old) => json!({"type": "tool_use", "id": id, "name": "Edit",
                "input": {"file_path": f.path, "old_string": old, "new_string": f.text}}),
            None => json!({"type": "tool_use", "id": id, "name": "Write",
                "input": {"file_path": f.path, "content": f.text}}),
        };
        emit(json!({"type": "assistant", "message": {"content": [use_]}}));
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": "written"}]}}));
    }
    for path in &step.delete {
        let _ = std::fs::remove_file(cwd.join(path));
        let id = tool_id();
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": format!("rm {path}")}}]}}));
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": ""}]}}));
    }
    if let Some(msg) = &step.commit {
        git(&cwd, &["add", "-A"]);
        let out = git(&cwd, &["commit", "-q", "-m", msg]);
        let id = tool_id();
        emit(json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": format!("git commit -m {msg:?}")}}]}}));
        emit(json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": out}]}}));
    }
    if let Some(code) = step.exit {
        return ExitCode::from(code as u8);
    }
    if step.linger_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(step.linger_ms));
    }
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    let output = step
        .output
        .as_ref()
        .and_then(|o| serde_json::to_value(o).ok())
        .map(|v| {
            substitute(
                v,
                &[
                    ("$HEAD", &head),
                    ("$SESSION", &session),
                    ("$ANSWER2", &answer2),
                    ("$ANSWER", &answer),
                    ("$APPROVED", &approved),
                ],
            )
        });
    let mut denials: Vec<Value> = step
        .denied
        .iter()
        .map(|c| json!({"tool_name": "Bash", "tool_input": {"command": c}}))
        .collect();
    denials.extend(prompt_denials);
    let mut result = json!({"type": "result", "subtype": "success", "is_error": step.is_error,
        "session_id": session, "total_cost_usd": step.cost, "num_turns": 1,
        "result": "done", "permission_denials": denials});
    if let Some(o) = output {
        result["structured_output"] = o;
    }
    emit(result);
    ExitCode::SUCCESS
}

/// The `sh` that runs `exec` commands. On Windows, `sh` is often not on the PATH (only Git's
/// `cmd` folder is), so it is found from Git's own install, as Claude Code finds Git Bash.
fn shell() -> PathBuf {
    if cfg!(windows) {
        let exec = git(Path::new("."), &["--exec-path"]);
        // e.g. C:/Program Files/Git/mingw64/libexec/git-core: Git's root is three folders up.
        if let Some(root) = Path::new(&exec).ancestors().nth(3) {
            for sh in ["bin/sh.exe", "usr/bin/sh.exe"] {
                let p = root.join(sh);
                if p.exists() {
                    return p;
                }
            }
        }
    }
    PathBuf::from("sh")
}

/// An exclusive lock file, removed when dropped. A lock older than 10 s is left over from an
/// agent that was killed, and is taken over.
struct Lock(std::path::PathBuf);

impl Lock {
    fn take(path: &std::path::Path) -> Lock {
        let start = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => return Lock(path.to_path_buf()),
                Err(_) => {
                    let stale = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|m| m.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(10));
                    if stale || start.elapsed() > std::time::Duration::from_secs(20) {
                        let _ = std::fs::remove_file(path);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
