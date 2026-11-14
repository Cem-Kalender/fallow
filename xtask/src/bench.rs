//! `cargo xtask bench` (007 T083; constitution VIII, "Budgets & Measurement").
//!
//! Three budgets, measured on Fallow's own process (`fallow serve`), never its agents:
//! - page ready: process launch to the page answering, p95 of 30 launches after 3 warm-ups,
//!   under 500 ms;
//! - idle memory: settled, with no loop running, under 60 MB;
//! - running memory: four loops whose checkpoints run 4 checker steps at once (the scripted
//!   fake agent), under 200 MB. (The four steps come from four loops, each with one checker, so
//!   the measure holds whatever a pipeline marks `parallel_with`.)
//!
//!
//! 012 (T052; research R-17) adds three for solo sessions:
//! - `solo_start`: from the first message to the turn's start, on a project of 10,000 files,
//!   under 5 s (SC-001);
//! - `solo_four`: four scripted sessions at once, Fallow under 200 MB, and each session's first
//!   activity line shown within 2 s of its message at p95 (SC-009);
//! - `refwatch_latency`: a commit made in the project folder from a terminal shown among the
//!   project's git changes within 5 s (SC-004).
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
const SOLO_START_MS: f64 = 5000.0;
const SOLO_FOUR_MB: f64 = 200.0;
const SOLO_ACTIVITY_MS: f64 = 2000.0;
const REFWATCH_MS: f64 = 5000.0;

/// One measure against its budget: (name, value, budget, unit).
type Measure = (&'static str, f64, f64, &'static str);

struct Args {
    bin: PathBuf,
    fake: PathBuf,
    /// `--only <name>`: the benchmarks whose name starts with it (all when unset).
    only: Option<String>,
}

fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug")
}

fn parse(args: &[String]) -> Result<Args, String> {
    let exe = |n: &str| target_dir().join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let mut a = Args {
        bin: exe("fallow"),
        fake: exe("fallow-fake-agent"),
        only: None,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--bin" => a.bin = it.next().ok_or("--bin needs a path")?.into(),
            "--fake" => a.fake = it.next().ok_or("--fake needs a path")?.into(),
            "--only" => a.only = Some(it.next().ok_or("--only needs a name")?.clone()),
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
            "bench: {e}\nusage: cargo xtask bench [--bin <fallow>] [--fake <fallow-fake-agent>] [--only <name>]"
        )
    })?;
    let mut failed = Vec::new();
    let one =
        |name: &'static str, unit: &'static str, f: fn(&Args) -> Result<(f64, f64), String>| {
            move |a: &Args| f(a).map(|(v, b)| vec![(name, v, b, unit)])
        };
    type Bench = Box<dyn Fn(&Args) -> Result<Vec<Measure>, String>>;
    let benches: Vec<(&str, Bench)> = vec![
        ("page ready", Box::new(one("page ready", "ms", ready))),
        ("idle memory", Box::new(one("idle memory", "MB", idle))),
        (
            "running memory",
            Box::new(one("running memory", "MB", running)),
        ),
        ("solo_start", Box::new(solo_start)),
        ("solo_four", Box::new(solo_four)),
        ("refwatch_latency", Box::new(refwatch_latency)),
    ];
    let chosen: Vec<&(&str, Bench)> = benches
        .iter()
        .filter(|(n, _)| args.only.as_deref().is_none_or(|o| n.starts_with(o)))
        .collect();
    if chosen.is_empty() {
        let names: Vec<&str> = benches.iter().map(|(n, _)| *n).collect();
        return Err(format!(
            "bench: no benchmark is named like {:?}: {}",
            args.only.unwrap_or_default(),
            names.join(", ")
        ));
    }
    for (_, f) in chosen {
        let first = f(&args)?;
        if first.iter().all(|(_, v, b, _)| v < b) {
            for (name, v, b, unit) in &first {
                println!("bench: {name}: {v:.1} {unit} (budget {b} {unit}): ok");
            }
            continue;
        }
        // Once more; the second result is final.
        let second = f(&args)?;
        for ((name, v, b, unit), (_, v2, _, _)) in first.iter().zip(&second) {
            let ok = v2 < b;
            println!(
                "bench: {name}: {v:.1} {unit}, then {v2:.1} {unit} (budget {b} {unit}): {}",
                if ok { "ok" } else { "OVER" }
            );
            if !ok {
                failed.push(*name);
            }
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
    // The service's socket in the user's cache folder (`config::data_dir`), wherever
    // `--data-dir` points: there is one service per user.
    #[cfg(unix)]
    {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME").ok_or("no HOME")?;
        let cache = if cfg!(target_os = "macos") {
            home.join("Library/Caches")
        } else {
            var("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"))
        };
        let path = cache.join("fallow/service.sock");
        let mut s = std::os::unix::net::UnixStream::connect(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        s.write_all(b"{\"kind\":\"cli\",\"request\":{\"cmd\":\"url\",\"path\":\"/\"}}\n")
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(s)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("no url: {line}"))
    }
}

