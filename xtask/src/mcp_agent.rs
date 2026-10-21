//! The `mcp-agent` persona (003 T015): a scripted MCP client standing in for an agent CLI.
//!
//! Fallow starts it as a session with the "claude" orchestration style, so its argv ends with
//! `--mcp-config <json> --allowedTools mcp__fallow__*`. Like Claude Code, it starts the server
//! named in that JSON (`fallow mcp-bridge …`), runs `initialize`, then the steps of its script.
//! Results are printed to its terminal for the PTY harness to read.
//!
//! Script lines (`#` starts a comment):
//! - `call <tool> <json>`: one tool call; prints `ok <n>` or `fail <n> <why>`.
//! - `expect <text>`: the last result contains `text`, else `fail`.
//! - `sleep <ms>`
//! - `until <text>`: reads lines typed into the terminal until one contains `text`.
//! - `post-timed <count> <interval ms> <scope>`: posts `t=<epoch ms> i=<n>` messages.
//! - `measure-reads <count> <scope> [file]`: polls `board_read` until `count` timed messages
//!   arrived and prints (and writes to `file`) `latency p95_ms=… max_ms=… n=…`.
//! - `post-stamped <file> <text>`: posts `text` and writes the epoch milliseconds just before
//!   the call to `file`.
//! - `write <file>`: writes the last result (or verdict) to `file`.
//! - `wait-loop <name> <timeout_s|-> <file>`: calls `event_wait` again after each
//!   `still_waiting`, then writes `status=… repeats=… from=…` to `file`.
//! - `save-bridge <file>`: writes the bridge's command line (JSON array) to `file`.
//! - `bridge-from <file>`: runs the bridge saved in `file` and prints `accepted` or `refused`.
//! - `print <text>`
//!
//! After the script it prints `script done` and echoes every typed line as `got <line>`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next: u64,
}

impl Client {
    fn start(cmd: &[String]) -> Result<Client, String> {
        let (exe, args) = cmd.split_first().ok_or("empty server command")?;
        let mut child = Command::new(exe)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start the server: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Ok(Client {
            child,
            stdin,
            lines: rx,
            next: 1,
        })
    }

