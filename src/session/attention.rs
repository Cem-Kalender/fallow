//! "Needs attention" (002 FR-009, research R-4).
//!
//! Signals, most reliable first:
//! 1. BEL from the agent;
//! 2. OSC 9 / OSC 777 notifications and OSC 7501 "waiting"/"done" (OSC 9;4 progress and
//!    OSC 7501 "working" mean working);
//! 3. title status: Gemini CLI's "✋ Action Required" and "◇ Ready" (needs attention) and
//!    "✦ Working";
//! 4. the quiet fallback: no output for the attention delay while the agent's process group is
//!    under the busy threshold (001 FR-012: 2% of one core).
//!
//! An explicit signal (1–3) stays until the user types into the session or the agent reports
//! working again: agent TUIs often redraw right after ringing the bell, so plain output must not
//! clear it. The quiet fallback clears on any new output.

use std::time::{Duration, Instant};

use super::term::Signal;
use super::{Session, SessionState};

/// 001 FR-012 busy threshold, % of one core averaged over 10 s.
pub const BUSY_CPU: f32 = 2.0;

/// Why a session needs attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Explicit,
    Quiet,
}

/// What a title says about the agent's state, if anything.
fn title_state(title: &str) -> Option<bool> {
    let t = title.trim_start();
    if t.starts_with('✋') || t.starts_with('◇') {
        Some(true)
    } else if t.starts_with('✦') {
        Some(false)
    } else {
        None
    }
}

/// Applies the signals of new output. Returns true if the session started needing attention.
pub fn on_output(s: &mut Session, signals: &[Signal], now: Instant) -> bool {
    if !s.state.running() || s.state == SessionState::Stopping {
        return false;
    }
    let mut want: Option<bool> = None;
    for sig in signals {
        match sig {
            Signal::Bell | Signal::Notify => want = Some(true),
            Signal::Working => want = Some(false),
            Signal::Title(t) => {
                if let Some(w) = title_state(t) {
                    want = Some(w);
                }
            }
        }
    }
    match want {
        Some(true) => enter(s, Source::Explicit, now),
        Some(false) => {
            leave(s, now);
            false
        }
        None => {
            if s.attention == Some(Source::Quiet) {
                leave(s, now);
            } else if s.state == SessionState::Starting {
                s.set_state_at(SessionState::Working, now);
            }
            false
        }
    }
}

/// The user typed into the session: it is working on the input.
pub fn on_input(s: &mut Session, now: Instant) {
    if s.state == SessionState::NeedsAttention {
        leave(s, now);
    }
}

/// Periodic check for the quiet fallback. `group_cpu`: the agent's process group, % of one
/// core (`None` before the first snapshot that includes it). Returns true if the session
/// started needing attention.
pub fn on_tick(s: &mut Session, group_cpu: Option<f32>, delay: Duration, now: Instant) -> bool {
    if s.state != SessionState::Working {
        return false;
    }
    let quiet = now.saturating_duration_since(s.last_output) >= delay;
    let idle = group_cpu.is_some_and(|c| c < BUSY_CPU);
    if quiet && idle {
        return enter(s, Source::Quiet, now);
    }
    false
}

fn enter(s: &mut Session, source: Source, now: Instant) -> bool {
    let was = s.state == SessionState::NeedsAttention;
    s.attention = Some(match (s.attention, source) {
        (Some(Source::Explicit), _) => Source::Explicit,
        (_, src) => src,
    });
    s.set_state_at(SessionState::NeedsAttention, now);
    !was
}

fn leave(s: &mut Session, now: Instant) {
    s.attention = None;
    if matches!(
        s.state,
        SessionState::NeedsAttention | SessionState::Starting
    ) {
        s.set_state_at(SessionState::Working, now);
    }
}