/// The page's cookie, from the service's secret address.
fn cookie_of(svc: &Service) -> Result<String, String> {
    let url = secret_url()?;
    let path_q = url
        .split_once(&format!(":{}", svc.port))
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    let (_, head, _) = http(svc.port, "GET", &path_q, "", None)?;
    head.lines()
        .find_map(|l| {
            l.strip_prefix("Set-Cookie: ")
                .or_else(|| l.strip_prefix("set-cookie: "))
        })
        .and_then(|c| c.split(';').next())
        .map(str::to_string)
        .ok_or_else(|| "no cookie".to_string())
}

/// One JSON request to the service, its `value`.
fn call(
    svc: &Service,
    cookie: &str,
    method: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let (_, _, b) = http(svc.port, method, path, cookie, Some(&body))?;
    let v: Value = serde_json::from_str(&b).map_err(|e| format!("{path}: {e}: {b}"))?;
    if v["ok"] == false {
        return Err(format!("{method} {path}: {}", v["error"]));
    }
    Ok(v["value"].clone())
}

/// A service whose `claude` plays `scenario`, with up to four agents at once, and a project
/// for `repo`: the service, its cookie and the project's id.
fn solo_service(
    args: &Args,
    dir: &Path,
    scenario: &str,
    repo: &Path,
) -> Result<(Service, String, String), String> {
    let sc = dir.join("scenario.toml");
    std::fs::write(&sc, scenario).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("config/settings.toml"),
        format!(
            "[defaults]\nmax_parallel = 4\n\n[agents.claude]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            args.fake.display().to_string(),
            sc.display().to_string()
        ),
    )
    .map_err(|e| e.to_string())?;
    let (svc, _) = start(args, dir)?;
    let cookie = cookie_of(&svc)?;
    let p = call(
        &svc,
        &cookie,
        "POST",
        "/api/projects",
        json!({ "path": repo }),
    )?["project"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    git(repo, &["add", "."])?;
    git(repo, &["commit", "-q", "-m", "spec kit"])?;
    Ok((svc, cookie, p))
}

