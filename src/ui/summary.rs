//! The System area under the process list (FR-016, FR-019).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use super::processes::bytes;
use crate::app::state::AppState;

/// "GPU memory (VRAM) not included" applies wherever a discrete GPU's memory is not part of the
/// process figures: everywhere except Apple Silicon's unified memory (R-3).
const VRAM_NOTE: bool = !cfg!(all(target_os = "macos", target_arch = "aarch64"));

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    if area.height == 0 {
        return;
    }
    let block = Block::bordered().title(" System ");
    let Some(s) = state.snapshot.as_deref().map(|s| s.summary) else {
        f.render_widget(Paragraph::new("scanning…").block(block), area);
        return;
    };
    let dim = Style::default().add_modifier(Modifier::DIM);
    let remaining = Line::from(vec![
        ratatui::text::Span::raw(format!("Total {}  Remaining ", bytes(s.total_ram))),
        if s.low_memory {
            ratatui::text::Span::styled(
                format!("{} (low)", bytes(s.available_ram)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ratatui::text::Span::raw(bytes(s.available_ram))
        },
    ]);
    let mut lines = vec![
        remaining,
        Line::raw(format!(
            "LLM {}  Local models {}",
            bytes(s.llm_ram),
            bytes(s.local_model_ram)
        )),
        Line::raw(format!("Fallow {}", bytes(s.fallow_ram.bytes))),
        Line::styled(format!("Metric: {}", s.fallow_ram.metric.label()), dim),
    ];
    if VRAM_NOTE {
        lines.push(Line::styled("GPU memory (VRAM) not included", dim));
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}
