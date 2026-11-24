//! 016 (SC-007, research M-16, T039): a release `Fallow.app` with its window open listens on no
//! port: neither Fallow's process nor the WebKit processes serving its window.
//!
//!   cargo xtask no-listener --app target/macos/Fallow.app
//!       starts the app through LaunchServices (`open -a`), as the engineer does (CI);
//!   cargo xtask no-listener --app <Fallow.app> --direct --home /tmp/f016nl
//!       starts its binary directly with a throwaway HOME (on a developer's Mac: LaunchServices
//!       would use the real HOME and its data folder).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::mac_procs;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn ask(sock: &Path, cmd: &str) -> Result<serde_json::Value, String> {
    let mut s = UnixStream::connect(sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    writeln!(s, "{{\"kind\":\"cli\",\"request\":{{\"cmd\":\"{cmd}\"}}}}")
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))
}

pub fn run(args: &[String]) -> Result<(), String> {
    // Absolute: `open -a` takes a relative path for an app's name (CI's first run).
    let app = PathBuf::from(
        arg(args, "--app")
            .ok_or("usage: no-listener --app <Fallow.app> [--direct --home <dir>]")?,
    );
    let app = app
        .canonicalize()
        .map_err(|e| format!("{}: {e}", app.display()))?;
    let exe = app.join("Contents/MacOS/fallow");
    if !exe.is_file() {
        return Err(format!("no {}", exe.display()));
    }
    let direct = args.iter().any(|a| a == "--direct");
    let home = match arg(args, "--home") {
        Some(h) => PathBuf::from(h),
        None if direct => return Err("--direct needs --home <dir>".into()),
        None => PathBuf::from(std::env::var("HOME").map_err(|_| "no HOME")?),
    };
    let sock = home.join("Library/Application Support/Fallow/service.sock");
    if ask(&sock, "status").is_ok() {
        return Err("a Fallow already runs for this HOME: stop it first".into());
    }
    let before = mac_procs::webkit_now();
    let mut child = None;
    if direct {
        std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
        child = Some(
            Command::new(&exe)
                .env("HOME", &home)
                .env("__CFBundleIdentifier", "dev.fallow.Fallow")
                // A release must ignore what asks the test build for its page (014 R-15).
                .env("FALLOW_AUTOMATION", "1")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("cannot start {}: {e}", exe.display()))?,
        );
    } else {
        let s = Command::new("/usr/bin/open")
            .arg("-a")
            .arg(&app)
            .status()
            .map_err(|e| e.to_string())?;
        if !s.success() {
            return Err(format!("open -a {} failed", app.display()));
        }
    }
    let direct_pid = child.as_ref().map(|c| c.id() as i32);
    let result = check(&exe, &sock, direct_pid, &before);
    // Stopped however the check ended (review): through its socket, else killed.
    let _ = ask(&sock, "stop");
    if let Some(mut c) = child {
        let end = Instant::now() + Duration::from_secs(15);
        while c.try_wait().ok().flatten().is_none() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = c.kill();
        let _ = c.wait();
    }
    result
}

/// Finds Fallow's process and its WebKit processes once its window is open, and fails on any
/// listening socket among them.
fn check(
    exe: &Path,
    sock: &Path,
    direct_pid: Option<i32>,
    before: &std::collections::HashSet<i32>,
) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs(30);
    while ask(sock, "status").is_err() {
        if Instant::now() > end {
            return Err("Fallow did not start within 30 s".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let pid = match direct_pid {
        // The one this check started (review: another instance of the bundle may run).
        Some(p) => p,
        None => {
            let exe_text = exe
                .canonicalize()
                .unwrap_or(exe.to_path_buf())
                .display()
                .to_string();
            mac_procs::all_pids()
                .into_iter()
                .find(|p| {
                    let mut b = vec![0u8; 4096];
                    // SAFETY: the buffer and its length.
                    let n =
                        unsafe { libc::proc_pidpath(*p, b.as_mut_ptr().cast(), b.len() as u32) };
                    n > 0 && String::from_utf8_lossy(&b[..n as usize]) == exe_text
                })
                .ok_or("cannot find Fallow's process")?
        }
    };
    // The window opens at start; its web content process follows.
    let end = Instant::now() + Duration::from_secs(30);
    let mut helpers = mac_procs::webkit_of(pid, before);
    while !helpers.iter().any(|(_, n)| n.contains("WebContent")) {
        if Instant::now() > end {
            return Err(format!(
                "the window's web content process did not appear (found: {helpers:?})"
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
        helpers = mac_procs::webkit_of(pid, before);
    }
    // Sampled twice, a few seconds apart: a socket opened later is seen too.
    let mut seen = String::new();
    for _ in 0..2 {
        std::thread::sleep(Duration::from_secs(3));
        helpers = mac_procs::webkit_of(pid, before);
        let mut pids = vec![pid];
        pids.extend(helpers.iter().map(|(p, _)| *p));
        let list = pids
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        // A positive control first: lsof can read these processes at all (review: an empty
        // answer must not pass for "no socket").
        let all = Command::new("/usr/sbin/lsof")
            .args(["-nP", "-a", "-p", &list])
            .output()
            .map_err(|e| format!("lsof: {e}"))?;
        if String::from_utf8_lossy(&all.stdout).lines().count() < 2 {
            return Err(format!(
                "lsof could not read Fallow's processes: {}",
                String::from_utf8_lossy(&all.stderr)
            ));
        }
        let out = Command::new("/usr/sbin/lsof")
            .args(["-nP", "-iTCP", "-iUDP", "-a", "-p", &list])
            .output()
            .map_err(|e| format!("lsof: {e}"))?;
        seen.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    let mb = mac_procs::total_mb(pid, before);
    println!(
        "Fallow {pid} and its WebKit processes: {helpers:?}; memory {mb:.1} MB (physical footprint)"
    );
    println!(
        "their network sockets:\n{}",
        if seen.trim().is_empty() {
            "(none)"
        } else {
            &seen
        }
    );
    let listening: Vec<&str> = seen.lines().filter(|l| l.contains("(LISTEN)")).collect();
    if !listening.is_empty() {
        return Err(format!("listening sockets: {listening:?}"));
    }
    // Fallow's own process has no network socket at all (it makes no request, V).
    let own: Vec<&str> = seen
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some(pid.to_string().as_str()))
        .collect();
    if !own.is_empty() {
        return Err(format!("Fallow's own process has network sockets: {own:?}"));
    }
    println!("no listening socket (SC-007)");
    Ok(())
}