/// Waits until `pred` holds for the session's view; the time it took.
fn until(
    svc: &Service,
    cookie: &str,
    s: &str,
    t0: Instant,
    pred: impl Fn(&Value) -> bool,
) -> Result<f64, String> {
    let end = t0 + Duration::from_secs(60);
    loop {
        let v = call(svc, cookie, "GET", &format!("/api/sessions/{s}"), json!({}))?;
        if pred(&v) {
            return Ok(t0.elapsed().as_secs_f64() * 1000.0);
        }
        if Instant::now() > end {
            return Err(format!("session {s} never got there: {}", v["session"]));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 012 SC-001: the first message of a session on a project of 10,000 files, to its turn's start
/// (the working copy and branch made, the agent started).
fn solo_start(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("solo-start");
    let r = repo(&dir, "big")?;
    for i in 0..10_000 {
        let d = r.join(format!("src/m{:03}", i / 100));
        if i % 100 == 0 {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        }
        std::fs::write(d.join(format!("f{i}.txt")), format!("file {i}\n"))
            .map_err(|e| e.to_string())?;
    }
    git(&r, &["add", "."])?;
    git(&r, &["commit", "-q", "-m", "ten thousand files"])?;
    let scenario =
        "[[step]]\nkind = \"assistant_turn\"\nsleep_ms = 20000\noutput = { reply = \"done\" }\n";
    let (svc, cookie, p) = solo_service(args, &dir, scenario, &r)?;
    let t0 = Instant::now();
    let v = call(
        &svc,
        &cookie,
        "POST",
        &format!("/api/projects/{p}/sessions"),
        json!({ "text": "Fix the week" }),
    )?;
    let s = v["session"]
        .as_str()
        .ok_or_else(|| format!("no session: {v}"))?
        .to_string();
    let ms = until(&svc, &cookie, &s, t0, |v| {
        v["busy"] == true || v["turns"].as_array().is_some_and(|t| !t.is_empty())
    })?;
    Ok(vec![("solo_start", ms, SOLO_START_MS, "ms")])
}

/// 012 SC-009: four sessions at once: Fallow's memory, and how soon each first activity line
/// shows (p95 of the four, read every 20 ms).
fn solo_four(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("solo-four");
    let r = repo(&dir, "four")?;
    let step = "[[step]]\nkind = \"assistant_turn\"\nweb = [\"https://example.com/docs\"]\nsleep_ms = 30000\noutput = { reply = \"done\" }\n\n";
    let (svc, cookie, p) = solo_service(args, &dir, &step.repeat(4), &r)?;
    let mut sessions = Vec::new();
    for i in 0..4 {
        let t0 = Instant::now();
        let v = call(
            &svc,
            &cookie,
            "POST",
            &format!("/api/projects/{p}/sessions"),
            json!({ "text": format!("Look up the docs {i}") }),
        )?;
        let s = v["session"]
            .as_str()
            .ok_or_else(|| format!("no session: {v}"))?
            .to_string();
        sessions.push((s, t0));
    }
    let mut seen = Vec::new();
    for (s, t0) in &sessions {
        seen.push(until(&svc, &cookie, s, *t0, |v| {
            v["activity"].as_array().is_some_and(|a| !a.is_empty())
        })?);
    }
    println!(
        "bench: solo_four first activity per session (ms): {}",
        seen.iter()
            .map(|v| format!("{v:.0}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    seen.sort_by(|a, b| a.total_cmp(b));
    let p95 = seen[((seen.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(seen.len() - 1)];
    let mut peak: f64 = 0.0;
    for _ in 0..10 {
        peak = peak.max(rss_mb(svc.child.id())?);
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(vec![
        ("solo_four memory", peak, SOLO_FOUR_MB, "MB"),
        ("solo_four activity", p95, SOLO_ACTIVITY_MS, "ms"),
    ])
}

/// 012 SC-004: a commit made in the project folder from a terminal, shown among the project's
/// git changes.
fn refwatch_latency(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("refwatch");
    let r = repo(&dir, "watched")?;
    let (svc, cookie, p) = solo_service(args, &dir, "", &r)?;
    // The watch has read the project once.
    std::thread::sleep(Duration::from_millis(3000));
    std::fs::write(r.join("t.txt"), "terminal\n").map_err(|e| e.to_string())?;
    git(&r, &["add", "t.txt"])?;
    let t0 = Instant::now();
    git(&r, &["commit", "-q", "-m", "from a terminal"])?;
    // That very commit, not an earlier change of `main` (the project's own setup commit).
    let head = Command::new("git")
        .current_dir(&r)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .map_err(|e| e.to_string())?;
    if head.len() != 40 {
        return Err(format!("cannot read the terminal commit: {head:?}"));
    }
    let end = t0 + Duration::from_secs(60);
    loop {
        let v = call(
            &svc,
            &cookie,
            "GET",
            &format!("/api/projects/{p}/git-changes"),
            json!({}),
        )?;
        if v["changes"].as_array().is_some_and(|c| {
            c.iter()
                .any(|c| c["refname"] == "refs/heads/main" && c["after"] == head.as_str())
        }) {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            return Ok(vec![("refwatch_latency", ms, REFWATCH_MS, "ms")]);
        }
        if Instant::now() > end {
            return Err(format!("the terminal commit never showed: {v}"));
        }
        std::thread::sleep(Duration::from_millis(50));
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
