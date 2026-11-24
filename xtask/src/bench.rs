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
//!   activity line shown within 2 s of the agent's output at p95 (SC-009; the constitution's
//!   activity latency: an agent's output event to its appearance). The four agents output once
//!   all four are working (5 s after each starts), so making a session is not counted; each
//!   session is watched as the page watches it (its change events, then its view), all four at
//!   once, and the fake agent writes the time it emits the output (issue #3, the engineer's
//!   choice on 2026-11-14);
//! - `refwatch_latency`: a commit made in the project folder from a terminal shown among the
//!   project's git changes within 5 s (SC-004).
//!
//! Each run uses its own config and data folders, so the engineer's are never touched; it needs
//! no other Fallow service running for the same user (there is one per user). Memory is the
//! private working set on Windows, the physical footprint on macOS (016: Fallow and its WebKit
//! processes) and VmRSS on Linux. A failing budget is measured once
//! more; the second result is final (shared runners are noisy).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[cfg(any(windows, target_os = "macos"))]
const WARMUPS: usize = 3;
#[cfg(any(windows, target_os = "macos"))]
const RUNS: usize = 30;
// 014 (constitution 8.0.0, research R-15): the window counts; memory is each process's own
// (private working set), summed over Fallow and its window's web view processes.
#[cfg(any(windows, target_os = "macos"))]
const WINDOW_READY_MS: f64 = 1000.0;
#[cfg(any(windows, target_os = "macos"))]
const IDLE_OPEN_MB: f64 = 250.0;
#[cfg(any(windows, target_os = "macos"))]
const IDLE_CLOSED_MB: f64 = 60.0;
const RUNNING_MB: f64 = 400.0;
/// 016 (constitution VIII, T041): from opening the app with no service running to its window
/// drawn, p95 of 10.
#[cfg(target_os = "macos")]
const COLD_RUNS: usize = 10;
const SOLO_START_MS: f64 = 5000.0;
const SOLO_FOUR_MB: f64 = 400.0;
const SOLO_ACTIVITY_MS: f64 = 2000.0;
const REFWATCH_MS: f64 = 5000.0;

/// One measure against its budget: (name, value, budget, unit).
type Measure = (&'static str, f64, f64, &'static str);

