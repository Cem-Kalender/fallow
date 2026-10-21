//! `cargo xtask canary --sessions` (002 T050, FR-026, SC-008): session output stays in memory.
//!
//! A stand-in agent prints a random 32-byte canary into a Fallow session, inside enough other
//! output that it reaches the scrollback. The harness sees the canary on Fallow's screen (so it
//! is in Fallow's memory). On macOS, Fallow's memory is dumped during the session and must hold
//! the canary: that proves the search can see it. After quitting, no file in the config
//! directory, Fallow's platform config, cache and log directories, the project, or files
//! changed in the temporary directory during the run may contain it (UTF-8 or UTF-16LE).
//!
//! The canary reaches the agent through a hand-off file that the agent deletes before printing,
//! so neither Fallow's command line, environment nor configuration ever holds it.
//!
//! Orchestration (003 SC-006): a second, `mcp-agent` session posts the canary to the board,
//! puts it in a task and asks to deliver it (its script lives in the hand-off folder, which is
//! not searched). The harness sees it in the Requests tab. After quit, Fallow's channel socket
//! must be gone from its run folder.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::fixtures;
use crate::pty::Session;

pub fn run(bin: &Path) -> i32 {
    let base = fixtures::temp_dir("canary-sessions");
    let result = check(bin, &base);
    let _ = std::fs::remove_dir_all(&base);
    let (code, result, detail) = match result {
        Ok((hits, dumped)) if hits.is_empty() => (0, "PASS", dumped),
        Ok((hits, dumped)) => {
            for h in &hits {
                println!("canary found in: {h}");
            }
            (1, "FAIL", dumped)
        }
        Err(e) => {
            println!("invalid: {e}");
            (2, "INVALID", "n/a".to_string())
        }
    };
    println!(
        "os={} sessions_canary memory_dump={detail} result={result}",
        std::env::consts::OS
    );
    fixtures::summary(&format!(
        "### Session canary (FR-026, {})\n\n| memory dump during the session | files after quit | result |\n|---|---|---|\n| {detail} | searched | **{result}** |\n",
        std::env::consts::OS
    ));
    code
}

/// Runs one session and returns where the canary was found, and the memory-dump outcome.
fn check(bin: &Path, base: &Path) -> Result<(Vec<String>, String), String> {
    let started = SystemTime::now();
    let config = base.join("config");
    let project = base.join("project");
    let handoff = base.join("handoff");
    for d in [&config, &project, &handoff] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let canary = fixtures::hex(&fixtures::random_bytes(32));
    let file = handoff.join("canary.txt");
    std::fs::write(&file, &canary).map_err(|e| e.to_string())?;

    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    profiles.push_str(&format!(
        "[[profile]]\nid = \"canary\"\nname = \"canary\"\ncommand = {:?}\nargs = [\"canary-agent\", {:?}]\n\n",
        me.display().to_string(),
        file.display().to_string()
    ));
    let script = handoff.join("orch.script");
    std::fs::write(
        &script,
        format!(
            "call board_post {{\"text\":\"{canary}\"}}\ncall task_create {{\"text\":\"{canary}\",\"target\":{{\"session\":1}}}}\ncall deliver {{\"session\":1,\"text\":\"{canary}\"}}\nprint orch-canary-posted\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    profiles.push_str(&format!(
        "[[profile]]\nid = \"orch\"\nname = \"orch\"\ncommand = {:?}\nargs = [\"mcp-agent\", {:?}]\norchestration = \"claude\"\n",
        me.display().to_string(),
        script.display().to_string()
    ));
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;

    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let shown = (|| {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".to_string());
        }
        s.expect(b"canary-ready");
        s.send(b"n");
        std::thread::sleep(Duration::from_millis(300));
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("the canary agent never started")?;
        // The agent deleted the hand-off file before printing.
        if file.exists() {
            return Err("the hand-off file was not deleted".into());
        }
        // The harness matches at most 32 bytes; half the canary is still unique.
        s.expect(&canary.as_bytes()[..32]);
        s.send(b"\r");
        s.seen(Duration::from_secs(10))
            .ok_or("the canary never reached Fallow's screen")?;
        // 003: the canary through the board, a task and a delivery request.
        s.send(b"\x1d");
        std::thread::sleep(Duration::from_millis(500));
        s.expect(b"orch-canary-posted");
        s.send(b"n");
        std::thread::sleep(Duration::from_millis(300));
        s.send(b"\x1b[B");
        std::thread::sleep(Duration::from_millis(100));
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("the orchestrated session never posted")?;
        s.send(b"\x1d");
        std::thread::sleep(Duration::from_millis(500));
        s.expect(&canary.as_bytes()[..32]);
        s.send(b"o");
        s.seen(Duration::from_secs(10))
            .ok_or("the delivery request with the canary never showed")?;
        s.send(b"\x1b");
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    })();
    let dumped = match &shown {
        Ok(()) if cfg!(target_os = "macos") => {
            let core = base.join("fallow.core");
            let d = dump_holds(s.pid, &core, canary.as_bytes());
            let _ = std::process::Command::new("sudo")
                .args(["rm", "-f"])
                .arg(&core)
                .status();
            d
        }
        Ok(()) => "not run (macOS only)".to_string(),
        Err(_) => "n/a".to_string(),
    };
    s.quit_with_sessions();
    let outcome = s.finish();
    shown?;
    // The channel's socket is removed when Fallow quits (Linux, macOS).
    let fallow_pid = outcome.stats.map_or(outcome.pid, |st| st.pid);
    if let Some(run) = run_dir() {
        let sock = run.join(format!("{fallow_pid}.sock"));
        if sock.exists() {
            return Err(format!("{} is still there after quit", sock.display()));
        }
    }
    if !outcome.clean_exit {
        return Err("fallow did not exit cleanly".into());
    }
    if dumped.starts_with("INVALID") {
        return Err(format!("memory dump: {dumped}"));
    }

    let utf8 = canary.as_bytes().to_vec();
    let utf16 = fixtures::utf16le(&canary);
    let mut places: Vec<PathBuf> = vec![config.clone(), project.clone()];
    places.extend(platform_dirs());
    let mut files: Vec<PathBuf> = places.iter().flat_map(|d| walk(d)).collect();
    // Files written anywhere in the temporary directory during the run.
    files.extend(
        walk(&std::env::temp_dir())
            .into_iter()
            .filter(|f| changed_since(f, started) && !f.starts_with(&handoff)),
    );
    files.sort();
    files.dedup();
    // Control: the search finds the canary where it is planted (in UTF-8 and UTF-16LE).
    let control = handoff.join("control.bin");
    let mut planted = b"padding ".to_vec();
    planted.extend_from_slice(&utf8);
    planted.extend_from_slice(&utf16);
    std::fs::write(&control, planted).map_err(|e| e.to_string())?;
    for needle in [&utf8, &utf16] {
        if !fixtures::file_contains(&control, needle).unwrap_or(false) {
            return Err("the control file's canary was not found: the search is broken".into());
        }
    }
    let mut hits = Vec::new();
    for f in &files {
        for (what, needle) in [("UTF-8", &utf8), ("UTF-16LE", &utf16)] {
            if fixtures::file_contains(f, needle).unwrap_or(false) {
                hits.push(format!("{} ({what})", f.display()));
            }
        }
    }
    println!(
        "canary: {} files searched after quit, memory dump: {dumped}",
        files.len()
    );
    Ok((hits, dumped))
}

