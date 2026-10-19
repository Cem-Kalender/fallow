//! One-line text prompts (open project, go to line, new file or folder name, rename) and the
//! shared rendering of a [`LineInput`] with its selection and cursor.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::editing::PromptKind;
use crate::editor::selection::LineInput;

/// Spans for `input` in `width` columns (scrolled so the cursor is visible) and the cursor's
/// column within them.
pub fn input_line(input: &LineInput, width: u16) -> (Line<'static>, u16) {
    let chars: Vec<char> = input.text().chars().collect();
    let width = width.max(1) as usize;
    let cursor = input.cursor().min(chars.len());
    let start = (cursor + 1).saturating_sub(width);
    let sel = input.selection().range();
    let selected = Style::default().add_modifier(Modifier::REVERSED);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_sel = false;
    for (i, c) in chars.iter().enumerate().skip(start).take(width) {
        let s = sel.contains(&i);
        if s != run_sel && !run.is_empty() {
            let style = if run_sel { selected } else { Style::default() };
            spans.push(Span::styled(std::mem::take(&mut run), style));
        }
        run_sel = s;
        run.push(*c);
    }
    if !run.is_empty() {
        let style = if run_sel { selected } else { Style::default() };
        spans.push(Span::styled(run, style));
    }
    (Line::from(spans), (cursor - start) as u16)
}

pub fn draw(
    f: &mut Frame<'_>,
    area: Rect,
    kind: &PromptKind,
    input: &LineInput,
    error: Option<&str>,
) {
    let width = 64.min(area.width.saturating_sub(2)).max(10);
    let [v] = Layout::vertical([Constraint::Length(5)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    f.render_widget(Clear, r);
    let mut title = format!(" {} ", kind.title());
    if let PromptKind::NewFile(d) | PromptKind::NewFolder(d) = kind {
        title = format!(" {} in {} ", kind.title(), d.display());
    }
    let block = Block::bordered()
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [field, msg, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    let (line, cx) = input_line(input, field.width.saturating_sub(1));
    f.render_widget(Paragraph::new(line), field);
    f.set_cursor_position(Position::new(field.x + cx, field.y));
    let note = match error {
        Some(e) => Line::styled(e.to_string(), Style::default().fg(Color::Red)),
        None => Line::styled(
            "Enter: OK   Esc: cancel",
            Style::default().add_modifier(Modifier::DIM),
        ),
    };
    f.render_widget(Paragraph::new(note), msg);
}
