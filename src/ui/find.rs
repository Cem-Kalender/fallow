//! The find bar and project results (FR-005, T077; contracts/ui-commands.md "Find bar").
//!
//! `Ctrl-F` opens the bar with scope "this file"; `Tab` toggles "this file" ⇄ "whole project",
//! and the scope is always shown. In project scope `Enter` runs the search (never while typing)
//! and opens the results list.
//!
//! The results header starts with `Results:` only once there is at least one result row; the
//! search benchmark (T084) waits for that text.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::prompt::input_line;
use crate::app::editing::{FindBar, Scope};
use crate::editor::search::{RESULT_CAP, SearchState};

/// The marker the search benchmark waits for.
pub const RESULTS_MARKER: &str = "Results:";

/// The result index under screen row `y` (the first row is the header).
pub fn row_at(area: Rect, offset: usize, y: u16) -> Option<usize> {
    let top = area.y + 1;
    (y >= top && y < area.bottom()).then(|| offset + (y - top) as usize)
}

/// Draws the bar; returns where the text cursor goes.
pub fn draw_bar(f: &mut Frame<'_>, fb: &FindBar, area: Rect) -> Position {
    if area.height == 0 {
        return Position::new(area.x, area.y);
    }
    let label = "Find: ";
    let scope = format!(
        " [{}] Tab: {} ",
        fb.scope.label(),
        match fb.scope {
            Scope::File => "whole project",
            Scope::Project => "this file",
        }
    );
    let note = if fb.no_match { " No match " } else { "" };
    let fixed = (label.len() + scope.chars().count() + note.len()) as u16;
    let field_w = area.width.saturating_sub(fixed).max(1);
    let (line, cx) = input_line(&fb.input, field_w);
    let mut spans = vec![Span::styled(
        label,
        Style::default().add_modifier(Modifier::BOLD),
    )];
    let mut text_w = 0u16;
    for s in &line.spans {
        text_w += s.content.chars().count() as u16;
    }
    spans.extend(line.spans);
    spans.push(Span::raw(
        " ".repeat(field_w.saturating_sub(text_w) as usize),
    ));
    spans.push(Span::styled(scope, Style::default().fg(Color::Cyan)));
    if !note.is_empty() {
        spans.push(Span::styled(note, Style::default().fg(Color::Red)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    Position::new(area.x + label.len() as u16 + cx, area.y)
}

pub fn draw_results(f: &mut Frame<'_>, fb: &FindBar, area: Rect) {
    if area.height == 0 {
        return;
    }
    let Some(s) = &fb.session else {
        return;
    };
    let n = s.results.len();
    let mut header = if n == 0 {
        match s.state {
            SearchState::Running => "Searching…".to_string(),
            SearchState::Cancelled => "Search cancelled, no results".to_string(),
            SearchState::Failed => {
                format!("Search failed: {}", s.error.clone().unwrap_or_default())
            }
            _ => format!("No results for {:?}", s.query),
        }
    } else {
        format!("{RESULTS_MARKER} {n}")
    };
    if n > 0 {
        match s.state {
            SearchState::Running => header.push_str(" · searching…"),
            SearchState::Capped => header.push_str(&format!(
                " (stopped at the {RESULT_CAP} result cap; refine the query)"
            )),
            SearchState::Cancelled => header.push_str(" · cancelled"),
            _ => {}
        }
        if let Some(d) = s.finished {
            header.push_str(&format!(" · {} ms", d.as_millis()));
        }
        header.push_str(" · Enter/double-click: open");
    }
    let mut lines = vec![Line::styled(
        header,
        Style::default().add_modifier(Modifier::BOLD),
    )];
    let width = area.width as usize;
    let rows = area.height.saturating_sub(1) as usize;
    for (i, hit) in s.results.iter().enumerate().skip(fb.offset).take(rows) {
        let loc = format!("{}:{}: ", s.relative(&hit.path).display(), hit.line);
        let text = hit.text.trim_start();
        let mut style = Style::default();
        if i == fb.selected {
            style = style.add_modifier(Modifier::REVERSED);
        }
        let all: String = format!("{loc}{text}").chars().take(width).collect();
        let loc_len = loc.chars().count().min(all.chars().count());
        let (a, b): (String, String) = (
            all.chars().take(loc_len).collect(),
            all.chars().skip(loc_len).collect(),
        );
        lines.push(Line::from(vec![
            Span::styled(a, style.fg(Color::Cyan)),
            Span::styled(b, style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}
