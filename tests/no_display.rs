//! 016 (US5 AS4, FR-029, T040): without a display (as over SSH; `FALLOW_NO_DISPLAY=1` stands in
//! for one in tests), `fallow` explains and exits 1 instead of opening anything, and `fallow serve`
//! runs the engine without AppKit: the service answers a request for its window with the reason.

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn home(name: &str) -> PathBuf {
    let h = PathBuf::from(format!("/tmp/f016nd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

#[test]
fn fallow_without_a_display_explains_and_opens_nothing() {
    let h = home("cli");
    let out = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .env("HOME", &h)
        .env("FALLOW_NO_DISPLAY", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("the Mac's desktop"), "{err}");
    assert!(err.contains("fallow serve"), "{err}");
    // Nothing was started.
    assert!(
        !h.join("Library/Application Support/Fallow/service.sock")
            .exists()
    );
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn the_service_without_a_display_runs_and_says_why_it_has_no_window() {
    let h = home("serve");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .env("HOME", &h)
        .env("FALLOW_NO_DISPLAY", "1")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let sock = h.join("Library/Application Support/Fallow/service.sock");
    let ask = |cmd: &str| -> Option<serde_json::Value> {
        let mut s = UnixStream::connect(&sock).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(30))).ok();
        writeln!(s, "{{\"kind\":\"cli\",\"request\":{{\"cmd\":\"{cmd}\"}}}}").ok()?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    };
    let end = Instant::now() + Duration::from_secs(20);
    while ask("status").is_none() {
        assert!(Instant::now() < end, "the service did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    // The engine answers (it runs on the main thread); AppKit does not run, so no window.
    let w = ask("window").unwrap();
    assert_eq!(w["ok"], false, "{w}");
    assert!(
        w["error"]
            .as_str()
            .unwrap_or("")
            .contains("started without a display"),
        "{w}"
    );
    assert_eq!(ask("stop").unwrap()["ok"], true);
    let end = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < end, "the service did not stop");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&h);
}
