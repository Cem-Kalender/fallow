//! The main event loop (001 R-6, 002 R-5): block on the application event channel with a 250 ms
//! timeout, handle everything queued, and redraw only when the state changed, at most once per
//! 16 ms while output streams.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

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
    // The remembered project list is read after the first frame (002 FR-028).
    state.load_projects();
    let mut dirty = true;
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
    let mut rearmed = Instant::now();
    while !state.quit {
        let mut wait = if state.busy() {
            BUSY_POLL
        } else {
            POLL_TIMEOUT
        };
        // A session with output waiting is re-armed within a frame (R-5).
        if state.sessions.output_waiting() {
            wait = wait.min(FRAME.saturating_sub(rearmed.elapsed()));
        }
        if dirty {
            let hold = frames.wait();
            if hold.is_zero() {
                let t = Instant::now();
                state.prepare_frame();
                term.draw(|f| ui::draw(f, state))?;
                super::diag::mark("drawn", t.elapsed().as_micros() as u64);
                frames.drawn();
                dirty = false;
            } else {
                wait = wait.min(hold);
            }
        }
        state.ticks += 1;
        let batch = events::next_batch(&rx, wait);
        super::diag::mark("wake", batch.len() as u64);
        for ev in batch {
            dirty |= match ev {
                AppEvent::Input(ev, burst) => {
                    super::diag::mark("input", 0);
                    state.burst = burst;
                    let changed = handle(state, ev);
                    state.burst = false;
                    changed
                }
                AppEvent::Snapshot(s) => {
                    state.set_snapshot(s);
                    true
                }
                AppEvent::SessionOutput(id) => {
                    if state.workspace.active == Some(id) {
                        super::diag::mark("out_active", u64::from(id));
                    }
                    state.session_output(id)
                }
                AppEvent::SessionExited(id, code) => state.session_exited(id, code),
                AppEvent::Wake => true,
                AppEvent::Hub(i) => state.hub_event(i),
            };
            if state.quit {
                break;
            }
        }
        if let Some(c) = &worker {
            cycles.store(c.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        // Sessions may wake the loop again at most once per frame: a flooding agent costs one
        // wake-up per frame, not one per read (R-5).
        if rearmed.elapsed() >= FRAME {
            state.sessions.rearm();
            rearmed = Instant::now();
        }
        dirty |= state.poll_background();
        dirty |= state.tick_sessions();
        dirty |= state.tick();
        dirty |= state.tick_hub();
    }
    super::diag::flush();
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
