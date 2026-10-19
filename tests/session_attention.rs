//! 002 T027: "needs attention" (FR-009, SC-004, research R-4).

use std::time::{Duration, Instant};

use fallow::session::attention::{BUSY_CPU, Source, on_input, on_output, on_tick};
use fallow::session::profiles::builtins;
use fallow::session::term::Signal;
use fallow::session::{Session, SessionState};

fn session() -> Session {
    Session::detached(1, 1, builtins().remove(0))
}

const DELAY: Duration = Duration::from_secs(5);

#[test]
fn each_explicit_signal_asks_for_attention() {
    let cases = [
        vec![Signal::Bell],
        vec![Signal::Notify],
        vec![Signal::Title("✋ Action Required".into())],
        vec![Signal::Title("◇  Ready".into())],
    ];
    for sigs in cases {
        let mut s = session();
        let now = Instant::now();
        assert!(on_output(&mut s, &sigs, now), "{sigs:?}");
        assert_eq!(s.state, SessionState::NeedsAttention);
        assert_eq!(s.attention, Some(Source::Explicit));
        // Plain redraws after the bell don't clear it.
        assert!(!on_output(&mut s, &[], now));
        assert_eq!(s.state, SessionState::NeedsAttention, "{sigs:?}");
        // Working signals do.
        on_output(&mut s, &[Signal::Working], now);
        assert_eq!(s.state, SessionState::Working);
        // So does typing into the session.
        on_output(&mut s, &sigs, now);
        on_input(&mut s, now);
        assert_eq!(s.state, SessionState::Working);
    }
    let mut s = session();
    on_output(&mut s, &[Signal::Title("✦ Working".into())], Instant::now());
    assert_eq!(s.state, SessionState::Working);
    on_output(
        &mut s,
        &[Signal::Title("plain title".into())],
        Instant::now(),
    );
    assert_eq!(s.state, SessionState::Working, "other titles mean nothing");
}

#[test]
fn the_quiet_fallback_needs_quiet_and_idle_cpu() {
    let mut s = session();
    let t0 = s.last_output;
    // Quiet but busy: no.
    assert!(!on_tick(&mut s, Some(BUSY_CPU + 10.0), DELAY, t0 + DELAY));
    // Idle but not quiet long enough: no.
    assert!(!on_tick(
        &mut s,
        Some(0.0),
        DELAY,
        t0 + DELAY - Duration::from_millis(1)
    ));
    // No CPU figure yet (agent not in a snapshot): no.
    assert!(!on_tick(&mut s, None, DELAY, t0 + DELAY * 2));
    // Quiet and idle: yes, within the delay + 2 s (SC-004) at a 2 s check interval.
    assert!(on_tick(
        &mut s,
        Some(0.5),
        DELAY,
        t0 + DELAY + Duration::from_secs(2)
    ));
    assert_eq!(s.state, SessionState::NeedsAttention);
    assert_eq!(s.attention, Some(Source::Quiet));
    assert!(
        !on_tick(&mut s, Some(0.5), DELAY, t0 + DELAY * 3),
        "entered once"
    );
    // New output ends a quiet-fallback attention.
    on_output(&mut s, &[], t0 + DELAY * 3);
    assert_eq!(s.state, SessionState::Working);
}

#[test]
fn exited_and_stopping_sessions_are_left_alone() {
    let mut s = session();
    s.state = SessionState::Exited(Some(0));
    assert!(!on_output(&mut s, &[Signal::Bell], Instant::now()));
    assert!(!on_tick(
        &mut s,
        Some(0.0),
        DELAY,
        Instant::now() + DELAY * 2
    ));
    s.state = SessionState::Stopping;
    assert!(!on_output(&mut s, &[Signal::Bell], Instant::now()));
    assert_eq!(s.state, SessionState::Stopping);
}

#[test]
fn a_hidden_session_raises_a_notice_and_counts_per_project() {
    use fallow::app::state::AppState;
    let mut st = AppState::new(None, Vec::new());
    let p = st
        .projects
        .push(std::env::temp_dir(), Some("api".into()), true);
    let pid = st.projects.projects[p].id;
    let mut s = Session::detached(7, pid, builtins().remove(0));
    s.project_name = "api".into();
    st.sessions.sessions.push(s);
    // Not focused: a bell-equivalent signal through the app path produces a notice.
    {
        let sess = st.sessions.get_mut(7).unwrap();
        on_output(sess, &[Signal::Bell], Instant::now());
    }
    st.session_output(7);
    // The state was already set; a second explicit signal doesn't re-notify, but the per-project
    // count reflects it.
    let waiting = st
        .sessions
        .sessions
        .iter()
        .filter(|x| x.project == pid && x.state == SessionState::NeedsAttention)
        .count();
    assert_eq!(waiting, 1);
    // A fresh session entering attention while hidden: the app says so.
    let mut s2 = Session::detached(8, pid, builtins().remove(0));
    s2.project_name = "api".into();
    st.sessions.sessions.push(s2);
    st.sessions.get_mut(8).unwrap().last_output = Instant::now() - Duration::from_secs(60);
    // Quiet fallback through the app tick needs a CPU figure from a snapshot; without a
    // snapshot nothing fires.
    assert!(!st.tick_sessions());
    let text = format!("{:?}", st.message);
    assert!(!text.contains("#8"), "no CPU figure, no fallback: {text}");
}