struct Args {
    bin: PathBuf,
    fake: PathBuf,
    /// `--only <name>`: the benchmarks whose name starts with it (all when unset).
    only: Option<String>,
    /// 016 (T041): `--app <Fallow.app>`: the app opened through LaunchServices for the cold
    /// start (macOS; skipped without it).
    app: Option<PathBuf>,
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
        app: None,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--bin" => a.bin = it.next().ok_or("--bin needs a path")?.into(),
            "--fake" => a.fake = it.next().ok_or("--fake needs a path")?.into(),
            "--only" => a.only = Some(it.next().ok_or("--only needs a name")?.clone()),
            "--app" => a.app = Some(it.next().ok_or("--app needs a path")?.into()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    for p in [&a.bin, &a.fake] {
        if !p.is_file() {
            return Err(format!(
                "{} is missing: build it first (cargo build --features automation)",
                p.display()
            ));
        }
    }
    Ok(a)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let args = parse(args).map_err(|e| {
        format!(
            "bench: {e}\nusage: cargo xtask bench [--bin <fallow>] [--fake <fallow-fake-agent>] [--only <name>] [--app <Fallow.app>]"
        )
    })?;
    let mut failed = Vec::new();
    let one =
        |name: &'static str, unit: &'static str, f: fn(&Args) -> Result<(f64, f64), String>| {
            move |a: &Args| f(a).map(|(v, b)| vec![(name, v, b, unit)])
        };
    type Bench = Box<dyn Fn(&Args) -> Result<Vec<Measure>, String>>;
    let benches: Vec<(&str, Bench)> = vec![
        (
            "running memory",
            Box::new(one("running memory", "MB", running)),
        ),
        ("solo_start", Box::new(solo_start)),
        ("solo_four", Box::new(solo_four)),
        ("refwatch_latency", Box::new(refwatch_latency)),
        // 014: after the sessions' benchmarks, whose 10,000-file checkout is sensitive to what
        // ran before it on a runner.
        ("window", Box::new(window)),
        ("close_keeps_work", Box::new(close_keeps_work)),
        // 016 (T042): the WebKit private calls Fallow relies on still work.
        ("close_ends_page", Box::new(close_ends_page)),
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
    /// 016: the WebKit processes there before this service started (macOS: those that came
    /// after are its window's).
    #[cfg(target_os = "macos")]
    webkit_before: std::collections::HashSet<i32>,
    /// The service's other log lines (`window ready <ms>`, 014).
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    log: std::sync::Mutex<std::sync::mpsc::Receiver<String>>,
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
    #[cfg(target_os = "macos")]
    let webkit_before = crate::mac_procs::webkit_now();
    // 014 (research R-15): the page the bench drives is the `automation` build's, asked for.
    let mut child = Command::new(&args.bin)
        .env("FALLOW_AUTOMATION", "1")
        .env("FALLOW_BENCH", "1")
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
    let (ltx, lrx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            // A release build on Windows has no page (014): running is enough.
            if line.starts_with("Fallow is running (its window") {
                let _ = tx.send(Ok(0));
                continue;
            }
            // The page's port: `http://127.0.0.1:<port>/` elsewhere, `… on port <port>` from the
            // Windows `automation` build (014: no secret in the log).
            if let Some(rest) = line
                .split("http://127.0.0.1:")
                .nth(1)
                .or_else(|| line.split("page on port ").nth(1))
            {
                let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let _ = tx.send(Ok(port.parse::<u16>().unwrap_or(0)));
            } else if line.contains("already running") {
                let _ = tx.send(Err(line));
            } else {
                let _ = ltx.send(line);
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
            #[cfg(target_os = "macos")]
            webkit_before,
            log: std::sync::Mutex::new(lrx),
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

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
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

// ---- 014: the window (research R-14, R-15) ---------------------------------------------------

/// The service's process and its window's web view processes (never its agents, their bridges or
/// the commands they run), each one's own memory (private working set) summed, in MB. Windows:
/// PowerShell's process table and performance counters; no reading is an error, never 0.
#[cfg(windows)]
fn private_mb(pid: u32) -> Result<f64, String> {
    let script = format!(
        "$ids=@({pid}); $all=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name; do {{ $n=$ids.Count; $ids=@($ids + ($all | Where-Object {{ $ids -contains $_.ParentProcessId }} | ForEach-Object ProcessId)) | Select-Object -Unique }} while ($ids.Count -ne $n); $counted=@($all | Where-Object {{ ($ids -contains $_.ProcessId) -and ($_.ProcessId -eq {pid} -or $_.Name -eq 'msedgewebview2.exe') }} | ForEach-Object ProcessId); $s=(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object {{ $counted -contains $_.IDProcess }} | Measure-Object WorkingSetPrivate -Sum).Sum; if ($s) {{ [math]::Round($s/1MB,1) }} else {{ 'none' }}"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    let mb: f64 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .replace(',', ".")
        .parse()
        .map_err(|e| format!("private memory could not be read: {e}"))?;
    if mb <= 0.0 {
        return Err("private memory could not be read (no performance counters?)".into());
    }
    Ok(mb)
}

/// How many web view processes the service has.
#[cfg(windows)]
fn web_views(pid: u32) -> usize {
    let script = format!(
        "$ids=@({pid}); $all=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name; do {{ $n=$ids.Count; $ids=@($ids + ($all | Where-Object {{ $ids -contains $_.ParentProcessId }} | ForEach-Object ProcessId)) | Select-Object -Unique }} while ($ids.Count -ne $n); @($all | Where-Object {{ ($ids -contains $_.ProcessId) -and $_.Name -eq 'msedgewebview2.exe' }}).Count"
    );
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// `fallow` (the command line) asks the running service for its window; returns once it exists.
#[cfg(any(windows, target_os = "macos"))]
fn open_window(args: &Args) -> Result<(), String> {
    let out = Command::new(&args.bin)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", args.bin.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "the window did not open: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Closes the service's window as its close button does, and waits until its web view's
/// processes have ended.
#[cfg(windows)]
fn close_window(pid: u32) -> Result<(), String> {
    let script = format!(
        "$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($p -and $p.MainWindowHandle -ne 0) {{ [void]$p.CloseMainWindow() }}"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output();
    let end = Instant::now() + Duration::from_secs(15);
    while web_views(pid) > 0 {
        if Instant::now() > end {
            return Err("the window's web view processes did not end".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

/// 014 (SC-002, SC-003, FR-011): window ready (`fallow` with the service running and the window
/// closed, p95 of 30 after 3 warm-ups, one data folder), then idle memory with the window open
/// and with it closed.
#[cfg(windows)]
fn window(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("window");
    let (svc, _) = start(args, &dir)?;
    let pid = svc.child.id();
    let mut times = Vec::new();
    for i in 0..WARMUPS + RUNS {
        open_window(args)?;
        let end = Instant::now() + Duration::from_secs(20);
        let ms = loop {
            let left = end.saturating_duration_since(Instant::now());
            let line = svc
                .log
                .lock()
                .map_err(|_| "the log".to_string())?
                .recv_timeout(left)
                .map_err(|_| "the window never said it was drawn".to_string())?;
            if let Some(v) = line.strip_prefix("window ready ") {
                break v
                    .trim_end_matches(" ms")
                    .parse::<f64>()
                    .map_err(|e| e.to_string())?;
            }
        };
        if i >= WARMUPS {
            times.push(ms);
        }
        close_window(pid)?;
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    println!(
        "bench: window ready (ms): median {:.0}, p95 {p95:.0}, slowest {:.0}",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    open_window(args)?;
    std::thread::sleep(Duration::from_secs(8));
    let open = private_mb(pid)?;
    close_window(pid)?;
    std::thread::sleep(Duration::from_secs(3));
    let closed = private_mb(pid)?;
    Ok(vec![
        ("window ready", p95, WINDOW_READY_MS, "ms"),
        ("idle memory, window open", open, IDLE_OPEN_MB, "MB"),
        ("idle memory, window closed", closed, IDLE_CLOSED_MB, "MB"),
    ])
}

/// Linux has no window yet.
#[cfg(not(any(windows, target_os = "macos")))]
fn window(_args: &Args) -> Result<Vec<Measure>, String> {
    Ok(Vec::new())
}

// ---- 016: the window on macOS (research M-17, M-18; T041, T042, T045) ----------------------

/// The per-user service socket (in the data folder under `HOME`, wherever `--data-dir` points).
#[cfg(target_os = "macos")]
fn service_sock() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .ok_or("no HOME")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Fallow/service.sock"))
}

/// One request on the service's socket.
#[cfg(target_os = "macos")]
fn sock_ask(req: Value) -> Result<Value, String> {
    let path = service_sock()?;
    let mut s = std::os::unix::net::UnixStream::connect(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(60))).ok();
    writeln!(s, "{}", json!({ "kind": "cli", "request": req })).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
    if v["ok"] == false {
        return Err(format!("{req}: {}", v["error"]));
    }
    Ok(v)
}

/// Whether the service's window has its web content process.
#[cfg(target_os = "macos")]
fn has_page(svc: &Service) -> bool {
    crate::mac_procs::webkit_of(svc.child.id() as i32, &svc.webkit_before)
        .iter()
        .any(|(_, n)| n.contains("WebContent"))
}

/// Waits (up to `secs`) until the window's web content process is there (`true`) or gone.
#[cfg(target_os = "macos")]
fn until_page(svc: &Service, there: bool, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while has_page(svc) != there {
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    true
}

/// The next `window ready <ms>` line of the service's log.
#[cfg(target_os = "macos")]
fn ready_ms(svc: &Service) -> Result<f64, String> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let line = svc
            .log
            .lock()
            .map_err(|_| "the log".to_string())?
            .recv_timeout(left)
            .map_err(|_| {
                "the window never said it was drawn (is the screen locked?)".to_string()
            })?;
        if let Some(v) = line.strip_prefix("window ready ") {
            return v
                .trim_end_matches(" ms")
                .parse::<f64>()
                .map_err(|e| e.to_string());
        }
    }
}

/// Closes the window as its red button does (`window-close`, `automation` builds) and waits
/// until its web content process has ended.
#[cfg(target_os = "macos")]
fn close_window_mac(svc: &Service) -> Result<(), String> {
    sock_ask(json!({ "cmd": "window-close" }))?;
    if until_page(svc, false, 15) {
        Ok(())
    } else {
        Err("the window's web content process did not end".into())
    }
}

/// 016 (FR-031, M-17, T041): window ready (p95 of 30 after 3 warm-ups, the service running),
/// idle memory with the window open and 10 s after closing it (physical footprint of Fallow and
/// its WebKit processes), and with `--app` the cold start: the app opened through LaunchServices
/// with no service running, to its window drawn (p95 of 10).
#[cfg(target_os = "macos")]
fn window(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("window");
    let (svc, _) = start(args, &dir)?;
    let pid = svc.child.id() as i32;
    let mut times = Vec::new();
    for i in 0..WARMUPS + RUNS {
        open_window(args)?;
        let ms = ready_ms(&svc)?;
        if i >= WARMUPS {
            times.push(ms);
        }
        close_window_mac(&svc)?;
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    println!(
        "bench: window ready (ms): median {:.0}, p95 {p95:.0}, slowest {:.0}",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    open_window(args)?;
    let _ = ready_ms(&svc);
    std::thread::sleep(Duration::from_secs(8));
    let open = crate::mac_procs::total_mb(pid, &svc.webkit_before);
    close_window_mac(&svc)?;
    std::thread::sleep(Duration::from_secs(10));
    let closed = crate::mac_procs::total_mb(pid, &svc.webkit_before);
    drop(svc);
    let mut m = vec![
        ("window ready", p95, WINDOW_READY_MS, "ms"),
        ("idle memory, window open", open, IDLE_OPEN_MB, "MB"),
        ("idle memory, window closed", closed, IDLE_CLOSED_MB, "MB"),
    ];
    if let Some(app) = &args.app {
        m.push((
            "cold start to window",
            cold_start(app)?,
            WINDOW_READY_MS,
            "ms",
        ));
    }
    Ok(m)
}

/// 016 (constitution VIII, T041): `open -a` on `app` with no Fallow running, to the window drawn
/// (its log's `window ready`), p95 of 10. LaunchServices starts the app with the user's real
/// `HOME`: for CI's runners, never a developer's Mac.
#[cfg(target_os = "macos")]
fn cold_start(app: &Path) -> Result<f64, String> {
    let sock = service_sock()?;
    let log = sock.with_file_name("service.log");
    let mut times = Vec::new();
    for _ in 0..COLD_RUNS {
        if sock_ask(json!({ "cmd": "status" })).is_ok() {
            return Err("a Fallow runs already: the cold start needs none".into());
        }
        let from = std::fs::metadata(&log).map_or(0, |m| m.len());
        let t0 = Instant::now();
        // Absolute: `open -a` takes a relative path for an app's name.
        let app = app
            .canonicalize()
            .map_err(|e| format!("{}: {e}", app.display()))?;
        let s = Command::new("/usr/bin/open")
            .args(["--env", "FALLOW_BENCH=1", "-a"])
            .arg(&app)
            .status()
            .map_err(|e| e.to_string())?;
        if !s.success() {
            return Err(format!("open -a {} failed", app.display()));
        }
        let end = t0 + Duration::from_secs(20);
        let ms = loop {
            let text = std::fs::read(&log).unwrap_or_default();
            let new = String::from_utf8_lossy(text.get(from as usize..).unwrap_or_default());
            if new.lines().any(|l| l.starts_with("window ready ")) {
                break t0.elapsed().as_secs_f64() * 1000.0;
            }
            if Instant::now() > end {
                let _ = sock_ask(json!({ "cmd": "stop" }));
                return Err("the app's window never said it was drawn".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        times.push(ms);
        sock_ask(json!({ "cmd": "stop" }))?;
        let end = Instant::now() + Duration::from_secs(20);
        while sock.exists() || sock_ask(json!({ "cmd": "status" })).is_ok() {
            if Instant::now() > end {
                return Err("the app did not stop".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    println!(
        "bench: cold start (ms): median {:.0}, p95 {p95:.0}",
        times[times.len() / 2]
    );
    Ok(p95)
}

/// 016 (research M-18, T042): the WebKit private calls Fallow relies on still work: closing the
/// window ends its web content process (`_close`), 5 times; and with the window minimized, the
/// page still gets the Studio's changes (hidden-page suppression off): a session's activity that
/// comes 25 s after the window went into the Dock is shown by the page.
#[cfg(target_os = "macos")]
fn close_ends_page(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("close-ends-page");
    let r = repo(&dir, "page")?;
    let scenario = "[[step]]\nkind = \"assistant_turn\"\nweb = [\"https://example.com/docs/late\"]\nbefore_ms = 25000\nsleep_ms = 2000\noutput = { reply = \"done\" }\n";
    let (svc, cookie, p) = solo_service(args, &dir, scenario, &r)?;
    let mut left = 0.0;
    for _ in 0..5 {
        open_window(args)?;
        if !until_page(&svc, true, 20) {
            return Err("the window's web content process did not start".into());
        }
        sock_ask(json!({ "cmd": "window-close" }))?;
        if !until_page(&svc, false, 10) {
            left += 1.0;
        }
    }
    println!("bench: web content processes left 10 s after a close: {left} of 5");
    // The session first, then the window on its project (the page shows its last session),
    // then the page's first activity line (the turn's start), then the Dock.
    let v = call(
        &svc,
        &cookie,
        "POST",
        &format!("/api/projects/{p}/sessions"),
        json!({ "text": "look it up later" }),
    )?;
    let t0 = Instant::now();
    let s = v["session"]
        .as_str()
        .ok_or_else(|| format!("no session: {v}"))?
        .to_string();
    sock_ask(json!({ "cmd": "window", "route": format!("#/p/{p}") }))?;
    let next_activity = |until: Instant| -> Option<f64> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match svc.log.lock().ok()?.recv_timeout(left) {
                Ok(l) if l.starts_with(&format!("activity {s} ")) => {
                    return Some(t0.elapsed().as_secs_f64());
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    };
    let first = next_activity(t0 + Duration::from_secs(20))
        .ok_or("the page never showed the session's start")?;
    sock_ask(json!({ "cmd": "window-minimize" }))?;
    let shown = next_activity(t0 + Duration::from_secs(60)).unwrap_or(f64::INFINITY);
    // The late activity itself, not another early line (review).
    if shown < 24.0 {
        return Err(format!(
            "the page showed activity at {shown:.1} s, before the late one (25 s)"
        ));
    }
    println!(
        "bench: the page showed the turn's start at {first:.1} s, was minimized, and showed the activity that came at 25 s at {shown:.1} s"
    );
    Ok(vec![
        ("web content left after closes", left, 1.0, "of 5"),
        (
            "minimized window gets the Studio's changes",
            shown,
            40.0,
            "s",
        ),
    ])
}

#[cfg(not(target_os = "macos"))]
fn close_ends_page(_args: &Args) -> Result<Vec<Measure>, String> {
    Ok(Vec::new())
}

/// 014 (SC-005, T031): work goes on with the window closed. A session's turn is started on the
/// scripted agent with the window open; the window is closed at once; the turn must finish
/// (within 30 s, the "budget"). A session rather than a loop: both run in the engine, which
/// never depends on the window (constitution VII). 016 (T045): on macOS too.
#[cfg(any(windows, target_os = "macos"))]
fn close_keeps_work(args: &Args) -> Result<Vec<Measure>, String> {
    let dir = scratch("close-keeps-work");
    let r = repo(&dir, "work")?;
    let scenario = "[[step]]
kind = \"assistant_turn\"
sleep_ms = 4000
output = { reply = \"done\" }
";
    let (svc, cookie, p) = solo_service(args, &dir, scenario, &r)?;
    open_window(args)?;
    let t0 = Instant::now();
    let v = call(
        &svc,
        &cookie,
        "POST",
        &format!("/api/projects/{p}/sessions"),
        json!({ "text": "keep working" }),
    )?;
    let s = v["session"]
        .as_str()
        .ok_or_else(|| format!("no session: {v}"))?
        .to_string();
    #[cfg(windows)]
    close_window(svc.child.id())?;
    #[cfg(target_os = "macos")]
    {
        if !until_page(&svc, true, 20) {
            return Err("the window's web content process did not start".into());
        }
        close_window_mac(&svc)?;
    }
    let s_done = until(&svc, &cookie, &s, t0, |v| {
        v["busy"] == false
            && v["turns"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["reply"] == "done"))
    })?;
    Ok(vec![("close keeps work", s_done / 1000.0, 30.0, "s")])
}

#[cfg(not(any(windows, target_os = "macos")))]
fn close_keeps_work(_args: &Args) -> Result<Vec<Measure>, String> {
    Ok(Vec::new())
}

/// The memory counted: the service with its window open on Windows (014), the service elsewhere.
fn counted_mb(args: &Args, svc: &Service) -> Result<f64, String> {
    #[cfg(windows)]
    {
        let _ = args;
        private_mb(svc.child.id())
    }
    // 016 (research M-17): physical footprint, Fallow and its WebKit processes summed.
    #[cfg(target_os = "macos")]
    {
        let _ = args;
        Ok(crate::mac_procs::total_mb(
            svc.child.id() as i32,
            &svc.webkit_before,
        ))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = args;
        rss_mb(svc.child.id())
    }
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
    // The service's socket in the user's data folder (`config::data_dir`), wherever
    // `--data-dir` points: there is one service per user.
    #[cfg(unix)]
    {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME").ok_or("no HOME")?;
        // 016 (research M-14): Application Support on macOS, no longer Caches.
        let path = if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Fallow/service.sock")
        } else {
            var("XDG_CACHE_HOME")
                .unwrap_or_else(|| home.join(".cache"))
                .join("fallow/service.sock")
        };
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
    let stamps = dir.join("stamps");
    std::fs::create_dir_all(&stamps).map_err(|e| e.to_string())?;
    let scenario: String = (0..4)
        .map(|i| {
            format!(
                "[[step]]\nkind = \"assistant_turn\"\nweb = [\"https://example.com/docs/{i}\"]\nstamp_dir = {:?}\nbefore_ms = 5000\nsleep_ms = 30000\noutput = {{ reply = \"done\" }}\n\n",
                stamps.display().to_string()
            )
        })
        .collect();
    let (svc, cookie, p) = solo_service(args, &dir, &scenario, &r)?;
    let mut all_made = 0.0;
    let seen: Vec<Result<(f64, f64), String>> = std::thread::scope(|sc| {
        let mut watching = Vec::new();
        for i in 0..4 {
            let v = call(
                &svc,
                &cookie,
                "POST",
                &format!("/api/projects/{p}/sessions"),
                json!({ "text": format!("Look up the docs {i}") }),
            );
            let s = match v.and_then(|v| {
                v["session"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("no session: {v}"))
            }) {
                Ok(s) => s,
                Err(e) => {
                    watching.push(sc.spawn(move || Err(e)));
                    continue;
                }
            };
            let (svc, cookie, stamps) = (&svc, &cookie, &stamps);
            watching.push(sc.spawn(move || first_activity(svc, cookie, &s, stamps)));
        }
        all_made = unix_ms();
        watching
            .into_iter()
            .map(|w| w.join().unwrap_or_else(|_| Err("a watch failed".into())))
            .collect()
    });
    let seen = seen
        .into_iter()
        .collect::<Result<Vec<(f64, f64)>, String>>()?;
    // The constitution's case: four sessions working when the agents output (review).
    if seen.iter().any(|(_, at)| *at < all_made) {
        return Err(
            "solo_four: an agent output before all four sessions were made: raise before_ms".into(),
        );
    }
    let mut seen: Vec<f64> = seen.into_iter().map(|(ms, _)| ms).collect();
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
    #[cfg(windows)]
    {
        open_window(args)?;
        std::thread::sleep(Duration::from_secs(5));
    }
    let mut peak: f64 = 0.0;
    for _ in 0..10 {
        peak = peak.max(counted_mb(args, &svc)?);
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(vec![
        ("solo_four memory", peak, SOLO_FOUR_MB, "MB"),
        ("solo_four activity", p95, SOLO_ACTIVITY_MS, "ms"),
    ])
}

/// Milliseconds since the Unix epoch.
fn unix_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Session `s` watched as the page watches it (its change events, then its view): the time from
/// the fake agent's output, as it stamped it in `stamps`, to its first activity line in the
/// view (issue #3); and that output's time.
fn first_activity(
    svc: &Service,
    cookie: &str,
    s: &str,
    stamps: &Path,
) -> Result<(f64, f64), String> {
    let end = Instant::now() + Duration::from_secs(60);
    let mut after = 0u64;
    loop {
        let v = call(svc, cookie, "GET", &format!("/api/sessions/{s}"), json!({}))?;
        let seen = unix_ms();
        let activity = v["activity"].to_string();
        if let Some(rest) = activity.split("https://example.com/docs/").nth(1) {
            let url = format!(
                "https://example.com/docs/{}",
                rest.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            );
            let name: String = url
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let stamp = std::fs::read_to_string(stamps.join(format!("{name}.ms")))
                .map_err(|e| format!("no time stamp for {url}: {e}"))?;
            let at: f64 = stamp
                .trim()
                .parse()
                .map_err(|e| format!("time stamp {stamp:?}: {e}"))?;
            return Ok((seen - at, at));
        }
        if Instant::now() > end {
            return Err(format!(
                "session {s} never showed its activity: {}",
                v["session"]
            ));
        }
        // The next change of the session, as the page waits for it.
        let e = call(
            svc,
            cookie,
            "GET",
            &format!("/api/events?scope=conv:{s}&after={after}"),
            json!({}),
        )?;
        after = e["seq"].as_u64().unwrap_or(after);
    }
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
    // 014: with the window open, its web view counted (constitution 8.0.0).
    #[cfg(windows)]
    {
        open_window(args)?;
        std::thread::sleep(Duration::from_secs(5));
    }
    let mut peak: f64 = 0.0;
    for _ in 0..10 {
        peak = peak.max(counted_mb(args, &svc)?);
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok((peak, RUNNING_MB))
}