    fn send(&mut self, v: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{v}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("server closed: {e}"))
    }

    /// One request; waits for its response (skipping notifications).
    fn request(&mut self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .map_err(|_| format!("no answer to {method}"))?;
            let v: Value = serde_json::from_str(&line).map_err(|e| format!("bad line: {e}"))?;
            if v.get("id") == Some(&json!(id)) {
                if let Some(e) = v.get("error") {
                    return Err(format!("error {e}"));
                }
                return Ok(v["result"].clone());
            }
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "mcp-agent", "version": "1"}}),
            Duration::from_secs(10),
        )?;
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
    }

    /// A tool call; the structured result, or the tool error as `Err`.
    fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        let r = self.request(
            "tools/call",
            json!({"name": tool, "arguments": args}),
            Duration::from_secs(70),
        )?;
        let body = r["structuredContent"].clone();
        if r["isError"] == true {
            Err(body.to_string())
        } else {
            Ok(body)
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// The server command from `--mcp-config <json>` in our own arguments.
fn server_command(args: &[String]) -> Result<Vec<String>, String> {
    let i = args
        .iter()
        .position(|a| a == "--mcp-config")
        .ok_or("started without --mcp-config (orchestration off for this session?)")?;
    let cfg: Value = serde_json::from_str(args.get(i + 1).ok_or("no config")?)
        .map_err(|e| format!("bad --mcp-config: {e}"))?;
    let s = &cfg["mcpServers"]["fallow"];
    let mut cmd = vec![s["command"].as_str().ok_or("no command")?.to_string()];
    for a in s["args"].as_array().ok_or("no args")? {
        cmd.push(a.as_str().ok_or("bad arg")?.to_string());
    }
    Ok(cmd)
}

/// Runs the persona: `mcp-agent <script> [--mcp-config <json> …]`. Never returns.
pub fn run(args: &[String]) -> ! {
    let out = |s: &str| {
        println!("{s}");
        let _ = std::io::stdout().flush();
    };
    let script = args
        .first()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let typed = {
        let (tx, rx) = channel::<String>();
        std::thread::spawn(move || {
            for l in std::io::stdin().lock().lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        rx
    };
    let mut client = match server_command(args).and_then(|c| {
        let mut cl = Client::start(&c)?;
        cl.initialize()?;
        Ok((cl, c))
    }) {
        Ok(c) => Some(c),
        Err(e) => {
            out(&format!("fail connect {e}"));
            None
        }
    };
    let mut last = String::new();
    for (n, line) in script.lines().enumerate() {
        let n = n + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        match (cmd, &mut client) {
            ("print", _) => out(rest),
            ("sleep", _) => std::thread::sleep(Duration::from_millis(rest.parse().unwrap_or(0))),
            ("until", _) => loop {
                match typed.recv() {
                    Ok(l) if l.contains(rest) => {
                        out(&format!("got {rest}"));
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            },
            ("wait-loop", Some((c, _))) => {
                let mut p = rest.split_whitespace();
                let name = p.next().unwrap_or("x");
                let timeout: Option<u64> = p.next().and_then(|t| t.parse().ok());
                let file = p.next().unwrap_or("wait.out");
                let mut a = json!({"name": name});
                if let Some(t) = timeout {
                    a["timeout_s"] = json!(t);
                }
                let mut repeats = 0;
                let line = loop {
                    match c.call("event_wait", a.clone()) {
                        Ok(v) if v["status"] == "still_waiting" => repeats += 1,
                        Ok(v) => {
                            break format!(
                                "status={} repeats={repeats} from={}",
                                v["status"].as_str().unwrap_or("?"),
                                v["signal"]["from"]["session"]
                            );
                        }
                        Err(e) => break format!("status=error {e}"),
                    }
                };
                let _ = std::fs::write(file, &line);
                out(&line);
            }
            ("write", _) => {
                let _ = std::fs::write(rest, &last);
                out(&format!("ok {n}"));
            }
            ("post-stamped", Some((c, _))) => {
                let (file, text) = rest.split_once(' ').unwrap_or((rest, "stamped"));
                let t = now_ms();
                match c.call("board_post", json!({"text": text})) {
                    Ok(_) => {
                        let _ = std::fs::write(file, t.to_string());
                        out(&format!("ok {n}"));
                    }
                    Err(e) => out(&format!("fail {n} {e}")),
                }
            }
            ("expect", _) => {
                if last.contains(rest) {
                    out(&format!("ok {n}"));
                } else {
                    out(&format!("fail {n} missing {rest}"));
                }
            }
            ("call", Some((c, _))) => {
                let (tool, json_args) = rest.split_once(' ').unwrap_or((rest, "{}"));
                let a: Value = serde_json::from_str(json_args).unwrap_or(json!({}));
                match c.call(tool, a) {
                    Ok(v) => {
                        last = v.to_string();
                        out(&format!("ok {n}"));
                    }
                    Err(e) => {
                        last = e.clone();
                        out(&format!(
                            "fail {n} {}",
                            e.chars().take(60).collect::<String>()
                        ));
                    }
                }
            }
            ("post-timed", Some((c, _))) => {
                let mut p = rest.split_whitespace();
                let count: u32 = p.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                let every: u64 = p.next().and_then(|v| v.parse().ok()).unwrap_or(100);
                let scope = p.next().unwrap_or("project");
                let mut failed = 0;
                for i in 0..count {
                    let text = format!("t={} i={i}", now_ms());
                    if c.call("board_post", json!({"text": text, "scope": scope}))
                        .is_err()
                    {
                        failed += 1;
                    }
                    std::thread::sleep(Duration::from_millis(every));
                }
                out(&format!("posted {} failed {failed}", count - failed));
            }
            ("measure-reads", Some((c, _))) => {
                let mut p = rest.split_whitespace();
                let count: usize = p.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                let scope = p.next().unwrap_or("project");
                let file = p.next();
                let mut since = 0u64;
                let mut lat = Vec::new();
                let until = Instant::now() + Duration::from_secs(120);
                while lat.len() < count && Instant::now() < until {
                    if let Ok(v) = c.call(
                        "board_read",
                        json!({"scope": scope, "since_id": since, "limit": 100}),
                    ) {
                        let seen = now_ms();
                        for m in v["messages"].as_array().into_iter().flatten() {
                            since = since.max(m["id"].as_u64().unwrap_or(0));
                            let t = m["text"]
                                .as_str()
                                .and_then(|s| s.strip_prefix("t="))
                                .and_then(|s| s.split(' ').next())
                                .and_then(|s| s.parse::<u128>().ok());
                            if let Some(t) = t {
                                lat.push(seen.saturating_sub(t) as f64);
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                lat.sort_by(f64::total_cmp);
                let p95 = lat
                    .get(((lat.len() as f64 * 0.95).ceil() as usize).saturating_sub(1))
                    .copied()
                    .unwrap_or(f64::NAN);
                let max = lat.last().copied().unwrap_or(f64::NAN);
                let line = format!("latency p95_ms={p95:.0} max_ms={max:.0} n={}", lat.len());
                if let Some(f) = file {
                    let _ = std::fs::write(f, &line);
                }
                out(&line);
            }
            ("save-bridge", Some((_, cmd))) => {
                let _ = std::fs::write(rest, serde_json::to_string(cmd).unwrap_or_default());
                out(&format!("ok {n}"));
            }
            ("bridge-from", _) => {
                let cmd: Vec<String> = std::fs::read_to_string(rest)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default();
                let verdict = Client::start(&cmd)
                    .and_then(|mut c| c.initialize())
                    .map_or("refused", |()| "accepted");
                last = verdict.to_string();
                out(verdict);
            }
            (_, None) => out(&format!("fail {n} not connected")),
            _ => out(&format!("fail {n} unknown step {cmd}")),
        }
    }
    out("script done");
    loop {
        match typed.recv() {
            Ok(l) => out(&format!("got {l}")),
            Err(_) => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
        }
    }
}
