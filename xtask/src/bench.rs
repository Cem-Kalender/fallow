//! `cargo xtask bench` (007 T083; constitution VIII, "Budgets & Measurement").
//!
//! Three budgets, measured on Fallow's own process (`fallow serve`), never its agents:
//! - page ready: process launch to the page answering, p95 of 30 launches after 3 warm-ups,
//!   under 500 ms;
//! - idle memory: settled, with no loop running, under 60 MB;
//! - running memory: four loops whose checkpoints run 4 checker steps at once (the scripted
//!   fake agent), under 200 MB. (A loop runs one step at a time: checkers marked
//!   `parallel_with` still run one after the other, so the four steps come from four loops.)
//!
//! Each run uses its own config and data folders, so the engineer's are never touched; it needs
//! no other Fallow service running for the same user (there is one per user). Memory is the
//! working set on Windows (`tasklist`) and VmRSS on Linux. A failing budget is measured once
//! more; the second result is final (shared runners are noisy).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const WARMUPS: usize = 3;
const RUNS: usize = 30;
const READY_MS: f64 = 500.0;
const IDLE_MB: f64 = 60.0;
const RUNNING_MB: f64 = 200.0;

struct Args {
    bin: PathBuf,
    fake: PathBuf,
}

fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug")
}

fn parse(args: &[String]) -> Result<Args, String> {
    let exe = |n: &str| target_dir().join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let mut a = Args {
        bin: exe("fallow"),
        fake: exe("fallow-fake-agent"),
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--bin" => a.bin = it.next().ok_or("--bin needs a path")?.into(),
            "--fake" => a.fake = it.next().ok_or("--fake needs a path")?.into(),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    for p in [&a.bin, &a.fake] {
        if !p.is_file() {
            return Err(format!(
                "{} is missing: build it first (cargo build)",
                p.display()
            ));
        }
    }
    Ok(a)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let args = parse(args).map_err(|e| {
        format!(
            "bench: {e}\nusage: cargo xtask bench [--bin <fallow>] [--fake <fallow-fake-agent>]"
        )
    })?;
    let mut failed = Vec::new();
    for (name, f) in [
        (
            "page ready",
            ready as fn(&Args) -> Result<(f64, f64), String>,
        ),
        ("idle memory", idle),
        ("running memory", running),
    ] {
        let (v, budget) = f(&args)?;
        let unit = if name == "page ready" { "ms" } else { "MB" };
        if v < budget {
            println!("bench: {name}: {v:.1} {unit} (budget {budget} {unit}): ok");
            continue;
        }
        // Once more; the second result is final.
        let (v2, _) = f(&args)?;
        let ok = v2 < budget;
        println!(
            "bench: {name}: {v:.1} {unit}, then {v2:.1} {unit} (budget {budget} {unit}): {}",
            if ok { "ok" } else { "OVER" }
        );
        if !ok {
            failed.push(name);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("over budget: {}", failed.join(", ")))
    }
}

// ---- A Fallow service of its own ------------------------------------------------------------

struct Service {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-bench-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("config")).unwrap();
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

/// Starts `fallow serve`; returns when it says its port, and the time that took.
fn start(args: &Args, dir: &Path) -> Result<(Service, Instant), String> {
    let t0 = Instant::now();
    let mut child = Command::new(&args.bin)
        .args(["serve", "--config-dir"])
        .arg(dir.join("config"))
        .arg("--data-dir")
        .arg(dir.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", args.bin.display()))?;
    let err = child.stderr.take().ok_or("no stderr")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if let Some(rest) = line.split("http://127.0.0.1:").nth(1) {
                let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let _ = tx.send(Ok(port.parse::<u16>().unwrap_or(0)));
            } else if line.contains("already running") {
                let _ = tx.send(Err(line));
            }
        }
    });
    let port = rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| "Fallow did not say its port".to_string())??;
    Ok((
        Service {
            child,
            port,
            dir: dir.to_path_buf(),
        },
        t0,
    ))
}

/// One HTTP request; returns the status, the headers and the body.
fn http(
    port: u16,
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<&Value>,
) -> Result<(u16, String, String), String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let origin = format!("http://127.0.0.1:{port}");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let (head, rest) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, head.to_string(), rest.to_string()))
}

