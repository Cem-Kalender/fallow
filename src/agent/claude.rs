//! The Claude Code runtime: `claude -p` without a terminal (006 research R-3, R-4; 007
//! research S-5, S-7, verified 2026-10-30).

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde_json::{Value, json};

use super::rights::{Edits, Rights};
use super::{
    Agent, Availability, Call, Capabilities, Event, Outcome, Runtime, Session, ToolKind, clip,
};

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

/// Runs `cmd` for its output, killing it after 4 s (a `--version` that hangs on an update
/// prompt must not hold Fallow).
fn probe(cmd: &mut std::process::Command) -> Option<std::process::Output> {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if std::time::Instant::now() < end => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

impl Runtime for Claude {
    fn name(&self) -> &str {
        "claude"
    }

    /// 012 (FR-010a): checked on the installed `claude` by T001 (specs/012-solo-session/checks/
    /// claude-check.md): images in a stream-json message, web tools off by `--disallowedTools`,
    /// and the settings files it would read from a working copy.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
            web_off: true,
            control_files: CONTROL_FILES.iter().map(|f| f.to_string()).collect(),
        }
    }

    fn available(&self) -> Availability {
        let Some(program) = crate::platform::program_to_run(&self.agent.command) else {
            return Availability {
                installed: false,
                version: None,
            };
        };
        let version = probe(
            crate::platform::command(program)
                .args(&self.agent.args)
                .arg("--version"),
        )
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        });
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
    // Fallow's tools by name: an agent may call `ask` and `progress`; `approve` is Claude Code's
    // permission prompt, never the agent's to call (Phase 3 review).
    let mut allow: Vec<String> = [
        "Read",
        "Grep",
        "Glob",
        "mcp__fallow__ask",
        "mcp__fallow__progress",
    ]
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

/// 012: the tools a whole-tool ask rule sends to the permission prompt for the assistant
/// (research R-6; T001 found no `MultiEdit` on 2.1.318, kept for older versions).
const ASKED: [&str; 6] = ["Bash", "PowerShell", "Edit", "Write", "MultiEdit", "NotebookEdit"];

/// 012: the files in a working copy through which Claude Code reads settings of its own.
const CONTROL_FILES: [&str; 2] = [".claude/settings.json", ".claude/settings.local.json"];

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
    let (mut allow, mut deny, mode) = rules(&c.rights);
    // 012 (research R-8): the web tools are the assistant's, on its switch; off for everyone
    // else.
    let web = ["WebFetch".to_string(), "WebSearch".to_string()];
    if c.web {
        allow.extend(web);
    } else {
        deny.extend(web);
    }
    if !c.attachments.is_empty() {
        // The instruction then goes as one stream-json user message with its images (T001).
        a.push("--input-format".into());
        a.push("stream-json".into());
    }
    // 012 (FR-042, research R-6, T001): no settings from the working copy, ever; none of the
    // runtime's own for unattended kinds; for the assistant the engineer's user settings, with
    // every command and edit still sent to Fallow by whole-tool ask rules, and no hook.
    a.push("--setting-sources".into());
    a.push(if c.user_settings { "user" } else { "" }.into());
    a.push("--settings".into());
    a.push(
        if c.user_settings {
            json!({"permissions": {"ask": ASKED}, "disableAllHooks": true})
        } else {
            json!({"disableAllHooks": true})
        }
        .to_string(),
    );
    // The fixed tool list (the engineer's decision of 2026-11-05): no other built-in tool.
    let mut tools = vec!["Read", "Glob", "Grep", "Bash", "PowerShell", "Task"];
    if !matches!(c.rights.edits, Edits::None) {
        tools.extend(["Edit", "Write", "NotebookEdit"]);
    }
    if c.web {
        tools.extend(["WebSearch", "WebFetch"]);
    }
    a.push("--tools".into());
    a.push(tools.join(","));
    // Its own settings files are never an agent's to edit (Capabilities.control_files).
    for f in CONTROL_FILES {
        push(&mut deny, [format!("Edit({f})"), format!("Write({f})")]);
    }
    a.push("--permission-mode".into());
    a.push(mode.into());
    a.push("--allowedTools".into());
    a.extend(allow);
    a.push("--disallowedTools".into());
    a.extend(deny);
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
    // 012 (FR-042): always a model, since the user's default no longer applies to every kind.
    a.push("--model".into());
    a.push(
        c.model
            .as_ref()
            .filter(|m| !m.is_empty())
            .cloned()
            .unwrap_or_else(|| super::registry::models("claude")[0].to_string()),
    );
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

