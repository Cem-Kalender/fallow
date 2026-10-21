//! 003 T016: the channel and who may use it (FR-002, FR-003, research R-2, R-3). Runs on all
//! three OSes. The full bridge-from-outside refusal runs in `cargo xtask orchestration`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use fallow::app::events::AppEvent;
use fallow::orchestration::channel::{self as ch, Incoming};
use fallow::orchestration::identity::Peer;
use fallow::orchestration::inject::Bridge;
use fallow::orchestration::{Sessions, tools};
use fallow::session::profiles::{AgentProfile, Style};
use fallow::session::{SessionManager, StartRequest};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-orch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::path::absolute(&d).unwrap()
}

fn recv_hub(rx: &std::sync::mpsc::Receiver<AppEvent>) -> Incoming {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Ok(AppEvent::Hub(i)) = rx.recv_timeout(Duration::from_millis(100)) {
            return i;
        }
    }
    panic!("no channel event");
}

#[test]
fn a_connection_carries_the_kernel_reported_peer_and_mcp_lines() {
    let dir = temp("chan").join("run");
    let (tx, rx) = channel();
    let c = ch::open(&dir, tools::list(), tx).expect("open the channel");
    let endpoint = c.endpoint().to_string();
    if cfg!(unix) {
        assert!(endpoint.ends_with(&format!("{}.sock", std::process::id())));
    } else {
        assert!(endpoint.starts_with(r"\\.\pipe\fallow-"));
    }
    let client = std::thread::spawn(move || {
        let (r, mut w) = ch::connect(&endpoint, std::process::id()).expect("connect");
        let mut r = BufReader::new(r);
        writeln!(w, r#"{{"session":7,"token":"{}"}}"#, "ab".repeat(32)).unwrap();
        writeln!(
            w,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"t","version":"9"}}}}}}"#
        )
        .unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("2025-11-25"), "{line}");
        writeln!(
            w,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"whoami"}}}}"#
        )
        .unwrap();
        line.clear();
        r.read_line(&mut line).unwrap();
        line
    });
    let Incoming::Hello {
        session,
        token,
        peer,
        verdict,
        out,
        ..
    } = recv_hub(&rx)
    else {
        panic!("expected the hello first");
    };
    assert_eq!(session, 7);
    assert_eq!(token, "ab".repeat(32));
    assert_eq!(peer.pid, std::process::id(), "the peer is this process");
    #[cfg(unix)]
    assert_eq!(peer.uid, Some(fallow::orchestration::identity::my_uid()));
    verdict.send(Ok(())).unwrap();
    assert!(matches!(recv_hub(&rx), Incoming::Client { ref name, .. } if name == "t"));
    let Incoming::Call { id, name, .. } = recv_hub(&rx) else {
        panic!("expected the tool call");
    };
    assert_eq!(name, "whoami");
    out.send(ch::Outgoing::Line(fallow::orchestration::rpc::result_line(
        &id,
        serde_json::json!({"ok": true}),
    )))
    .unwrap();
    let reply = client.join().unwrap();
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    drop(c);
    if cfg!(unix) {
        assert!(
            std::fs::read_dir(&dir).unwrap().next().is_none(),
            "the socket is removed when the channel closes"
        );
    }
}

#[test]
fn a_refused_hello_closes_the_connection() {
    let dir = temp("refuse").join("run");
    let (tx, rx) = channel();
    let c = ch::open(&dir, tools::list(), tx).unwrap();
    let endpoint = c.endpoint().to_string();
    let client = std::thread::spawn(move || {
        let (r, mut w) = ch::connect(&endpoint, std::process::id()).unwrap();
        writeln!(w, r#"{{"session":1,"token":"{}"}}"#, "00".repeat(32)).unwrap();
        let mut line = String::new();
        BufReader::new(r).read_line(&mut line).unwrap_or(0)
    });
    let Incoming::Hello { verdict, .. } = recv_hub(&rx) else {
        panic!()
    };
    verdict.send(Err("no".into())).unwrap();
    assert_eq!(client.join().unwrap(), 0, "closed without a byte");
}

#[cfg(unix)]
#[test]
fn the_run_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let base = temp("private");
    let dir = base.join("run");
    ch::private_dir(&dir).unwrap();
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ch::private_dir(&dir).unwrap_err().contains("700"));
    let link = base.join("link");
    std::os::unix::fs::symlink(&base, &link).unwrap();
    assert!(ch::private_dir(&link).is_err(), "a symlink is refused");
}

fn long_profile(style: Style) -> AgentProfile {
    let (command, args): (&str, &[&str]) = if cfg!(windows) {
        ("ping.exe", &["-n", "120", "127.0.0.1"])
    } else {
        ("/bin/sleep", &["120"])
    };
    AgentProfile {
        id: "t".into(),
        name: "Test".into(),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: Vec::new(),
        builtin: false,
        orchestration: style,
    }
}

