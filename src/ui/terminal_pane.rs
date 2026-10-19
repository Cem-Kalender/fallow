//! A session's terminal pane (002 T023): the agent's screen or scrollback, the selection, the
//! cursor, and the exited banner.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::state::{AppState, Overlay};
use crate::app::workspace::{AgentsFocus, View};
use crate::session::{Session, SessionState};

/// Synchronized output holds redraws for at most this long (research R-5).
const SYNC_LIMIT: std::time::Duration = std::time::Duration::from_millis(150);

/// Draws session `id` in `area`. `primary`: the active pane (the one keys go to when focused).
pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect, id: Option<u32>, primary: bool) {
    let focused = primary && state.workspace.focus == AgentsFocus::Session;
    let Some(s) = id.and_then(|id| state.sessions.get(id)) else {
        let b = Block::bordered().title(" Session ");
        let leave = state.sessions.limits.leave_key.label();
        let mut text = vec![
            Line::raw("No session shown."),
            Line::raw(""),
            Line::styled(
                format!("n: start an agent in the selected project · {leave}: leave a session"),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        if !state.notices.is_empty() {
            text.push(Line::raw(""));
            text.push(Line::styled("Notices:", Style::default().fg(Color::Yellow)));
            text.extend(state.notices.iter().map(|n| Line::raw(n.clone())));
        }
        f.render_widget(
            Paragraph::new(text)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .block(b),
            area,
        );
        return;
    };
    let border = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut title = format!(" {} ", s.label());
    if let Some(t) = &s.title {
        title.push_str(&format!("— {} ", t.chars().take(40).collect::<String>()));
    }
    let mut b = Block::bordered().title(title).border_style(border);
    if s.scroll > 0 {
        b = b
            .title_bottom(Line::raw(format!(" scrolled back {} lines ", s.scroll)).right_aligned());
    }
    if !focused && s.state.running() {
        let leave = state.sessions.limits.leave_key.label();
        b = b.title_bottom(Line::raw(format!(" Enter: focus · {leave} leaves ")));
    }
    let inner = b.inner(area);
    f.render_widget(b, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let Ok(mut term) = s.term.lock() else {
        return;
    };
    if s.scroll == 0 && term.holding(SYNC_LIMIT) {
        // The agent is in the middle of a synchronized update: keep the last frame.
        return;
    }
    let rows = term.view(s.scroll);
    let total = term.total_lines();
    let height = term.size().0 as usize;
    let top = total.saturating_sub(height + s.scroll);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let v = top + i;
        lines.push(select(row, v, s));
    }
    if s.scroll > 0 && top == 0 && term.dropped() > 0 {
        if let Some(first) = lines.first_mut() {
            *first = Line::styled(
                format!(
                    "… {} older lines were dropped (sessions.scrollback_lines)",
                    term.dropped()
                ),
                Style::default().add_modifier(Modifier::DIM),
            );
        }
    }
    let cursor = (!term.screen().hide_cursor()).then(|| term.screen().cursor_position());
    drop(term);
    f.render_widget(Paragraph::new(lines), inner);
    match &s.state {
        SessionState::Exited(code) => banner(
            f,
            inner,
            &format!(
                " exited{} · R restart · w close ",
                code.map_or(String::new(), |c| format!(" with status {c}"))
            ),
        ),
        SessionState::Failed(why) => banner(
            f,
            inner,
            &format!(" could not start: {why} · R retry · w close "),
        ),
        _ => {}
    }
    let typing = focused
        && state.workspace.view == View::Agents
        && matches!(state.overlay, Overlay::None)
        && s.scroll == 0
        && s.state.running();
    if let (true, Some((r, c))) = (typing, cursor) {
        if r < inner.height && c < inner.width {
            f.set_cursor_position(Position::new(inner.x + c, inner.y + r));
        }
    }
}

/// Applies the selection highlight to one row.
fn select(row: Vec<(String, Style)>, v: usize, s: &Session) -> Line<'static> {
    let Some(((l0, c0), (l1, c1))) = s.selection else {
        return Line::from(
            row.into_iter()
                .map(|(t, st)| Span::styled(t, st))
                .collect::<Vec<_>>(),
        );
    };
    if v < l0 || v > l1 {
        return Line::from(
            row.into_iter()
                .map(|(t, st)| Span::styled(t, st))
                .collect::<Vec<_>>(),
        );
    }
    let from = if v == l0 { c0 as usize } else { 0 };
    let to = if v == l1 { c1 as usize } else { usize::MAX };
    let mut spans = Vec::new();
    let mut col = 0usize;
    for (text, st) in row {
        for ch in text.chars() {
            let style = if col >= from && col < to {
                st.add_modifier(Modifier::REVERSED)
            } else {
                st
            };
            spans.push(Span::styled(ch.to_string(), style));
            col += 1;
        }
    }
    Line::from(spans)
}

fn banner(f: &mut Frame<'_>, inner: Rect, text: &str) {
    let r = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    f.render_widget(
        Paragraph::new(Line::styled(
            text.to_string(),
            Style::default().add_modifier(Modifier::REVERSED),
        )),
        r,
    );
}
