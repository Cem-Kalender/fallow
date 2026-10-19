//! The main event loop (001 R-6, 002 R-5): block on the application event channel with a 250 ms
//! timeout, handle everything queued, and redraw only when the state changed, at most once per
//! 16 ms while output streams.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;

use crossterm::event::Event;

use super::events::{self, AppEvent, FrameCap};
use super::state::AppState;
use crate::procmon::worker::WorkerHandle;
use crate::ui::{self, terminal::Tui};

pub const POLL_TIMEOUT: Duration = Duration::from_millis(250);
/// While a search runs, its results are polled this often (SC-010).
const BUSY_POLL: Duration = Duration::from_millis(15);
const FRAME: Duration = Duration::from_millis(16);

/// Runs until the user quits. The first frame is drawn before `start` launches the background
/// threads; `cycles` mirrors the worker's refresh counter for `--bench-stats`.
pub fn run(
    term: &mut Tui,
    state: &mut AppState,
    start: impl FnOnce() -> Option<WorkerHandle>,
    cycles: &AtomicU64,
) -> io::Result<()> {
    let size = term.size()?;
    state.area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    term.draw(|f| ui::draw(f, state))?;
    let (tx, rx) = channel();
    events::spawn_input(tx.clone())?;
    let worker = start().map(|w| {
        let WorkerHandle {
            snapshots,
            requests,
            cycles,
        } = w;
        events::forward_snapshots(snapshots, tx.clone());
        state.requests = Some(requests);
        cycles
    });
    state.events = Some(tx);
    let mut frames = FrameCap::new(FRAME);
    let mut dirty = false;
    while !state.quit {
        let mut wait = if state.busy() {
            BUSY_POLL
        } else {
            POLL_TIMEOUT
        };
        if dirty {
            let hold = frames.wait();
            if hold.is_zero() {
                state.prepare_frame();
                term.draw(|f| ui::draw(f, state))?;
                frames.drawn();
                dirty = false;
            } else {
                wait = wait.min(hold);
            }
        }
        state.ticks += 1;
        for ev in events::next_batch(&rx, wait) {
            dirty |= match ev {
                AppEvent::Input(ev, burst) => {
                    state.burst = burst;
                    let changed = handle(state, ev);
                    state.burst = false;
                    changed
                }
                AppEvent::Snapshot(s) => {
                    state.set_snapshot(s);
                    true
                }
                AppEvent::SessionOutput(id) => state.session_output(id),
                AppEvent::SessionExited(id, code) => state.session_exited(id, code),
                AppEvent::Wake => true,
            };
            if state.quit {
                break;
            }
        }
        if let Some(c) = &worker {
            cycles.store(c.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        dirty |= state.poll_background();
        dirty |= state.tick();
    }
    Ok(())
}

fn handle(state: &mut AppState, ev: Event) -> bool {
    match ev {
        Event::Key(key) => state.handle_key(key),
        Event::Mouse(m) => state.handle_mouse(m),
        Event::Paste(text) => state.paste(&text),
        Event::Resize(w, h) => {
            state.area = ratatui::layout::Rect::new(0, 0, w, h);
            true
        }
        Event::FocusGained | Event::FocusLost => false,
    }
}
