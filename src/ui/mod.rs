//! Rendering: layout, panes and the key-hint bar.

pub mod hints;
pub mod layout;
pub mod terminal;

use crate::app::state::AppState;

/// Draws one frame.
pub fn draw(f: &mut ratatui::Frame<'_>, state: &AppState) {
    layout::draw(f, state);
}
