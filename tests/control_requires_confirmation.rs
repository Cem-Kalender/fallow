//! Control paths need confirmation (constitution IV; 001 SC-008; 002 T028, FR-012).
//!
//! 002: stopping a session, force-stopping it, and quitting with sessions running are each
//! unreachable without a `ConfirmedToken` for exactly that action, and the only way to get a
//! token is to confirm the dialog (`ui::confirm`, whose token has no public constructor).
//! 001's own process-control cases (T094) join this file when Phase 8 lands.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fallow::session::profiles::AgentProfile;
use fallow::session::{
    SessionManager, SessionState, StartRequest, Workdir, force_subject, stop_subject,
};
use fallow::ui::confirm::{Dialog, Outcome};

fn token_for(subject: &str) -> fallow::ui::confirm::ConfirmedToken {
    let mut d = Dialog::new("t", subject, vec![], "OK");
    match d.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)) {
        Outcome::Confirmed(t) => t,
        o => panic!("{o:?}"),
    }
}

fn long_running(m: &mut SessionManager) -> u32 {
    let (tx, _rx) = channel();
    std::mem::forget(_rx);
    let profile = if cfg!(windows) {
        AgentProfile {
            id: "p".into(),
            name: "p".into(),
            command: "ping.exe".into(),
            args: vec!["-n".into(), "120".into(), "127.0.0.1".into()],
            env: vec![],
            builtin: false,
        }
    } else {
        AgentProfile {
            id: "p".into(),
            name: "p".into(),
            command: "/bin/sleep".into(),
            args: vec!["120".into()],
            env: vec![],
            builtin: false,
        }
    };
    m.start(
        StartRequest {
            project: 1,
            project_name: "p".into(),
            cwd: std::env::temp_dir(),
            workdir: Workdir::ProjectFolder,
            profile,
            rows: 10,
            cols: 40,
        },
        &tx,
    )
    .unwrap()
}

#[test]
fn stopping_needs_a_token_for_that_session() {
    let mut m = SessionManager::default();
    let a = long_running(&mut m);
    let b = long_running(&mut m);
    // A token for another session, or for a force stop, does not stop this one.
    assert!(m.stop(a, token_for(&stop_subject(b))).is_err());
    assert!(m.stop(a, token_for(&force_subject(a))).is_err());
    assert!(m.stop(a, token_for("quit")).is_err());
    assert!(m.get(a).unwrap().state.running());
    assert_ne!(m.get(a).unwrap().state, SessionState::Stopping);
    // The right token stops it gracefully.
    m.stop(a, token_for(&stop_subject(a))).unwrap();
    assert_eq!(m.get(a).unwrap().state, SessionState::Stopping);
    assert!(m.get(a).unwrap().stop_requested.is_some());
}

#[test]
fn force_stop_needs_its_own_token() {
    let mut m = SessionManager::default();
    let a = long_running(&mut m);
    let pid = m.get(a).unwrap().pid.unwrap();
    assert!(m.force_stop(a, token_for(&stop_subject(a))).is_err());
    m.force_stop(a, token_for(&force_subject(a))).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let alive = |pid: u32| {
        if cfg!(windows) {
            let o = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
        } else {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .is_ok_and(|s| s.success())
        }
    };
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive(pid), "force stop ends the agent");
}

#[test]
fn quitting_with_sessions_asks_once_and_lists_them() {
    use fallow::app::editing::Pending;
    use fallow::app::state::{AppState, Overlay};
    let mut s = AppState::new(None, Vec::new());
    let a = long_running(&mut s.sessions);
    let b = long_running(&mut s.sessions);
    s.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(!s.quit, "not without confirmation");
    match &s.overlay {
        Overlay::Confirm { dialog, action } => {
            assert_eq!(*action, Pending::Quit);
            let body = dialog.body.join("\n");
            assert!(
                body.contains(&format!("#{a}")) && body.contains(&format!("#{b}")),
                "{body}"
            );
        }
        o => panic!("{o:?}"),
    }
    // Esc keeps everything running.
    s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!s.quit);
    assert!(s.sessions.get(a).unwrap().state.running());
    // Confirming stops them all, then quits once they are gone (or after 5 s).
    s.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    s.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(s.sessions.get(a).unwrap().state, SessionState::Stopping);
    assert_eq!(s.sessions.get(b).unwrap().state, SessionState::Stopping);
    let until = Instant::now() + Duration::from_secs(8);
    while !s.quit && Instant::now() < until {
        s.tick();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(s.quit, "quit completes within 5 s of the confirmation");
}
