//! `cargo xtask bench-sessions` (002 T026, SC-003, research R-9).
//!
//! Fallow runs in the PTY harness with a project and one profile: the xtask `echo-agent`
//! persona (other profiles disabled in a temporary config directory). The project is a git
//! repository, so `n`, `Enter` takes the default choice and starts the agent in a new worktree
//! (US6); the time from `Enter` to the agent's first output is reported (informational). The
//! harness then types 100 keys. For each, the time from the key to the
//! agent's redraw appearing on Fallow's screen is measured. Fails if the p95 is 50 ms or more.
//! A run where the session never starts, or Fallow's identity checks fail, is INVALID.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::pty::{self, Session};

const KEYS: usize = 100;
const BUDGET: Duration = Duration::from_millis(50);

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
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("bench-sessions: {}: {e}", d.display());
            return 2;
        }
    }
    let me = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bench-sessions: {e}");
            return 2;
        }
    };
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    profiles.push_str(&format!(
        "[[profile]]\nid = \"echo\"\nname = \"Echo\"\ncommand = {:?}\nargs = [\"echo-agent\"]\n",
        me.display().to_string()
    ));
    if let Err(e) = std::fs::write(config.join("profiles.toml"), profiles) {
        eprintln!("bench-sessions: profiles.toml: {e}");
        return 2;
    }
    if let Err(e) = init_repo(&project) {
        eprintln!("bench-sessions: INVALID: cannot create the test repository: {e}");
        return 2;
    }
    println!(
        "bench-sessions: {} ({KEYS} keys, budget echo p95 < {} ms)",
        bin.display(),
        BUDGET.as_millis()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = match Session::spawn(&bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bench-sessions: {e}");
            return 2;
        }
    };
    let result = measure(&mut s);
    // Leave the session (Ctrl-]) so Ctrl-Q reaches Fallow, then quit.
    s.send(b"\x1d");
    std::thread::sleep(Duration::from_millis(300));
    let outcome = s.finish();
    let _ = std::fs::remove_dir_all(&base);
    let mut problems = outcome.problems(None);
    let (start, times) = match result {
        Ok(t) => t,
        Err(e) => {
            problems.push(e);
            (Duration::ZERO, Vec::new())
        }
    };
    println!("evidence: {}", outcome.evidence());
    if !problems.is_empty() {
        for p in &problems {
            println!("invalid: {p}");
        }
        println!("os={} result=INVALID", std::env::consts::OS);
        pty::step_summary(&format!(
            "### Session echo latency ({})\n\nINVALID: {}\n",
            std::env::consts::OS,
            problems.join("; ")
        ));
        return 2;
    }
    let mut sorted = times.clone();
    sorted.sort();
    let p95 = sorted[((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1];
    let median = sorted[sorted.len() / 2];
    let pass = p95 < BUDGET;
    let result = if pass { "PASS" } else { "FAIL" };
    println!(
        "os={} worktree_start_ms={:.1} keys={} median_ms={:.1} p95_ms={:.1} max_ms={:.1} budget_ms={} result={result}",
        std::env::consts::OS,
        pty::ms(start),
        sorted.len(),
        pty::ms(median),
        pty::ms(p95),
        pty::ms(*sorted.last().unwrap_or(&Duration::ZERO)),
        BUDGET.as_millis()
    );
    pty::step_summary(&format!(
        "### Session echo latency ({})\n\n| session start in a new worktree | median | p95 | budget | result |\n|---|---|---|---|---|\n| {:.0} ms | {:.1} ms | {:.1} ms | < {} ms | **{result}** |\n",
        std::env::consts::OS,
        pty::ms(start),
        pty::ms(median),
        pty::ms(p95),
        BUDGET.as_millis()
    ));
    if pass { 0 } else { 1 }
}

/// A one-commit repository, so the session starts in a new worktree by default.
fn init_repo(dir: &std::path::Path) -> Result<(), String> {
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

fn measure(s: &mut Session) -> Result<(Duration, Vec<Duration>), String> {
    if s.wait_frame(Duration::from_secs(10)).is_none() {
        return Err("no first frame".into());
    }
    // Projects has focus with the project selected: `n` opens the profile menu, Enter starts
    // the only profile.
    s.expect(b"echo-ready");
    s.send(b"n");
    std::thread::sleep(Duration::from_millis(300));
    let t0 = Instant::now();
    s.send(b"\r");
    let Some(ready) = s.seen(Duration::from_secs(30)) else {
        return Err("the session never showed echo-ready".into());
    };
    let start = ready.saturating_duration_since(t0);
    std::thread::sleep(Duration::from_millis(300));
    let mut times = Vec::with_capacity(KEYS);
    for i in 0..KEYS {
        let want: &[u8] = if i % 2 == 0 { b"OOOOOOOO" } else { b"XXXXXXXX" };
        s.expect(want);
        let t0 = Instant::now();
        s.send(b"k");
        match s.seen(Duration::from_secs(5)) {
            Some(t1) => times.push(t1.saturating_duration_since(t0)),
            None => return Err(format!("key {i}: no echo within 5 s")),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok((start, times))
}
