//! The main event loop (research R-6): block on input with a 250 ms timeout, drain the worker
//! channels on every wake, and redraw only when the state changed.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use crossterm::event::{self, Event};

use super::state::AppState;
use crate::procmon::worker::WorkerHandle;
use crate::ui::{self, terminal::Tui};

pub const POLL_TIMEOUT: Duration = Duration::from_millis(250);

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
    let worker = start();
    if let Some(w) = &worker {
        state.requests = Some(w.requests.clone());
    }
    let mut dirty = false;
    while !state.quit {
        if dirty {
            term.draw(|f| ui::draw(f, state))?;
            dirty = false;
        }
        state.ticks += 1;
        if event::poll(POLL_TIMEOUT)? {
            // Take everything already queued before drawing again.
            loop {
                dirty |= handle(state, event::read()?);
                if state.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if let Some(w) = &worker {
            loop {
                match w.snapshots.try_recv() {
                    Ok(s) => {
                        state.set_snapshot(s);
                        dirty = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            cycles.store(w.cycles.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        dirty |= state.tick();
    }
    Ok(())
}

fn handle(state: &mut AppState, ev: Event) -> bool {
    match ev {
        Event::Key(key) => state.handle_key(key),
        Event::Mouse(m) => state.handle_mouse(m),
        Event::Resize(w, h) => {
            state.area = ratatui::layout::Rect::new(0, 0, w, h);
            true
        }
        _ => false,
    }
}
