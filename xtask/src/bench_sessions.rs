//! `cargo xtask bench-sessions` (002 T026, T036; SC-003, SC-005, SC-006; research R-9).
//!
//! Fallow runs in the PTY harness with a project that is a git repository and xtask personas
//! as its only profiles (built-ins disabled in a temporary config directory). New sessions take
//! the default choice, so each starts in a new worktree (US6).
//!
//! Run 1:
//! - **echo** (SC-003): an `echo-agent` session; for 100 keys, the time from the key to the
//!   agent's redraw on Fallow's screen. p95 < 50 ms. The session start time in a new worktree
//!   is reported (no budget).
//! - **switch** (SC-005): two `banner-agent` sessions that fill their screens with `A` and `B`;
//!   10 switches with `Ctrl-B`/`Ctrl-N`, each timed from the key to the other session's letters
//!   on screen. p95 < 100 ms.
//!
//! Run 2:
//! - **flood** (SC-006): one `echo-agent` session and seven `flood-agent` sessions printing
//!   continuously at 1,000 colored 200-column lines per second each (about 1.4 MB/s in all;
//!   research R-9 defines this as SC-006's "continuous output"); 50 keys into the echo session.
//!   p95 < 100 ms.
//!
//! A run where a session never starts, or Fallow's identity checks fail, is INVALID.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::pty::{self, Session};

const ECHO_KEYS: usize = 100;
const ECHO_BUDGET: Duration = Duration::from_millis(50);
const SWITCHES: usize = 10;
const SWITCH_BUDGET: Duration = Duration::from_millis(100);
const FLOOD_SESSIONS: usize = 7;
const FLOOD_KEYS: usize = 50;
const FLOOD_BUDGET: Duration = Duration::from_millis(100);

const LEAVE: &[u8] = b"\x1d";
const CTRL_B: &[u8] = b"\x02";
const CTRL_N: &[u8] = b"\x0e";
const DOWN: &[u8] = b"\x1b[B";