/// macOS: dumps Fallow's memory with lldb and reports whether it holds `needle`.
fn dump_holds(pid: u32, core: &Path, needle: &[u8]) -> String {
    if !crate::canary::save_core(pid, core, needle) {
        return "INVALID: no core dump holding the session's screen".to_string();
    }
    "canary present in live memory (expected)".to_string()
}

/// Fallow's own platform directories (config, cache, logs), when they exist.
fn platform_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut out = Vec::new();
    if cfg!(windows) {
        out.extend(env("APPDATA").map(|d| d.join("fallow")));
        out.extend(env("LOCALAPPDATA").map(|d| d.join("fallow")));
    } else if let Some(home) = env("HOME") {
        if cfg!(target_os = "macos") {
            for d in [
                "Library/Application Support/fallow",
                "Library/Caches/fallow",
                "Library/Logs/fallow",
            ] {
                out.push(home.join(d));
            }
        } else {
            for d in [".config/fallow", ".cache/fallow", ".local/state/fallow"] {
                out.push(home.join(d));
            }
        }
    }
    out
}

/// Fallow's run folder (`<cache>/fallow/run`), where its channel socket lives on Linux and
/// macOS.
fn run_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        return None;
    }
    let home = env("HOME")?;
    let cache = if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        env("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".cache"))
    };
    Some(cache.join("fallow").join("run"))
}

fn changed_since(f: &Path, t: SystemTime) -> bool {
    f.metadata()
        .and_then(|m| m.modified())
        .is_ok_and(|m| m >= t)
}

/// Every file under `dir` (unreadable entries are skipped).
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                out.push(e.path());
            }
        }
    }
    out
}

/// `canary-agent <file>`: reads the canary from `file`, deletes the file, prints
/// `canary-ready`, then 300 lines with the canary on some of them (so it reaches the
/// scrollback) and the canary again on the last line, and waits for input.
pub fn agent(file: &str) -> Option<i32> {
    use std::io::{Read, Write};
    let canary = std::fs::read_to_string(file).ok()?;
    let _ = std::fs::remove_file(file);
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[2J\x1b[Hcanary-ready\r\n");
    let _ = out.flush();
    let mut b = [0u8; 64];
    // Wait for the harness's Enter before printing, so it is watching for the canary.
    let _ = std::io::stdin().read(&mut b);
    for i in 0..300 {
        if i % 50 == 0 {
            let _ = write!(out, "line {i} {canary}\r\n");
        } else {
            let _ = write!(out, "line {i} lorem ipsum dolor sit amet\r\n");
        }
    }
    let _ = write!(out, "{canary}\r\n");
    let _ = out.flush();
    loop {
        match std::io::stdin().read(&mut b) {
            Ok(0) | Err(_) => return Some(0),
            Ok(_) => {}
        }
    }
}
