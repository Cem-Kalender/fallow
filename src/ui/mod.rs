//! Rendering: layout, panes, the hint bar and the overlays.

pub mod help;
pub mod hints;
pub mod layout;
pub mod palette;
pub mod processes;
pub mod summary;
pub mod terminal;

use crate::app::state::AppState;

/// Draws one frame.
pub fn draw(f: &mut ratatui::Frame<'_>, state: &AppState) {
    layout::draw(f, state);
}
