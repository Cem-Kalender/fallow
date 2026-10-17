//! The key-hint bar (FR-060, FR-066, contracts/ui-commands.md "Hint bar content").
//!
//! It always starts with a way to open help that doesn't need F1 and with `Ctrl-P Commands`.
//! `Ctrl-P Commands` is also the startup benchmark's first-frame marker (contracts/cli.md):
//! it must be present whenever no text input has focus.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::state::Pane;

/// A key and what it does.
pub type Hint = (&'static str, &'static str);

const SEPARATOR: &str = "  ";

/// The hints for the focused pane, in display order.
pub fn hints(focus: Pane, text_input: bool) -> Vec<Hint> {
    let mut out: Vec<Hint> = Vec::with_capacity(8);
    out.push(if text_input {
        ("Ctrl-P ›", "Help")
    } else {
        ("?", "Help")
    });
    out.push(("Ctrl-P", "Commands"));
    out.extend_from_slice(match focus {
        Pane::Processes => &[
            ("m", "Sort mem"),
            ("Enter", "Details"),
            ("g", "Go to dir"),
            ("p", "Pause"),
            ("Shift-K", "Kill"),
        ][..],
        Pane::Files => &[
            ("Enter", "Open"),
            ("a", "New file"),
            ("F2", "Rename"),
            ("d", "Delete"),
        ],
        Pane::Editor => &[
            ("Ctrl-S", "Save"),
            ("Ctrl-F", "Find"),
            ("Ctrl-G", "Go to line"),
            ("Esc Tab", "Next pane"),
        ],
    });
    out
}

/// The bar as one line, keeping as many whole hints as fit in `width` columns. The first two
/// (help and commands) are always kept.
pub fn line(focus: Pane, text_input: bool, width: u16) -> Line<'static> {
    let key = Style::default().add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, (k, label)) in hints(focus, text_input).into_iter().enumerate() {
        let sep = if i == 0 { 0 } else { SEPARATOR.len() };
        let len = sep + k.chars().count() + 1 + label.chars().count();
        if i >= 2 && used + len > width as usize {
            break;
        }
        if i > 0 {
            spans.push(Span::raw(SEPARATOR));
        }
        spans.push(Span::styled(k, key));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(label));
        used += len;
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn marker_present_whenever_no_text_input_has_focus() {
        for pane in Pane::ALL {
            let t = text(&line(pane, false, 200));
            assert!(t.starts_with("? Help  Ctrl-P Commands"), "{pane:?}: {t}");
        }
    }

    #[test]
    fn text_input_shows_palette_help() {
        let t = text(&line(Pane::Editor, true, 200));
        assert!(t.starts_with("Ctrl-P › Help  Ctrl-P Commands"), "{t}");
    }

    #[test]
    fn keeps_whole_hints_only() {
        let t = text(&line(Pane::Processes, false, 40));
        assert_eq!(t, "? Help  Ctrl-P Commands  m Sort mem");
        let t = text(&line(Pane::Processes, false, 10));
        assert_eq!(
            t, "? Help  Ctrl-P Commands",
            "help and commands are never dropped"
        );
    }
}
