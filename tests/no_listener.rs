//! 014 (FR-015, research R-16, T027): without the `automation` build, Fallow's service listens on
//! no port: the Studio reaches nothing but Fallow's own window. Runs `fallow serve` with its own
//! data and config folders, then reads the system's table of listening sockets for its process.

#[cfg(all(windows, not(feature = "automation")))]
#[test]
fn the_service_listens_on_no_port() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let root = std::env::temp_dir().join(format!("fallow-no-listener-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .args(["serve", "--config-dir"])
        .arg(root.join("config"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let err = child.stderr.take().unwrap();
    let mut started = false;
    let mut already = false;
    for line in BufReader::new(err).lines().map_while(Result::ok) {
        if line.contains("already running") {
            already = true;
            break;
        }
        if line.starts_with("Fallow is running") {
            assert!(!line.contains("127.0.0.1"), "no page address: {line}");
            started = true;
            break;
        }
    }
    if already {
        // One service per user: another Fallow runs on this machine (a developer's).
        let _ = child.kill();
        eprintln!("skipped: another Fallow service runs for this user");
        return;
    }
    assert!(started, "the service did not start");
    let mut listening = Vec::new();
    for proto in ["TCP", "TCPv6", "UDP", "UDPv6"] {
        let out = Command::new("netstat")
            .args(["-ano", "-p", proto])
            .output()
            .unwrap();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let owner = cols.last().and_then(|p| p.parse::<u32>().ok());
            let udp = proto.starts_with("UDP");
            if owner == Some(pid) && (udp || line.contains("LISTENING")) {
                listening.push(line.trim().to_string());
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&root);
    assert!(listening.is_empty(), "the service listens: {listening:?}");
}
