//! The main event loop (research R-6): block on input with a 250 ms timeout, drain the worker
//! channels on every wake, and redraw only when the state changed.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};

use super::state::AppState;
use crate::ui::{self, terminal::Tui};

pub const POLL_TIMEOUT: Duration = Duration::from_millis(250);

/// Runs until the user quits. The first frame is drawn before anything else happens.
pub fn run(term: &mut Tui, state: &mut AppState) -> io::Result<()> {
    let mut dirty = true;
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
        // Worker channels (procmon, dns, watch) are drained here once they exist.
    }
    Ok(())
}

fn handle(state: &mut AppState, ev: Event) -> bool {
    match ev {
        Event::Key(key) => state.handle_key(key),
        Event::Resize(..) => true,
        _ => false,
    }
}