fn rss_mb(pid: u32) -> Result<f64, String> {
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "fallow.exe","1234","Console","1","12,345 K"
        let field = text
            .split("\",\"")
            .nth(4)
            .ok_or_else(|| format!("tasklist: {text}"))?;
        let kb: String = field.chars().filter(char::is_ascii_digit).collect();
        Ok(kb.parse::<f64>().map_err(|e| e.to_string())? / 1024.0)
    }
    #[cfg(not(windows))]
    {
        let s =
            std::fs::read_to_string(format!("/proc/{pid}/status")).map_err(|e| e.to_string())?;
        let kb = s
            .lines()
            .find(|l| l.starts_with("VmRSS:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .ok_or("no VmRSS")?;
        Ok(kb / 1024.0)
    }
}

// ---- Page ready -----------------------------------------------------------------------------

fn ready(args: &Args) -> Result<(f64, f64), String> {
    let mut times = Vec::new();
    for i in 0..WARMUPS + RUNS {
        let dir = scratch(&format!("ready-{i}"));
        let (svc, t0) = start(args, &dir)?;
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            // The page answers: any HTTP answer (without the secret it refuses, which is an
            // answer too).
            if http(svc.port, "GET", "/", "", None).is_ok_and(|(code, _, _)| code > 0) {
                break;
            }
            if Instant::now() > end {
                return Err("the page did not answer within 10 s".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if i >= WARMUPS {
            times.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let p95 = times[(times.len() * 95).div_ceil(100) - 1];
    Ok((p95, READY_MS))
}

// ---- Memory ---------------------------------------------------------------------------------

fn idle(args: &Args) -> Result<(f64, f64), String> {
    let dir = scratch("idle");
    let (svc, _) = start(args, &dir)?;
    std::thread::sleep(Duration::from_secs(5));
    Ok((rss_mb(svc.child.id())?, IDLE_MB))
}

const SPEC: &str = "# Feature Specification: Calc\n\n## User Scenarios & Testing\n\n### User Story 1 - Add (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** two numbers, **When** adding 2 and 3, **Then** the result is 5.\n2. **Given** a string, **When** adding 'a' and 1, **Then** a TypeError is raised (must fail)\n\n### User Story 2 - Subtract (Priority: P2)\n\n**Acceptance Scenarios**:\n\n1. **Given** two numbers, **When** subtracting 3 from 5, **Then** the result is 2.\n2. **Given** a string, **When** subtracting 1 from 'a', **Then** a TypeError is raised (must fail)\n";
const PLAN: &str = "# Plan\n\n## Commands\n\n- `python -m calc`\n";
const TASKS: &str = "# Tasks: Calc\n\n## Phase 1: User Story 1 - Add\n\n- [ ] T001 [US1] add() in calc/__init__.py\n\n## Phase 2: User Story 2 - Subtract\n\n- [ ] T002 [US2] sub() in calc/__init__.py\n";

fn lit(s: &str) -> String {
    format!("'''\n{s}'''")
}

/// The fake agent's script: the lead's turns for four features, each loop's first task and
/// handover, and a long check at each loop's first checkpoint.
fn script() -> String {
    let lead = format!(
        "[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/001-calc/spec.md\", text = {} }}, {{ path = \"specs/001-calc/plan.md\", text = {} }}, {{ path = \"specs/001-calc/tasks.md\", text = {} }}]\noutput = {{ reply = \"Written.\", step = \"tasks\" }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed.\", step = \"analyze\", findings = [] }}\n\n",
        lit(SPEC),
        lit(PLAN),
        lit(TASKS)
    );
    let build = "[[step]]\nkind = \"build\"\npart = \"T001\"\nfiles = [{ path = \"calc/__init__.py\", text = '''\ndef add(a, b):\n    return a + b\n''' }]\noutput = { status = \"done\", task = \"T001\", files = [\"calc/__init__.py\"], summary = \"did T001\" }\n\n";
    let handover = "[[step]]\nkind = \"handover\"\npart = \"phase-1\"\nfiles = [{ path = \"specs/001-calc/handover/phase-1.md\", text = '''\n## Built\nPhase 1.\n\n## Decisions\nNone.\n\n## For the next phase\nGo on.\n''' }]\noutput = { status = \"done\", path = \"specs/001-calc/handover/phase-1.md\", summary = \"handed over\" }\n\n";
    let check = "[[step]]\nkind = \"review\"\npart = \"US1\"\nsleep_ms = 600000\noutput = { answers = [{ criterion = \"US1/1\", ran = \"python -c 1\", result = \"5\", passed = true }, { criterion = \"US1/2\", ran = \"python -c 1\", result = \"TypeError\", passed = true, failing_case = \"refused\" }], summary = \"checked\" }\n\n";
    [
        lead.repeat(LOOPS),
        build.repeat(LOOPS),
        handover.repeat(LOOPS),
        check.repeat(LOOPS),
    ]
    .concat()
}

const LOOPS: usize = 4;

const PIPELINE: &str = "name = \"bench\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n";

fn git(dir: &Path, a: &[&str]) -> Result<(), String> {
    let s = Command::new("git")
        .current_dir(dir)
        .args(a)
        .output()
        .map_err(|e| e.to_string())?;
    if s.status.success() {
        Ok(())
    } else {
        Err(format!("git {a:?}: {}", String::from_utf8_lossy(&s.stderr)))
    }
}

fn repo(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let r = dir.join(name);
    std::fs::create_dir_all(r.join(".fallow/pipelines")).map_err(|e| e.to_string())?;
    git(&r, &["init", "-q", "-b", "main"])?;
    for c in [
        ["user.email", "b@example.invalid"],
        ["user.name", "b"],
        ["commit.gpgsign", "false"],
        ["core.autocrlf", "false"],
    ] {
        git(&r, &["config", c[0], c[1]])?;
    }
    std::fs::write(r.join(".fallow/squad.toml"), "[[gate]]\nname = \"tests\"\ncommand = \"git status --short\"\nfailures = \"exit-code\"\nbaseline = true\n").map_err(|e| e.to_string())?;
    std::fs::write(r.join(".fallow/pipelines/bench.toml"), PIPELINE).map_err(|e| e.to_string())?;
    std::fs::write(r.join("README.md"), "calc\n").map_err(|e| e.to_string())?;
    git(&r, &["add", "."])?;
    git(&r, &["commit", "-q", "-m", "seed"])?;
    Ok(r)
}

/// The page's address with its secret, from the service's own pipe (as `fallow` gets it).
fn secret_url() -> Result<String, String> {
    #[cfg(windows)]
    {
        let out = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout);
        let sid = text
            .trim()
            .rsplit(',')
            .next()
            .unwrap_or("")
            .trim_matches('"')
            .to_string();
        let path = format!(r"\\.\pipe\fallow-{sid}");
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{path}: {e}"))?;
        f.write_all(b"{\"kind\":\"cli\",\"request\":{\"cmd\":\"url\",\"path\":\"/\"}}\n")
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(f)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("no url: {line}"))
    }
    #[cfg(not(windows))]
    {
        Err("the running-memory benchmark reads the service's address on Windows only".into())
    }
}

fn running(args: &Args) -> Result<(f64, f64), String> {
    let dir = scratch("running");
    let scenario = dir.join("scenario.toml");
    std::fs::write(&scenario, script()).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("config/settings.toml"),
        format!(
            "[defaults]\nmax_parallel = 4\n\n[agents.claude]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            args.fake.display().to_string(),
            scenario.display().to_string()
        ),
    )
    .map_err(|e| e.to_string())?;
    let repos: Vec<PathBuf> = (1..=LOOPS)
        .map(|i| repo(&dir, &format!("p{i}")))
        .collect::<Result<_, _>>()?;
    let (svc, _) = start(args, &dir)?;
    let url = secret_url()?;
    let path_q = url
        .split_once(&format!(":{}", svc.port))
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    let (_, head, _) = http(svc.port, "GET", &path_q, "", None)?;
    let cookie = head
        .lines()
        .find_map(|l| {
            l.strip_prefix("Set-Cookie: ")
                .or_else(|| l.strip_prefix("set-cookie: "))
        })
        .and_then(|c| c.split(';').next())
        .ok_or("no cookie")?
        .to_string();
    let call = |method: &str, path: &str, body: Value| -> Result<Value, String> {
        let (_, _, b) = http(svc.port, method, path, &cookie, Some(&body))?;
        let v: Value = serde_json::from_str(&b).map_err(|e| format!("{path}: {e}: {b}"))?;
        if v["ok"] == false {
            return Err(format!("{method} {path}: {}", v["error"]));
        }
        Ok(v["value"].clone())
    };
    for r in &repos {
        let p = call("POST", "/api/projects", json!({ "path": r }))?["project"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        git(r, &["add", "."])?;
        git(r, &["commit", "-q", "-m", "spec kit"])?;
        let f = call(
            "POST",
            &format!("/api/projects/{p}/features"),
            json!({ "title": "Calc" }),
        )?;
        let conv = f["conversation"].as_str().unwrap_or_default().to_string();
        for text in ["Write it.", "Analyze."] {
            call(
                "POST",
                &format!("/api/conversations/{conv}/say"),
                json!({ "text": text }),
            )?;
            std::thread::sleep(Duration::from_millis(300));
            let end = Instant::now() + Duration::from_secs(60);
            while call("GET", &format!("/api/conversations/{conv}"), json!({}))?["busy"] != false {
                if Instant::now() > end {
                    return Err("the lead's turn did not end".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        call(
            "POST",
            &format!("/api/features/{p}/001-calc/lock"),
            json!({ "pipeline": "bench", "autonomy": "normal", "budget": { "usd": 5.0, "minutes": 60 } }),
        )?;
    }
    // Four checks at once, then the peak over a few seconds.
    let end = Instant::now() + Duration::from_secs(180);
    loop {
        let d = call("GET", "/api/dashboard", json!({}))?;
        if d["agents_running"].as_u64() == Some(LOOPS as u64) {
            break;
        }
        if Instant::now() > end {
            let loops: Vec<String> = d["loops"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    format!(
                        "{} {} {} {}",
                        l["stage"], l["task"], l["waiting"], l["agents"]
                    )
                })
                .collect();
            return Err(format!(
                "4 steps did not run at once: {} running; {loops:?}; {}",
                d["agents_running"], d["needs_you"]
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut peak: f64 = 0.0;
    for _ in 0..10 {
        peak = peak.max(rss_mb(svc.child.id())?);
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok((peak, RUNNING_MB))
}
