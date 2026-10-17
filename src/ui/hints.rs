//! The key-hint bar (FR-060, FR-066, contracts/ui-commands.md "Hint bar content"), generated
//! from the command registry.
//!
//! It always starts with a way to open help that doesn't need F1 and with `Ctrl-P Commands`.
//! `Ctrl-P Commands` is also the startup benchmark's first-frame marker (contracts/cli.md):
//! it must be present whenever no text input has focus.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::commands::{self, Context};
use crate::app::state::{AppState, Overlay};

const SEPARATOR: &str = "  ";

/// The bar as one line, keeping as many whole hints as fit in `width` columns. The first two
/// (help and commands) are always kept. `extra` (a transient message) follows them.
pub fn line(hints: &[(&str, &str)], extra: Option<&str>, width: u16) -> Line<'static> {
    let key = Style::default().add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, (k, label)) in hints.iter().enumerate() {
        let sep = if i == 0 { 0 } else { SEPARATOR.len() };
        let len = sep + k.chars().count() + 1 + label.chars().count();
        if i >= 2 && used + len > width as usize {
            break;
        }
        if i > 0 {
            spans.push(Span::raw(SEPARATOR));
        }
        spans.push(Span::styled(k.to_string(), key));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(label.to_string()));
        used += len;
        if i == 1 {
            if let Some(msg) = extra {
                spans.push(Span::raw(SEPARATOR));
                spans.push(Span::styled(
                    msg.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
                return Line::from(spans);
            }
        }
    }
    Line::from(spans)
}

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    if area.height == 0 {
        return;
    }
    let (context, text_input) = match state.overlay {
        Overlay::Palette { .. } => (Context::Global, true),
        _ => (state.focus.context(), false),
    };
    let mut hints = commands::hints(context, text_input);
    match state.overlay {
        Overlay::Help { .. } => {
            hints.truncate(2);
            hints.extend([("Esc", "Close"), ("↑↓", "Scroll")]);
        }
        Overlay::Palette { .. } => hints.extend([("Enter", "Run"), ("Esc", "Close")]),
        Overlay::None => {}
    }
    let msg = state.message.as_ref().map(|(m, _)| m.as_str());
    f.render_widget(Paragraph::new(line(&hints, msg, area.width)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Pane;

    fn text(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn marker_present_whenever_no_text_input_has_focus() {
        for pane in Pane::ALL {
            let t = text(&line(&commands::hints(pane.context(), false), None, 200));
            assert!(t.starts_with("? Help  Ctrl-P Commands"), "{pane:?}: {t}");
        }
    }

    #[test]
    fn text_input_shows_palette_help() {
        let t = text(&line(&commands::hints(Context::Editor, true), None, 200));
        assert!(t.starts_with("Ctrl-P › Help  Ctrl-P Commands"), "{t}");
    }

    #[test]
    fn keeps_whole_hints_only() {
        let h = commands::hints(Context::Processes, false);
        assert_eq!(
            text(&line(&h, None, 40)),
            "? Help  Ctrl-P Commands  m Sort mem"
        );
        assert_eq!(
            text(&line(&h, None, 10)),
            "? Help  Ctrl-P Commands",
            "never dropped"
        );
    }

    #[test]
    fn messages_follow_the_marker() {
        let h = commands::hints(Context::Processes, false);
        let t = text(&line(&h, Some("Pause: not available yet"), 200));
        assert_eq!(t, "? Help  Ctrl-P Commands  Pause: not available yet");
    }
}