fn p95(times: &[Duration]) -> Duration {
    let mut v = times.to_vec();
    v.sort();
    v[((v.len() as f64 * 0.95).ceil() as usize).clamp(1, v.len()) - 1]
}

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bin" => match it.next() {
                Some(b) => bin = PathBuf::from(b),
                None => {
                    eprintln!("--bin needs a value");
                    return 2;
                }
            },
            other => {
                eprintln!("bench-sessions: unknown argument {other:?}");
                return 2;
            }
        }
    }
    let base = std::env::temp_dir().join(format!("fallow-bench-sessions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let result = run_all(&bin, &base);
    let _ = std::fs::remove_dir_all(&base);
    match result {
        Ok(code) => code,
        Err(e) => {
            println!("invalid: {e}");
            println!("os={} result=INVALID", std::env::consts::OS);
            pty::step_summary(&format!(
                "### Sessions ({})\n\nINVALID: {e}\n",
                std::env::consts::OS
            ));
            2
        }
    }
}

fn run_all(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    // Menu order: echo (0), a (1), b (2), flood (3).
    for (id, args) in [
        ("echo", "\"echo-agent\""),
        ("a", "\"banner-agent\", \"A\""),
        ("b", "\"banner-agent\", \"B\""),
        ("flood", "\"flood-agent\", \"forever\""),
    ] {
        profiles.push_str(&format!(
            "[[profile]]\nid = \"{id}\"\nname = \"{id}\"\ncommand = {:?}\nargs = [{args}]\n\n",
            me.display().to_string()
        ));
    }
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;
    init_repo(&project).map_err(|e| format!("cannot create the test repository: {e}"))?;
    println!(
        "bench-sessions: {} (echo p95 < {} ms, switch p95 < {} ms, echo among {} flooding sessions p95 < {} ms)",
        bin.display(),
        ECHO_BUDGET.as_millis(),
        SWITCH_BUDGET.as_millis(),
        FLOOD_SESSIONS,
        FLOOD_BUDGET.as_millis()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();

    // Run 1: echo and switch.
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let first = (|| {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".to_string());
        }
        let start = start_session(&mut s, 0, b"echo-ready")?;
        let echo = echo_keys(&mut s, ECHO_KEYS)?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        start_session(&mut s, 1, b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        start_session(&mut s, 2, b"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBB")?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(500));
        // Now on B: Ctrl-B goes to A, Ctrl-N back to B.
        let mut switches = Vec::with_capacity(SWITCHES);
        for i in 0..SWITCHES {
            let (key, want): (&[u8], &[u8]) = if i % 2 == 0 {
                (CTRL_B, b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
            } else {
                (CTRL_N, b"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBB")
            };
            s.expect(want);
            let t0 = Instant::now();
            s.send(key);
            let t1 = s
                .seen(Duration::from_secs(5))
                .ok_or(format!("switch {i}: the other session never showed"))?;
            switches.push(t1.saturating_duration_since(t0));
            std::thread::sleep(Duration::from_millis(150));
            s.send(LEAVE);
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok((start, echo, switches))
    })();
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    let (start, echo, switches) = match first {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            (Duration::ZERO, Vec::new(), Vec::new())
        }
    };
    println!("evidence (run 1): {}", outcome.evidence());
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }

    // Run 2: one echo session among seven flooding ones.
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let second = (|| {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".to_string());
        }
        start_session(&mut s, 0, b"echo-ready")?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..FLOOD_SESSIONS {
            s.send(b"n");
            std::thread::sleep(Duration::from_millis(300));
            for _ in 0..3 {
                s.send(DOWN);
            }
            std::thread::sleep(Duration::from_millis(100));
            s.send(b"\r");
            std::thread::sleep(Duration::from_millis(2500));
            s.send(LEAVE);
            std::thread::sleep(Duration::from_millis(300));
        }
        // Sessions in start order: echo, flood ×7. From the last flood, Ctrl-N wraps to echo.
        s.send(CTRL_N);
        std::thread::sleep(Duration::from_millis(1000));
        let diag = diag_start(s.pid);
        let r = echo_keys(&mut s, FLOOD_KEYS);
        diag_finish(diag, &r);
        r
    })();
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    let flood = match second {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            Vec::new()
        }
    };
    println!("evidence (run 2): {}", outcome.evidence());
    let sessions = pty::stat_field(&outcome.stats_line, "sessions").unwrap_or(0);
    if sessions < (FLOOD_SESSIONS + 1) as u64 {
        problems.push(format!(
            "{sessions} sessions started, need {}",
            FLOOD_SESSIONS + 1
        ));
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }

    let (e95, s95, f95) = (p95(&echo), p95(&switches), p95(&flood));
    let pass = e95 < ECHO_BUDGET && s95 < SWITCH_BUDGET && f95 < FLOOD_BUDGET;
    let result = if pass { "PASS" } else { "FAIL" };
    println!(
        "os={} worktree_start_ms={:.1} keys={} median_ms={:.1} p95_ms={:.1} max_ms={:.1} budget_ms={} result={}",
        std::env::consts::OS,
        pty::ms(start),
        echo.len(),
        pty::ms(echo[echo.len() / 2]),
        pty::ms(e95),
        pty::ms(*echo.iter().max().unwrap_or(&Duration::ZERO)),
        ECHO_BUDGET.as_millis(),
        if e95 < ECHO_BUDGET { "PASS" } else { "FAIL" }
    );
    println!(
        "os={} switch_p95_ms={:.1} switch_max_ms={:.1} budget_ms={} flood_sessions={} flood_echo_p95_ms={:.1} flood_echo_max_ms={:.1} budget_ms={} result={result}",
        std::env::consts::OS,
        pty::ms(s95),
        pty::ms(*switches.iter().max().unwrap_or(&Duration::ZERO)),
        SWITCH_BUDGET.as_millis(),
        FLOOD_SESSIONS,
        pty::ms(f95),
        pty::ms(*flood.iter().max().unwrap_or(&Duration::ZERO)),
        FLOOD_BUDGET.as_millis()
    );
    pty::step_summary(&format!(
        "### Sessions ({})\n\n| measure | p95 | budget |\n|---|---|---|\n| key echo | {:.1} ms | < {} ms |\n| switch session | {:.1} ms | < {} ms |\n| key echo with {} sessions flooding | {:.1} ms | < {} ms |\n| session start in a new worktree | {:.0} ms | (no budget) |\n\n**{result}**\n",
        std::env::consts::OS,
        pty::ms(e95),
        ECHO_BUDGET.as_millis(),
        pty::ms(s95),
        SWITCH_BUDGET.as_millis(),
        FLOOD_SESSIONS,
        pty::ms(f95),
        FLOOD_BUDGET.as_millis(),
        pty::ms(start)
    ));
    Ok(if pass { 0 } else { 1 })
}