#[test]
fn only_a_sessions_own_processes_with_its_token_are_accepted() {
    if fallow::orchestration::inject::gemini_system_settings_exist() {
        eprintln!("skipped: a Gemini system settings file exists on this machine");
        return;
    }
    let dir = temp("verify");
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let mut m = SessionManager::default();
    m.bridge = Some(Bridge {
        exe: PathBuf::from("fallow"),
        endpoint: "unused".into(),
        fallow_pid: std::process::id(),
        run_dir: Some(run.clone()),
    });
    let (tx, _rx) = channel();
    let req = |p| StartRequest {
        project: 1,
        project_name: "p".into(),
        cwd: dir.clone(),
        workdir: fallow::session::Workdir::ProjectFolder,
        profile: p,
        rows: 24,
        cols: 80,
    };
    // The Gemini style adds only an environment variable and a file, so ping/sleep still run.
    let a = m.start(req(long_profile(Style::Gemini)), &tx).unwrap();
    let b = m.start(req(long_profile(Style::Gemini)), &tx).unwrap();
    let plain = m.start(req(long_profile(Style::None)), &tx).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let tok = |id| m.get(id).unwrap().orch.token.clone().unwrap().hex();
    let pid = |id| m.get(id).unwrap().pid.unwrap();
    let me = |p: u32| Peer {
        pid: p,
        #[cfg(unix)]
        uid: Some(fallow::orchestration::identity::my_uid()),
        #[cfg(windows)]
        uid: None,
    };
    assert!(
        run.join(format!("gemini-{a}.json")).exists(),
        "per-run settings file"
    );
    assert_eq!(m.verify(a, &tok(a), me(pid(a))), Ok(()));
    assert_eq!(m.verify(b, &tok(b), me(pid(b))), Ok(()));
    // Another session's process with this session's token.
    assert!(m.verify(a, &tok(a), me(pid(b))).is_err());
    // This session's process with another session's token.
    assert!(m.verify(a, &tok(b), me(pid(a))).is_err());
    // A process outside every session (this test) with a valid token.
    assert!(m.verify(a, &tok(a), me(std::process::id())).is_err());
    // A session without orchestration has no token at all.
    assert!(m.get(plain).unwrap().orch.token.is_none());
    assert_eq!(
        m.get(plain).unwrap().orch.unsupported.as_deref(),
        Some("profile")
    );
    #[cfg(unix)]
    assert!(
        m.verify(
            a,
            &tok(a),
            Peer {
                pid: pid(a),
                uid: Some(u32::MAX - 1)
            }
        )
        .is_err(),
        "another user's process"
    );
    let mut d = fallow::ui::confirm::Dialog::new("t", "quit", vec![], "OK");
    match d.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    )) {
        fallow::ui::confirm::Outcome::Confirmed(t) => m.stop_all_confirmed(t),
        o => panic!("{o:?}"),
    }
    for id in [a, b, plain] {
        m.exited(id, None);
    }
    assert!(
        !run.join(format!("gemini-{a}.json")).exists(),
        "the per-run file is removed when the agent exits"
    );
}

#[cfg(unix)]
#[test]
fn a_process_orphaned_inside_the_session_is_still_accepted() {
    // research R-3: a bridge whose parent (an npx or shell wrapper) exited is reparented, so
    // walking parents fails; it still belongs to the agent's POSIX session.
    if fallow::orchestration::inject::gemini_system_settings_exist() {
        eprintln!("skipped: a Gemini system settings file exists on this machine");
        return;
    }
    let dir = temp("orphan");
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let pidfile = dir.join("orphan.pid");
    let mut m = SessionManager::default();
    m.bridge = Some(Bridge {
        exe: PathBuf::from("fallow"),
        endpoint: "unused".into(),
        fallow_pid: std::process::id(),
        run_dir: Some(run),
    });
    let (tx, _rx) = channel();
    let script = format!("(sleep 30 & echo $! > {}); sleep 30", pidfile.display());
    let profile = AgentProfile {
        id: "t".into(),
        name: "Test".into(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script],
        env: Vec::new(),
        builtin: false,
        orchestration: Style::Gemini,
    };
    let id = m
        .start(
            StartRequest {
                project: 1,
                project_name: "p".into(),
                cwd: dir.clone(),
                workdir: fallow::session::Workdir::ProjectFolder,
                profile,
                rows: 24,
                cols: 80,
            },
            &tx,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let orphan: u32 = loop {
        if let Some(p) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|t| t.trim().parse().ok())
        {
            break p;
        }
        assert!(Instant::now() < until, "the orphan never started");
        std::thread::sleep(Duration::from_millis(50));
    };
    let token = m.get(id).unwrap().orch.token.clone().unwrap().hex();
    let peer = Peer {
        pid: orphan,
        uid: Some(fallow::orchestration::identity::my_uid()),
    };
    assert_eq!(m.verify(id, &token, peer), Ok(()), "same POSIX session");
    let _ = std::process::Command::new("kill")
        .arg(orphan.to_string())
        .status();
    m.exited(id, None);
}
