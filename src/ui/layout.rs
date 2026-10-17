//! Pane layout (contracts/ui-commands.md "Layout"): three panes plus the hint bar, collapsing to
//! tabs below 100×24 and to a "terminal too small" notice below 40×10.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::hints;
use crate::app::state::{AppState, Pane};

pub const FULL_MIN: (u16, u16) = (100, 24);
pub const TOO_SMALL: (u16, u16) = (40, 10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Tabs,
    TooSmall,
}

pub fn mode(area: Rect) -> Mode {
    if area.width < TOO_SMALL.0 || area.height < TOO_SMALL.1 {
        Mode::TooSmall
    } else if area.width < FULL_MIN.0 || area.height < FULL_MIN.1 {
        Mode::Tabs
    } else {
        Mode::Full
    }
}

pub fn draw(f: &mut Frame<'_>, state: &AppState) {
    let area = f.area();
    match mode(area) {
        Mode::TooSmall => draw_too_small(f, area),
        Mode::Tabs => {
            let [tabs, body, bar] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .areas(area);
            f.render_widget(tab_line(state.focus), tabs);
            draw_pane(f, state, state.focus, body);
            draw_hints(f, state, bar);
        }
        Mode::Full => {
            let [main, bar] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
            let s = state.sizes;
            let [files, editor, procs] = Layout::horizontal([
                Constraint::Percentage(s.files_pct),
                Constraint::Fill(1),
                Constraint::Percentage(s.processes_pct),
            ])
            .areas(main);
            draw_pane(f, state, Pane::Files, files);
            draw_pane(f, state, Pane::Editor, editor);
            let [list, system] =
                Layout::vertical([Constraint::Min(3), Constraint::Length(s.system_rows)])
                    .areas(procs);
            draw_pane(f, state, Pane::Processes, list);
            f.render_widget(
                Paragraph::new("(system summary not implemented yet)")
                    .block(Block::bordered().title(" System ")),
                system,
            );
            draw_hints(f, state, bar);
        }
    }
}

fn draw_too_small(f: &mut Frame<'_>, area: Rect) {
    let msg = "terminal too small";
    let [row] = Layout::vertical([Constraint::Length(1)])
        .flex(Flex::Center)
        .areas(area);
    let width = (msg.len() as u16).min(area.width);
    let [cell] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    f.render_widget(Paragraph::new(msg), cell);
}

fn tab_line(focus: Pane) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, pane) in Pane::ALL.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" │ "));
        }
        let style = if pane == focus {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", pane.title()), style));
    }
    Line::from(spans)
}

fn draw_pane(f: &mut Frame<'_>, state: &AppState, pane: Pane, area: Rect) {
    let focused = state.focus == pane;
    let border = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let block = Block::bordered()
        .title(format!(" {} ", pane.title()))
        .border_style(border);
    let body: Vec<Line<'_>> = match pane {
        Pane::Files => vec![Line::raw(match &state.project {
            Some(p) => p.display().to_string(),
            None => "No project open".to_string(),
        })],
        Pane::Editor => {
            let mut lines = vec![Line::raw("No file open")];
            if !state.notices.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "Configuration:",
                    Style::default().fg(Color::Yellow),
                ));
                lines.extend(state.notices.iter().map(|n| Line::raw(n.clone())));
            }
            lines
        }
        Pane::Processes => {
            let mut note = "Not covered: containers, remote machines".to_string();
            if cfg!(windows) {
                note.push_str(", WSL2");
            }
            vec![
                Line::raw("(process monitoring not implemented yet)"),
                Line::raw(""),
                Line::styled(note, Style::default().add_modifier(Modifier::DIM)),
            ]
        }
    };
    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(block),
        area,
    );
}

fn draw_hints(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    f.render_widget(
        Paragraph::new(hints::line(state.focus, false, area.width)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(w: u16, h: u16, state: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, state)).unwrap();
        let buf = t.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn modes_by_size() {
        assert_eq!(mode(Rect::new(0, 0, 120, 40)), Mode::Full);
        assert_eq!(mode(Rect::new(0, 0, 100, 24)), Mode::Full);
        assert_eq!(mode(Rect::new(0, 0, 99, 40)), Mode::Tabs);
        assert_eq!(mode(Rect::new(0, 0, 120, 23)), Mode::Tabs);
        assert_eq!(mode(Rect::new(0, 0, 40, 10)), Mode::Tabs);
        assert_eq!(mode(Rect::new(0, 0, 39, 30)), Mode::TooSmall);
        assert_eq!(mode(Rect::new(0, 0, 80, 9)), Mode::TooSmall);
    }

    #[test]
    fn full_layout_has_three_panes_system_and_marker() {
        let s = AppState::new(None, Vec::new());
        let screen = render(120, 40, &s);
        for title in [" Files ", " Editor ", " LLM Processes ", " System "] {
            assert!(screen.contains(title), "missing {title}\n{screen}");
        }
        assert!(screen.contains("Ctrl-P Commands"));
        // Wide enough that the coverage note (longer on Windows) doesn't wrap.
        let wide = render(200, 40, &s);
        let note = if cfg!(windows) {
            "Not covered: containers, remote machines, WSL2"
        } else {
            "Not covered: containers, remote machines"
        };
        assert!(wide.contains(note), "{wide}");
    }

    #[test]
    fn tabs_show_only_the_focused_pane() {
        let mut s = AppState::new(None, Vec::new());
        s.focus = Pane::Processes;
        let screen = render(80, 20, &s);
        assert!(screen.contains("(process monitoring not implemented yet)"));
        assert!(!screen.contains("No project open"));
        assert!(screen.contains("Ctrl-P Commands"));
    }

    #[test]
    fn too_small_shows_only_the_notice() {
        let s = AppState::new(None, Vec::new());
        for (w, h) in [(39, 10), (40, 9), (20, 5), (18, 1), (5, 1), (1, 1)] {
            let screen = render(w, h, &s);
            let text: String = screen.split_whitespace().collect::<Vec<_>>().join(" ");
            let expected: String = "terminal too small".chars().take(w as usize).collect();
            assert_eq!(text, expected.trim_end(), "{w}x{h}");
        }
    }

    #[test]
    fn config_notices_are_shown() {
        let s = AppState::new(None, vec!["settings.toml:3: bad value".into()]);
        assert!(render(120, 40, &s).contains("settings.toml:3: bad value"));
    }
}