/// 012: what goes on stdin: the instruction, or with attachments one stream-json user message
/// holding the text and each image as a base64 block (checked by T001). An image that cannot be
/// read is named in the text instead.
pub fn stdin_text(c: &Call) -> String {
    if c.attachments.is_empty() {
        return c.instruction.clone();
    }
    let mut content = vec![json!({"type": "text", "text": c.instruction})];
    for a in &c.attachments {
        match std::fs::read(&a.path) {
            Ok(bytes) => content.push(json!({"type": "image", "source": {
                "type": "base64", "media_type": a.media_type,
                "data": base64(&bytes)}})),
            Err(e) => content.push(json!({"type": "text",
                "text": format!("(an attached image could not be read: {e})")})),
        }
    }
    format!(
        "{}\n",
        json!({"type": "user", "message": {"role": "user", "content": content}})
    )
}

/// Standard base64 with padding (RFC 4648), for an image block.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= c.len() {
                out.push(T[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// 012 (contracts/runtime.md, "Permission requests"): Claude Code's permission prompt
/// (`{tool_name, input, tool_use_id}`) in the contract's terms.
pub fn permission_request(raw: &Value) -> super::PermissionRequest {
    let tool = raw["tool_name"].as_str().unwrap_or_default().to_string();
    let input = &raw["input"];
    let s = |k: &str| input[k].as_str().map(str::to_string);
    let mut r = super::PermissionRequest {
        kind: super::RequestKind::Other,
        command: None,
        path: None,
        url: None,
        query: None,
        tool: tool.clone(),
    };
    match tool.as_str() {
        "Bash" | "PowerShell" => {
            r.kind = super::RequestKind::Run;
            r.command = s("command");
        }
        "Edit" | "Write" | "MultiEdit" => {
            r.kind = super::RequestKind::Edit;
            r.path = s("file_path");
        }
        "NotebookEdit" => {
            r.kind = super::RequestKind::Edit;
            r.path = s("notebook_path");
        }
        "WebFetch" => {
            r.kind = super::RequestKind::Web;
            r.url = s("url");
        }
        "WebSearch" => {
            r.kind = super::RequestKind::Web;
            r.query = s("query");
        }
        _ => {}
    }
    // A request whose input does not have the field it needs is one Fallow cannot judge.
    let complete = match r.kind {
        super::RequestKind::Run => r.command.is_some(),
        super::RequestKind::Edit => r.path.is_some(),
        super::RequestKind::Web => r.url.is_some() || r.query.is_some(),
        super::RequestKind::Other => true,
    };
    if !complete {
        r.kind = super::RequestKind::Other;
    }
    r
}

/// 012: the permission prompt's answer, as Claude Code reads it.
pub fn permission_reply(allow: bool, message: &str, raw: &Value) -> Value {
    if allow {
        json!({ "behavior": "allow", "updatedInput": raw["input"] })
    } else {
        json!({ "behavior": "deny", "message": message })
    }
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
        // 012 (research R-8): web requests made as tool calls.
        "WebFetch" => (
            ToolKind::Web,
            format!("Fetched {}", input["url"].as_str().unwrap_or_default()),
            input["prompt"].as_str().unwrap_or_default().to_string(),
        ),
        "WebSearch" => (
            ToolKind::Web,
            format!("Searched {}", input["query"].as_str().unwrap_or_default()),
            String::new(),
        ),
        "mcp__fallow__ask" => (
            ToolKind::Ask,
            input["question"].as_str().unwrap_or_default().to_string(),
            input["options"]
                .as_array()
                .map(|o| {
                    o.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .unwrap_or_default(),
        ),
        "mcp__fallow__progress" => (
            ToolKind::Progress,
            input["text"].as_str().unwrap_or_default().to_string(),
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

/// A tool call's path as the agent's own folder sees it: `shop/cart.py`, not the worktree's
/// absolute path (T086: the page listed both, and counted them twice).
fn relative(e: Event, cwd: &Path) -> Event {
    // Only a file's path: a command's text is shown as the agent ran it.
    let Event::ToolCall {
        id,
        kind: kind @ (ToolKind::Read | ToolKind::Edit | ToolKind::Write),
        summary,
        detail,
    } = e
    else {
        return e;
    };
    let norm = |s: &str| s.replace('\\', "/");
    let base = norm(&cwd.display().to_string());
    let base = base.trim_end_matches('/');
    let s = norm(&summary);
    // Windows paths ignore case; elsewhere they do not.
    let same = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let summary = match s.get(..base.len()) {
        Some(head) if !base.is_empty() && same(head, base) && s[base.len()..].starts_with('/') => {
            s[base.len() + 1..].to_string()
        }
        _ => summary,
    };
    Event::ToolCall {
        id,
        kind,
        summary,
        detail,
    }
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

/// How often, and how long apart, a program that cannot be started is tried again.
const START_TRIES: u32 = 3;
#[cfg(not(test))]
const START_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)]
const START_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// Runs a call to its end or its time limit.
pub fn run(agent: &Agent, c: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
    let mut started = Instant::now();
    let fail = |why: String, started: Instant| Outcome {
        result: None,
        failure: Some(why),
        cost_usd: None,
        session: None,
        denials: Vec::new(),
        duration: started.elapsed(),
    };
    let group = match &c.group {
        Some(g) => std::sync::Arc::clone(g),
        None => match crate::platform::job::Group::new() {
            Ok(g) => std::sync::Arc::new(g),
            Err(e) => return fail(format!("cannot create a process group: {e}"), started),
        },
    };
    // The agent's program can be missing for a moment while it updates itself (T086: three
    // checks were used up in seconds): it is looked up and started again, a few times, before
    // the step fails.
    let mut tries = 0;
    let mut child = loop {
        // Ended while it waited: no agent is started after that, also if its program is back.
        if stop.load(Ordering::Relaxed) {
            return fail(
                format!(
                    "cannot start {}: the step was ended before its program ran",
                    agent.command
                ),
                started,
            );
        }
        let program = crate::platform::program_to_run(&agent.command)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| agent.command.clone());
        let mut cmd = Command::new(&program);
        cmd.args(&agent.args)
            .args(args(c))
            .current_dir(&c.cwd)
            // 012 (review M1): each shell command starts in the working copy again, so a `cd`
            // in one command never carries over to the next one Fallow judges.
            .env("CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window for the service's children.
            cmd.creation_flags(0x0800_0000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own process group, whose id is its pid: what `Group` stops, now and after a
            // restart (R-19; sixth review, 1: it was left in Fallow's group, so nothing was).
            cmd.process_group(0);
        }
        match cmd.spawn() {
            Ok(ch) => break ch,
            // Only a program not found (its folder there): any other error will not go away.
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && c.cwd.is_dir()
                    && tries < START_TRIES
                    && !stop.load(Ordering::Relaxed) =>
            {
                tries += 1;
                let until = Instant::now() + START_WAIT;
                while Instant::now() < until && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100).min(START_WAIT));
                }
            }
            Err(e) => return fail(format!("cannot start {}: {e}", agent.command), started),
        }
    };
    // The step's time limit counts from the agent's start, not from the waits before it.
    started = Instant::now();
    let _ = group.adopt(child.id());
    sink(Event::Launched);
    if let Some(mut stdin) = child.stdin.take() {
        let text = stdin_text(c);
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let stderr = child.stderr.take();
    let err_tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut err_reader = None;
    if let Some(e) = stderr {
        let tail = std::sync::Arc::clone(&err_tail);
        err_reader = Some(std::thread::spawn(move || {
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
        }));
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
                    sink(relative(e, &c.cwd));
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
            let (result, mut failure, cost_usd, session, denials) = result_of(&v);
            // An error with no message says why on stderr only, for example "No conversation
            // found with session ID" for a session that never started (T086).
            if failure
                .as_deref()
                .is_some_and(|f| f.ends_with("no message"))
            {
                if let Some(r) = err_reader.take() {
                    // A grandchild may hold the pipe open: wait briefly, never for long.
                    let until = Instant::now() + std::time::Duration::from_millis(500);
                    while !r.is_finished() && Instant::now() < until {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
                let tail = err_tail
                    .lock()
                    .map(|t| t.trim().to_string())
                    .unwrap_or_default();
                if let Some(line) = tail.lines().rev().find(|l| !l.trim().is_empty()) {
                    failure = Some(format!(
                        "the agent ended with an error: {}",
                        clip(line.trim(), 300)
                    ));
                }
            }
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

    #[test]
    fn a_program_not_found_is_tried_again_before_the_step_fails() {
        let agent = Agent {
            command: "fallow-no-such-agent-program".into(),
            args: Vec::new(),
        };
        let c = Call {
            instruction: "x".into(),
            schema: None,
            rights: rights::lead(Mode::ReadOnly, Vec::new()),
            mcp_config: None,
            session: Session::New("s".into()),
            model: None,
            budget_usd: None,
            cwd: std::env::temp_dir(),
            time_limit: std::time::Duration::from_secs(5),
            system: None,
            partial: false,
            group: None,
            attachments: Vec::new(),
            web: false,
            user_settings: false,
        };
        let t = Instant::now();
        let o = run(&agent, &c, &mut |_| {}, &AtomicBool::new(false));
        assert!(o.failure.unwrap().starts_with("cannot start"));
        assert!(t.elapsed() >= START_WAIT * START_TRIES, "tried again");
        // Stopped: no waiting.
        let t = Instant::now();
        let _ = run(&agent, &c, &mut |_| {}, &AtomicBool::new(true));
        assert!(t.elapsed() < START_WAIT, "a stop ends the waiting");
    }

    #[test]
    fn paths_in_the_agents_folder_are_shown_relative_to_it() {
        let read = |p: &str| Event::ToolCall {
            id: "t".into(),
            kind: ToolKind::Read,
            summary: p.into(),
            detail: String::new(),
        };
        let wd = Path::new(r"C:\Users\x\AppData\Local\fallow\runs\1\wt\build");
        let summary = |e: Event| match e {
            Event::ToolCall { summary, .. } => summary,
            _ => unreachable!(),
        };
        assert_eq!(
            summary(relative(
                read(r"C:\Users\x\AppData\Local\fallow\runs\1\wt\build\shop\cart.py"),
                wd
            )),
            "shop/cart.py"
        );
        if cfg!(windows) {
            assert_eq!(
                summary(relative(
                    read("c:/users/x/appdata/local/fallow/runs/1/wt/build/README.md"),
                    wd
                )),
                "README.md",
                "case and slashes do not matter on Windows"
            );
        }
        // A command is shown as the agent ran it, backslashes and all.
        let run = r#"C:\Users\x\AppData\Local\fallow\runs\1\wt\build\.venv\python.exe -c "print('a\nb')""#;
        let e = relative(
            Event::ToolCall {
                id: "r".into(),
                kind: ToolKind::Run,
                summary: run.into(),
                detail: String::new(),
            },
            wd,
        );
        assert_eq!(summary(e), run);
        // Outside it, or a sibling folder sharing its prefix: kept as given.
        assert_eq!(
            summary(relative(
                read(r"C:\Users\x\AppData\Local\fallow\runs\1\wt\build2\a.py"),
                wd
            )),
            r"C:\Users\x\AppData\Local\fallow\runs\1\wt\build2\a.py"
        );
        assert_eq!(summary(relative(read("shop/x.py"), wd)), "shop/x.py");
    }
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
            attachments: Vec::new(),
            web: false,
            user_settings: false,
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
            "mcp__fallow__ask",
        ] {
            assert!(has(&a, r), "lacks {r}");
        }
        assert!(
            !a.iter()
                .any(|x| x.contains("approve") && !x.starts_with("--")),
            "approve is never allowed"
        );
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

    #[test]
    fn web_tools_follow_the_calls_switch() {
        let pos = |a: &[String], flag: &str| a.iter().position(|x| x == flag).unwrap();
        let off = args(&call());
        let d = pos(&off, "--disallowedTools");
        assert!(off[d + 1..].iter().take_while(|x| !x.starts_with("--")).any(|x| x == "WebFetch"));
        let mut c = call();
        c.web = true;
        let on = args(&c);
        let al = pos(&on, "--allowedTools");
        let allowed: Vec<&String> = on[al + 1..].iter().take_while(|x| !x.starts_with("--")).collect();
        assert!(allowed.iter().any(|x| *x == "WebFetch") && allowed.iter().any(|x| *x == "WebSearch"));
        assert!(!on.contains(&"--input-format".to_string()));
    }

    #[test]
    fn attachments_go_as_one_stream_json_message() {
        let dir = std::env::temp_dir().join(format!("fallow-att-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("a.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let mut c = call();
        c.instruction = "Fallow step: assistant_turn
look".into();
        c.attachments = vec![super::super::Attachment {
            path: img,
            media_type: "image/png".into(),
        }];
        let a = args(&c);
        let i = a.iter().position(|x| x == "--input-format").unwrap();
        assert_eq!(a[i + 1], "stream-json");
        let text = stdin_text(&c);
        let v: Value = serde_json::from_str(text.trim()).unwrap();
        let content = v["message"]["content"].as_array().unwrap();
        assert_eq!(content[0]["text"], "Fallow step: assistant_turn
look");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], base64(b"\x89PNG"));
        // Without attachments the instruction goes as it is.
        assert_eq!(stdin_text(&call()), "x");
    }

    #[test]
    fn base64_is_rfc_4648() {
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn permission_prompts_map_to_the_contracts_requests() {
        use super::super::RequestKind as K;
        let r = |tool: &str, input: Value| permission_request(&json!({"tool_name": tool, "input": input, "tool_use_id": "t"}));
        let x = r("Bash", json!({"command": "npm test"}));
        assert_eq!((x.kind, x.command.as_deref()), (K::Run, Some("npm test")));
        assert_eq!(r("PowerShell", json!({"command": "ls"})).kind, K::Run);
        for t in ["Edit", "Write", "MultiEdit"] {
            let x = r(t, json!({"file_path": "a.txt"}));
            assert_eq!((x.kind, x.path.as_deref()), (K::Edit, Some("a.txt")), "{t}");
        }
        let x = r("NotebookEdit", json!({"notebook_path": "n.ipynb"}));
        assert_eq!((x.kind, x.path.as_deref()), (K::Edit, Some("n.ipynb")));
        let x = r("WebFetch", json!({"url": "https://example.com"}));
        assert_eq!((x.kind, x.url.as_deref()), (K::Web, Some("https://example.com")));
        let x = r("WebSearch", json!({"query": "rust"}));
        assert_eq!((x.kind, x.query.as_deref()), (K::Web, Some("rust")));
        // A tool Fallow does not know, or an input without the field it needs, is Other.
        assert_eq!(r("CronCreate", json!({})).kind, K::Other);
        assert_eq!(r("Bash", json!({})).kind, K::Other);
        assert_eq!(r("Edit", json!({"path": "a"})).kind, K::Other);
        let raw = json!({"tool_name": "Bash", "input": {"command": "x"}});
        assert_eq!(permission_reply(true, "", &raw), json!({"behavior": "allow", "updatedInput": {"command": "x"}}));
        assert_eq!(permission_reply(false, "no", &raw), json!({"behavior": "deny", "message": "no"}));
    }

    #[test]
    fn web_tool_uses_are_web_activity() {
        let e = tool_call(&json!({"id": "w", "name": "WebFetch", "input": {"url": "https://example.com", "prompt": "title"}}));
        match e {
            Event::ToolCall { kind, summary, .. } => {
                assert_eq!(kind, ToolKind::Web);
                assert_eq!(summary, "Fetched https://example.com");
            }
            _ => unreachable!(),
        }
        match tool_call(&json!({"id": "s", "name": "WebSearch", "input": {"query": "rust"}})) {
            Event::ToolCall { kind, summary, .. } => assert_eq!((kind, summary.as_str()), (ToolKind::Web, "Searched rust")),
            _ => unreachable!(),
        }
    }

    #[test]
    fn settings_and_tools_follow_the_kind() {
        let after = |a: &[String], flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        // An unattended kind: no settings source, no hooks, a model always, no web, no edits
        // for a checker.
        let mut c = call();
        c.rights = rights::checker(Vec::new());
        c.model = None;
        let a = args(&c);
        assert_eq!(after(&a, "--setting-sources"), "");
        assert_eq!(after(&a, "--settings"), r#"{"disableAllHooks":true}"#);
        assert_eq!(after(&a, "--tools"), "Read,Glob,Grep,Bash,PowerShell,Task");
        assert_eq!(after(&a, "--model"), "opus");
        // The assistant: the user's settings, every command and edit asked of Fallow.
        let mut c = call();
        c.rights = rights::maker(Vec::new(), Vec::new());
        c.user_settings = true;
        c.web = true;
        c.model = Some("sonnet".into());
        let a = args(&c);
        assert_eq!(after(&a, "--setting-sources"), "user");
        let s: Value = serde_json::from_str(&after(&a, "--settings")).unwrap();
        assert_eq!(s["disableAllHooks"], true);
        assert_eq!(s["permissions"]["ask"], json!(ASKED));
        assert_eq!(
            after(&a, "--tools"),
            "Read,Glob,Grep,Bash,PowerShell,Task,Edit,Write,NotebookEdit,WebSearch,WebFetch"
        );
        assert_eq!(after(&a, "--model"), "sonnet");
        // Claude Code's own settings files are never editable.
        let d = a.iter().position(|x| x == "--disallowedTools").unwrap();
        assert!(a[d + 1..].iter().any(|x| x == "Write(.claude/settings.local.json)"));
    }
}
