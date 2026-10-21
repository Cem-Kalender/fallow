//! Rendering: layout, panes, the hint bar and the overlays.

pub mod confirm;
pub mod editor;
pub mod files;
pub mod find;
pub mod goto;
pub mod help;
pub mod hints;
pub mod layout;
pub mod orchestration;
pub mod palette;
pub mod processes;
pub mod prompt;
pub mod summary;
pub mod terminal;
pub mod terminal_pane;
pub mod workspace;

use crate::app::state::AppState;

/// Draws one frame.
pub fn draw(f: &mut ratatui::Frame<'_>, state: &AppState) {
    layout::draw(f, state);
}
