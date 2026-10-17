//! The System area under the process list (FR-016, FR-019), with "RAM remaining" highlighted
//! when less than 10% of RAM is available (US2 AS3).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::processes::bytes;
use crate::app::state::AppState;
use crate::procmon::snapshot::SystemSummary;

/// "GPU memory (VRAM) not included" applies wherever a discrete GPU's memory is not part of the
/// process figures: everywhere except Apple Silicon's unified memory (R-3).
const VRAM_NOTE: bool = !cfg!(all(target_os = "macos", target_arch = "aarch64"));

/// The style of a low "RAM remaining" figure.
pub fn low_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// The area's text.
pub fn lines(s: &SystemSummary) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let remaining = if s.low_memory {
        Span::styled(format!(" {} low ", bytes(s.available_ram)), low_style())
    } else {
        Span::raw(bytes(s.available_ram))
    };
    let mut lines = vec![
        Line::from(vec![
            Span::raw(format!("Total {}  RAM remaining ", bytes(s.total_ram))),
            remaining,
        ]),
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
    lines
}

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    if area.height == 0 {
        return;
    }
    let block = Block::bordered().title(" System ");
    match state.snapshot.as_deref() {
        None => f.render_widget(Paragraph::new("scanning…").block(block), area),
        Some(s) => f.render_widget(Paragraph::new(lines(&s.summary)).block(block), area),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procmon::snapshot::{Footprint, FootprintMetric};

    fn summary(total: u64, available: u64) -> SystemSummary {
        SystemSummary {
            total_ram: total,
            available_ram: available,
            llm_ram: 0,
            local_model_ram: 0,
            fallow_ram: Footprint {
                bytes: 0,
                metric: FootprintMetric::Pss,
            },
            low_memory: SystemSummary::is_low_memory(total, available),
        }
    }

    #[test]
    fn ram_remaining_is_highlighted_only_when_low() {
        let gb = 1 << 30;
        let low = lines(&summary(16 * gb, gb));
        let span = &low[0].spans[1];
        assert_eq!(span.style, low_style());
        assert!(low[0].spans[0].content.contains("RAM remaining"));
        let ok = lines(&summary(16 * gb, 8 * gb));
        assert_eq!(ok[0].spans[1].style, Style::default());
    }
}