/// Opens the start menu, picks profile `index` with Down, starts it, and waits for `ready`.
/// Returns the time from Enter to `ready`.
fn start_session(s: &mut Session, index: usize, ready: &[u8]) -> Result<Duration, String> {
    s.expect(ready);
    s.send(b"n");
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..index {
        s.send(DOWN);
    }
    std::thread::sleep(Duration::from_millis(100));
    let t0 = Instant::now();
    s.send(b"\r");
    let at = s
        .seen(Duration::from_secs(30))
        .ok_or(format!("session {index} never showed its first screen"))?;
    std::thread::sleep(Duration::from_millis(300));
    Ok(at.saturating_duration_since(t0))
}

/// Types `n` keys into the focused echo session and times each redraw.
fn echo_keys(s: &mut Session, n: usize) -> Result<Vec<Duration>, String> {
    let mut times = Vec::with_capacity(n);
    for i in 0..n {
        let want: &[u8] = if i % 2 == 0 { b"OOOOOOOO" } else { b"XXXXXXXX" };
        s.expect(want);
        let t0 = Instant::now();
        s.send(b"k");
        let t1 = s
            .seen(Duration::from_secs(5))
            .ok_or(format!("key {i}: no echo within 5 s"))?;
        times.push(t1.saturating_duration_since(t0));
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(times)
}

/// A one-commit repository, so sessions start in a new worktree by default.
fn init_repo(dir: &Path) -> Result<(), String> {
    let run = |args: &[&str]| -> Result<(), String> {
        let o = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&o.stderr).into_owned())
        }
    };
    run(&["init", "-q"])?;
    std::fs::write(dir.join("README.md"), "bench\n").map_err(|e| e.to_string())?;
    run(&["add", "."])?;
    run(&[
        "-c",
        "user.name=bench",
        "-c",
        "user.email=bench@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "init",
    ])
}

/// `diag-sessions`: with `FALLOW_DIAG_SAMPLE=DIR`, profile Fallow while the keys are typed
/// (`sample` on macOS, which needs no privileges for one's own processes) and record the
/// per-process CPU (`ps`) once the flood is running.
fn diag_start(pid: u32) -> Option<(PathBuf, Option<std::process::Child>)> {
    let dir = PathBuf::from(std::env::var_os("FALLOW_DIAG_SAMPLE")?);
    let _ = std::fs::create_dir_all(&dir);
    let child = if cfg!(target_os = "macos") {
        std::process::Command::new("sample")
            .args([
                &pid.to_string(),
                "6",
                "-file",
                &dir.join("fallow-sample.txt").display().to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
    } else {
        None
    };
    if cfg!(unix) {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(o) = std::process::Command::new("ps")
            .args(["-A", "-o", "pid,ppid,%cpu,comm"])
            .output()
        {
            let _ = std::fs::write(dir.join("ps.txt"), o.stdout);
        }
    }
    Some((dir, child))
}

fn diag_finish(
    diag: Option<(PathBuf, Option<std::process::Child>)>,
    r: &Result<Vec<Duration>, String>,
) {
    let Some((dir, child)) = diag else {
        return;
    };
    if let Some(mut c) = child {
        let _ = c.wait();
    }
    if let Ok(times) = r {
        let list: Vec<String> = times
            .iter()
            .map(|t| format!("{:.1}", pty::ms(*t)))
            .collect();
        let _ = std::fs::write(dir.join("flood-echo-ms.txt"), list.join("\n"));
    }
}
