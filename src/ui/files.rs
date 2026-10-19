//! The Files pane (FR-002, FR-068, T067, T070): the lazily listed project tree.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::layout::pane_block;
use crate::app::state::{AppState, Pane};

/// Rows of the tree that fit in the pane.
pub fn visible_rows(pane: Rect) -> usize {
    pane.height.saturating_sub(2) as usize
}

/// The tree row index under screen row `y`, if `y` is inside the list.
pub fn row_at(pane: Rect, offset: usize, y: u16) -> Option<usize> {
    let top = pane.y + 1;
    (y >= top && y < pane.bottom().saturating_sub(1)).then(|| offset + (y - top) as usize)
}

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    let block = pane_block(state, Pane::Files, Line::raw(" Files "));
    let Some(p) = &state.project else {
        let text = vec![
            Line::raw("No project open"),
            Line::raw(""),
            Line::styled(
                "Ctrl-O: open a folder",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        f.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
            area,
        );
        return;
    };
    let inner = block.inner(area);
    f.render_widget(block, area);
    let focused =
        state.focus == Pane::Files && matches!(state.overlay, crate::app::state::Overlay::None);
    let rows = p.tree.rows();
    let width = inner.width as usize;
    let dirty: Vec<&std::path::Path> = state
        .editor
        .buffers
        .iter()
        .filter(|b| b.dirty())
        .filter_map(|b| b.path.as_deref())
        .collect();
    let lines: Vec<Line<'_>> = rows
        .iter()
        .enumerate()
        .skip(p.offset)
        .take(inner.height as usize)
        .map(|(i, r)| {
            let marker = if r.is_dir {
                if r.expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let name = if r.depth == 0 {
                // The root shows its folder name; the full path is in the title of Ctrl-O.
                r.path
                    .file_name()
                    .map_or_else(|| r.name.clone(), |n| n.to_string_lossy().into_owned())
            } else {
                r.name.clone()
            };
            let mut text = format!("{}{marker}{name}", "  ".repeat(r.depth));
            if dirty.contains(&r.path.as_path()) {
                text.push_str(" ●");
            }
            let text: String = text.chars().take(width).collect();
            let mut style = Style::default();
            if r.is_dir {
                style = style.fg(Color::Cyan);
            }
            if i == p.selected {
                style = if focused {
                    style.add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    style.add_modifier(Modifier::REVERSED)
                };
            }
            Line::from(Span::styled(text, style))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}
