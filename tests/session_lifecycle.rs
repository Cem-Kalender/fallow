//! 002 T015: session lifecycle with real processes in a pseudo-terminal (FR-001, FR-006,
//! FR-025, research R-7).

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use fallow::app::events::AppEvent;
use fallow::app::state::AppState;
use fallow::session::profiles::AgentProfile;
use fallow::session::{SessionManager, SessionState, StartRequest};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-life-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::path::absolute(&d).unwrap()
}

fn profile(command: &str, args: &[&str], env: &[(&str, &str)]) -> AgentProfile {
    AgentProfile {
        id: "test".into(),
        name: "Test".into(),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        builtin: false,
        orchestration: Default::default(),
    }
}

/// Prints a marker with an env addition, prints the working directory, exits with 3.
fn echo_profile() -> AgentProfile {
    if cfg!(windows) {
        profile(
            "cmd.exe",
            &["/d", "/c", "echo marker-%FALLOW_T%& cd & exit 3"],
            &[("FALLOW_T", "abc")],
        )
    } else {
        profile(
            "/bin/sh",
            &["-c", "echo marker-$FALLOW_T; pwd; exit 3"],
            &[("FALLOW_T", "abc")],
        )
    }
}

fn long_profile() -> AgentProfile {
    if cfg!(windows) {
        profile("ping.exe", &["-n", "120", "127.0.0.1"], &[])
    } else {
        profile("/bin/sleep", &["120"], &[])
    }
}

fn req(dir: &std::path::Path, p: AgentProfile) -> StartRequest {
    StartRequest {
        project: 1,
        project_name: "p".into(),
        cwd: dir.to_path_buf(),
        workdir: fallow::session::Workdir::ProjectFolder,
        profile: p,
        rows: 24,
        cols: 100,
    }
}

/// Waits for the session to exit, handling output events like the main loop does.
fn wait_exit(m: &mut SessionManager, rx: &Receiver<AppEvent>, id: u32) -> Option<u32> {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(AppEvent::SessionOutput(i)) => {
                m.output(i);
            }
            Ok(AppEvent::SessionExited(i, code)) if i == id => {
                m.exited(i, code);
                return code;
            }
            _ => {}
        }
    }
    panic!("session {id} did not exit in time");
}

fn screen_text(m: &SessionManager, id: u32) -> String {
    let s = m.get(id).unwrap();
    let mut t = s.term.lock().unwrap();
    (0..t.total_lines())
        .map(|v| t.line_text(v))
        .collect::<Vec<_>>()
        .join("\n")
}

fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    } else {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[test]
fn nothing_is_started_at_startup() {
    let d = temp("startup");
    let s = AppState::new(Some(d.clone()), Vec::new());
    assert_eq!(s.sessions.started, 0, "FR-025");
    assert!(s.sessions.sessions.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_session_runs_in_the_project_with_env_additions_and_reports_its_exit() {
    let d = temp("run");
    let (tx, rx) = channel();
    let mut m = SessionManager::default();
    let id = m.start(req(&d, echo_profile()), &tx).unwrap();
    let code = wait_exit(&mut m, &rx, id);
    // Give the reader thread a moment to deliver the last bytes.
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(AppEvent::SessionOutput(i)) = rx.try_recv() {
        m.output(i);
    }
    assert_eq!(code, Some(3));
    let s = m.get(id).unwrap();
    assert_eq!(s.state, SessionState::Exited(Some(3)));
    assert!(s.pty.is_none(), "the pseudo-terminal is released");
    let text = screen_text(&m, id);
    assert!(text.contains("marker-abc"), "env addition passed\n{text}");
    let dir_name = d.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        text.contains(&dir_name),
        "runs in the project folder\n{text}"
    );

    // Restart: same profile and project, a fresh terminal.
    m.restart(id, &tx).unwrap();
    assert_eq!(wait_exit(&mut m, &rx, id), Some(3));
    assert_eq!(m.started, 2);
    // Close only exited sessions.
    m.close(id).unwrap();
    assert!(m.get(id).is_none());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_missing_command_fails_in_its_own_session() {
    let d = temp("missing");
    let (tx, _rx) = channel();
    let mut m = SessionManager::default();
    let id = m
        .start(req(&d, profile("no-such-agent-xyz", &[], &[])), &tx)
        .unwrap();
    match &m.get(id).unwrap().state {
        SessionState::Failed(why) => assert!(why.contains("not found"), "{why}"),
        s => panic!("{s:?}"),
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_session_limit_is_enforced() {
    let d = temp("limit");
    let (tx, rx) = channel();
    let mut m = SessionManager::default();
    m.limits.max_sessions = 1;
    let id = m.start(req(&d, long_profile()), &tx).unwrap();
    let e = m.start(req(&d, long_profile()), &tx).unwrap_err();
    assert!(e.contains("limit"), "{e}");
    assert!(m.close(id).is_err(), "a running session can't be closed");
    drop(m);
    drop(rx);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn dropping_the_sessions_ends_their_agents() {
    let d = temp("drop");
    let (tx, _rx) = channel();
    let mut m = SessionManager::default();
    let id = m.start(req(&d, long_profile()), &tx).unwrap();
    let pid = m.get(id).unwrap().pty.as_ref().unwrap().pid.unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(pid), "the agent runs");
    drop(m);
    let until = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive(pid), "no agent outlives its session");
    let _ = std::fs::remove_dir_all(&d);
}

/// A real agent CLI, if installed: `claude --version` through its launcher (an npm `.cmd` shim on
/// Windows) and the pseudo-terminal. No account data is involved.
#[test]
fn claude_code_version_if_installed() {
    if fallow::session::profiles::resolve("claude").is_err() {
        eprintln!("claude not installed; skipped");
        return;
    }
    let d = temp("claude");
    let (tx, rx) = channel();
    let mut m = SessionManager::default();
    let id = m
        .start(req(&d, profile("claude", &["--version"], &[])), &tx)
        .unwrap();
    assert_eq!(wait_exit(&mut m, &rx, id), Some(0));
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(AppEvent::SessionOutput(i)) = rx.try_recv() {
        m.output(i);
    }
    let text = screen_text(&m, id);
    assert!(text.contains("Claude Code"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

/// 002 T055: through the app, the default for a git project is a new worktree; the agent runs
/// there; restart reuses it; closing with Remove removes a clean worktree.
#[test]
fn sessions_of_git_projects_run_in_their_own_worktree() {
    use fallow::app::workspace::Where;
    let base = temp("wt-app");
    let r = base.join("repo");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    git(&r, &["config", "user.email", "t@example.invalid"]);
    git(&r, &["config", "user.name", "t"]);
    std::fs::write(r.join("a.txt"), "x").unwrap();
    git(&r, &["add", "."]);
    git(
        &r,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"],
    );

    let (tx, rx) = channel();
    let mut s = AppState::new(Some(r.clone()), Vec::new());
    s.events = Some(tx);
    s.profiles = vec![echo_profile()];
    s.start_session_at("test", Where::NewWorktree).unwrap();
    let id = s.workspace.active.unwrap();
    let wt = match &s.sessions.get(id).unwrap().workdir {
        fallow::session::Workdir::Worktree(w) => w.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(wt.branch, "fallow/test-1");
    assert!(
        s.sessions
            .get(id)
            .unwrap()
            .label()
            .contains("fallow/test-1")
    );
    assert_eq!(wait_exit(&mut s.sessions, &rx, id), Some(3));
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(AppEvent::SessionOutput(i)) = rx.try_recv() {
        s.sessions.output(i);
    }
    let text = screen_text(&s.sessions, id);
    assert!(text.contains("test-1"), "runs in the worktree\n{text}");
    // Restart reuses the worktree.
    let tx2 = s.events.clone().unwrap();
    s.sessions.restart(id, &tx2).unwrap();
    assert_eq!(s.sessions.get(id).unwrap().cwd, wt.path);
    assert_eq!(wait_exit(&mut s.sessions, &rx, id), Some(3));
    // Close: the clean worktree can be removed; the dialog defaults to Keep.
    s.workspace.focus = fallow::app::workspace::AgentsFocus::Sessions;
    s.run("session.close");
    match &s.overlay {
        fallow::app::state::Overlay::Confirm { dialog, .. } => {
            assert_eq!(dialog.cancel_label, "Keep worktree");
            assert_eq!(dialog.focus, fallow::ui::confirm::Button::Cancel);
        }
        o => panic!("{o:?}"),
    }
    s.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(s.sessions.get(id).is_none(), "closed");
    assert!(!wt.path.exists(), "removed");
    let _ = std::fs::remove_dir_all(&base);
}
