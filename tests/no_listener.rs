//! 014 (FR-015, research R-16, T027): without the `automation` build, Fallow listens on no port:
//! neither its service nor its window's web view, even when asked for the test page and the
//! debugging port. Runs `fallow serve` with its own data and config folders, opens the window when
//! there is a desktop, then reads the system's tables of listening sockets for its processes.

#[cfg(all(windows, not(feature = "automation")))]
#[test]
fn neither_the_service_nor_its_window_listens() {
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
        // A release build must ignore these: they are the test build's (research R-15).
        .env("FALLOW_AUTOMATION", "1")
        .env("FALLOW_AUTOMATION_CDP", "9444")
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
    if already || !started {
        let _ = child.kill();
        let _ = child.wait();
        assert!(already, "the service did not start");
        // One service per user: another Fallow runs (a developer's machine). Never on CI.
        assert!(
            std::env::var_os("CI").is_none(),
            "another Fallow service runs on CI: this gate would check nothing"
        );
        eprintln!("skipped: another Fallow service runs for this user");
        return;
    }
    // The window, when there is a desktop: its web view's processes are checked too.
    let window = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .output()
        .is_ok_and(|o| o.status.success());
    if window {
        std::thread::sleep(std::time::Duration::from_secs(3));
    } else {
        eprintln!("no window here (no desktop or no WebView2): the service alone is checked");
    }
    let tree = process_tree(pid);
    let mut listening = Vec::new();
    for proto in ["TCP", "TCPv6", "UDP", "UDPv6"] {
        let out = Command::new("netstat")
            .args(["-ano", "-p", proto])
            .output()
            .unwrap();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let owner = cols.last().and_then(|p| p.parse::<u32>().ok());
            // A listener: a TCP socket LISTENING, or a UDP one open to anyone (`*:*`); a UDP
            // socket with a remote address is an outgoing one (the trace job's concern).
            let udp_open = proto.starts_with("UDP") && cols.get(2) == Some(&"*:*");
            if owner.is_some_and(|o| tree.contains(&o)) && (udp_open || line.contains("LISTENING"))
            {
                listening.push(line.trim().to_string());
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        listening.is_empty(),
        "Fallow or its window listens: {listening:?}"
    );
}

/// `root` and every process descended from it.
#[cfg(all(windows, not(feature = "automation")))]
fn process_tree(root: u32) -> Vec<u32> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId)\" }",
        ])
        .output()
        .unwrap();
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().map(|v| v.parse::<u32>());
            Some((it.next()?.ok()?, it.next()?.ok()?))
        })
        .collect();
    let mut tree = vec![root];
    let mut i = 0;
    while i < tree.len() {
        let p = tree[i];
        for (c, parent) in &pairs {
            if *parent == p && !tree.contains(c) {
                tree.push(*c);
            }
        }
        i += 1;
    }
    tree
}
