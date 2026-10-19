//! The editor view (FR-003, FR-004, FR-008, T072): line numbers, highlighted text, the
//! selection and the cursor, with the find bar and project results below or over it.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::find;
use super::layout::pane_block;
use crate::app::state::{AppState, Overlay, Pane};
use crate::editor::buffer::{Buffer, DiskState, char_width};
use crate::editor::syntax::Hl;

/// Where the parts of the editor pane are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Areas {
    pub gutter: Rect,
    pub text: Rect,
    /// The project results list (empty unless shown).
    pub results: Rect,
    /// The find bar row (empty unless open).
    pub find: Rect,
}

fn gutter_width(b: Option<&Buffer>) -> u16 {
    b.map_or(0, |b| b.len_lines().to_string().len() as u16 + 1)
}

pub fn areas(pane: Rect, state: &AppState) -> Areas {
    if pane.width < 3 || pane.height < 3 {
        return Areas::default();
    }
    let inner = Rect::new(pane.x + 1, pane.y + 1, pane.width - 2, pane.height - 2);
    let mut body = inner;
    let mut a = Areas::default();
    if let Some(f) = &state.find {
        a.find = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        body.height = body.height.saturating_sub(1);
        if f.results_open {
            a.results = body;
            return a;
        }
    }
    let g = gutter_width(state.editor.active()).min(body.width / 2);
    a.gutter = Rect::new(body.x, body.y, g, body.height);
    a.text = Rect::new(body.x + g, body.y, body.width - g, body.height);
    a
}

pub fn style(h: Hl) -> Style {
    match h {
        Hl::Keyword => Style::default().fg(Color::Magenta),
        Hl::String => Style::default().fg(Color::Green),
        Hl::Comment => Style::default().fg(Color::DarkGray),
        Hl::Function => Style::default().fg(Color::Blue),
        Hl::Type => Style::default().fg(Color::Yellow),
        Hl::Constant => Style::default().fg(Color::Cyan),
        Hl::Property => Style::default().fg(Color::LightBlue),
    }
}

fn title(state: &AppState) -> Line<'static> {
    let n = state.editor.buffers.len();
    let Some(b) = state.editor.active() else {
        return Line::raw(" Editor ");
    };
    let mut t = format!(" {}", b.name());
    if b.dirty() {
        t.push_str(" ●");
    }
    if n > 1 {
        t.push_str(&format!(" ({}/{n})", state.editor.active + 1));
    }
    t.push(' ');
    Line::raw(t)
}

fn status(b: &Buffer) -> String {
    let (l, c) = b.line_col();
    let mut s = format!(" Ln {}, Col {}", l + 1, c + 1);
    if let Some(lang) = b.language {
        s.push_str(&format!(" · {}", lang.name()));
    }
    if b.read_only {
        s.push_str(" · read-only");
    }
    match b.disk_state {
        DiskState::InSync => {}
        DiskState::ChangedOnDisk => s.push_str(" · changed on disk"),
        DiskState::DeletedOnDisk => s.push_str(" · deleted on disk"),
    }
    s.push(' ');
    s
}

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    let mut block = pane_block(state, Pane::Editor, title(state));
    if let Some(b) = state.editor.active() {
        block = block.title_bottom(Line::raw(status(b)).right_aligned());
    }
    let a = areas(area, state);
    match state.editor.active() {
        None if state.find.as_ref().is_none_or(|f| !f.results_open) => {
            let mut lines = vec![
                Line::raw("No file open"),
                Line::raw(""),
                Line::styled(
                    "Open a file from Files with Enter, or a project with Ctrl-O",
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ];
            if !state.notices.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::styled("Notices:", Style::default().fg(Color::Yellow)));
                lines.extend(state.notices.iter().map(|n| Line::raw(n.clone())));
            }
            let inner = block.inner(area);
            f.render_widget(block, area);
            let text = Rect {
                height: inner.height.saturating_sub(a.find.height),
                ..inner
            };
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text);
        }
        _ => {
            f.render_widget(block, area);
            if let Some(b) = state.editor.active() {
                if a.text.height > 0 {
                    draw_text(f, state, b, &a);
                }
            }
        }
    }
    if let Some(fb) = &state.find {
        if fb.results_open {
            find::draw_results(f, fb, a.results);
        }
        let cursor = find::draw_bar(f, fb, a.find);
        if state.focus == Pane::Editor && matches!(state.overlay, Overlay::None) {
            f.set_cursor_position(cursor);
        }
    }
}

fn draw_text(f: &mut Frame<'_>, state: &AppState, b: &Buffer, a: &Areas) {
    let tab = state.prefs.tab_width;
    let rows = a.text.height as usize;
    let width = a.text.width as usize;
    let (cur_line, cur_col) = b.line_col();
    let sel = b.selection();
    let hl = b.syntax.as_ref().map(|s| &s.highlights);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut gutter = Vec::with_capacity(rows);
    let mut lines = Vec::with_capacity(rows);
    let text = b.text();
    for row in 0..rows {
        let line = b.top + row;
        if line >= b.len_lines() {
            gutter.push(Line::raw(""));
            lines.push(Line::raw(""));
            continue;
        }
        let num = format!(
            "{:>w$} ",
            line + 1,
            w = a.gutter.width.saturating_sub(1) as usize
        );
        gutter.push(Line::styled(
            num,
            if line == cur_line {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                dim
            },
        ));
        let start = b.line_start(line);
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut run = String::new();
        let mut run_style = Style::default();
        let mut d = 0usize;
        let mut used = 0usize;
        for (col, c) in text.line(line).chars().enumerate() {
            if c == '\n' || c == '\r' || used >= width {
                break;
            }
            let w = char_width(c, d, tab);
            let at = d;
            d += w;
            if d <= b.left {
                continue;
            }
            let mut style = hl
                .and_then(|h| h.at(line, col))
                .map_or_else(Style::default, style);
            if sel.as_ref().is_some_and(|r| r.contains(&(start + col))) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            // Only the visible part of a char cut by the left edge, as spaces.
            let visible = (d - at.max(b.left)).min(width - used);
            let shown: String = if c == '\t' || at < b.left {
                " ".repeat(visible)
            } else if c.is_control() {
                "·".into()
            } else {
                c.to_string()
            };
            if style != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = style;
            run.push_str(&shown);
            used += visible;
        }
        // A selected line break shows as one reversed cell.
        if let Some(r) = &sel {
            let eol = start + b.line_len(line);
            if r.contains(&eol) && line + 1 < b.len_lines() && used < width && d >= b.left {
                if !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                spans.push(Span::styled(
                    " ",
                    Style::default().add_modifier(Modifier::REVERSED),
                ));
            }
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(gutter), a.gutter);
    f.render_widget(Paragraph::new(lines), a.text);
    let editing = state.focus == Pane::Editor
        && state.find.is_none()
        && !state.editor.released
        && matches!(state.overlay, Overlay::None);
    if editing && cur_line >= b.top && cur_line < b.top + rows {
        let dcol = crate::editor::buffer::display_col(text.line(cur_line), cur_col, tab);
        if dcol >= b.left && dcol - b.left < width {
            f.set_cursor_position(Position::new(
                a.text.x + (dcol - b.left) as u16,
                a.text.y + (cur_line - b.top) as u16,
            ));
        }
    }
}
