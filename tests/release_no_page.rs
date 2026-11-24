//! 016 (FR-015, FR-018, T038): a macOS build without the `automation` feature has no page: the
//! service starts without a port (also when `FALLOW_AUTOMATION=1` asks for the test build's), and
//! `url` is refused. Built and run without the feature (CI's macOS job runs it so).

#![cfg(all(target_os = "macos", not(feature = "automation")))]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn a_release_on_macos_serves_no_page() {
    let h = PathBuf::from(format!("/tmp/f016np-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .env("HOME", &h)
        .env("FALLOW_AUTOMATION", "1")
        // No window needed for this check.
        .env("FALLOW_NO_DISPLAY", "1")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let data = h.join("Library/Application Support/Fallow");
    let ask = |cmd: &str| -> Option<serde_json::Value> {
        let mut s = UnixStream::connect(data.join("service.sock")).ok()?;
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
    let service: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("service.json")).unwrap()).unwrap();
    assert!(service["port"].is_null(), "{service}");
    let url = ask("url").unwrap();
    assert_eq!(url["ok"], false, "{url}");
    assert!(
        url["error"].as_str().unwrap().contains("Fallow's window"),
        "{url}"
    );
    let _ = ask("stop");
    let end = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&h);
}
