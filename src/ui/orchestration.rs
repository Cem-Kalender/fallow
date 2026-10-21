//! The orchestration view (003 contracts/ui-commands.md "Orchestration view"): one overlay with
//! the tabs Requests, Board, Queue, Permissions and Activity, and a counts line on top.

use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::app::state::AppState;
use crate::orchestration::Hub;
use crate::orchestration::board::BoardId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    Requests,
    #[default]
    Board,
    Queue,
    Permissions,
    Activity,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Requests,
        Tab::Board,
        Tab::Queue,
        Tab::Permissions,
        Tab::Activity,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Requests => "Requests",
            Tab::Board => "Board",
            Tab::Queue => "Queue",
            Tab::Permissions => "Permissions",
            Tab::Activity => "Activity",
        }
    }

    pub fn next(self, step: isize) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        Tab::ALL[(i + step).rem_euclid(Tab::ALL.len() as isize) as usize]
    }
}

/// What the overlay shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrchView {
    pub tab: Tab,
    /// Board tab: the shared board instead of the selected project's.
    pub shared: bool,
    /// Lines scrolled up from the newest (Board, Activity) or the selected row (lists).
    pub scroll: usize,
    pub selected: usize,
}

/// "now", "42s", "3m", "2h", "5d": how long ago.
pub fn age(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .map_or(0, |d| d.as_secs());
    match s {
        0..=4 => "now".into(),
        5..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// The counts line (SC-008).
pub fn counts(hub: Option<&Hub>, state: &AppState) -> String {
    let Some(h) = hub else {
        return "no session uses orchestration yet".into();
    };
    let connected = state
        .sessions
        .sessions
        .iter()
        .filter(|s| h.connected(s.id))
        .count();
    format!("{connected} connected")
}

/// Splits `text` into display lines at most `width` columns wide.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut w = 0;
        for c in para.chars() {
            let cw = c.width().unwrap_or(0);
            if w + cw > width {
                out.push(std::mem::take(&mut line));
                w = 0;
            }
            line.push(if c == '\t' { ' ' } else { c });
            w += cw.max(usize::from(c == '\t'));
        }
        out.push(line);
    }
    out
}

/// The board the Board tab shows.
pub fn board_shown(state: &AppState, v: &OrchView) -> (BoardId, String) {
    match (v.shared, state.projects.selected()) {
        (false, Some(p)) => (BoardId::Project(p.id), format!("{} board", p.name)),
        _ => (BoardId::Shared, "shared board".into()),
    }
}

fn board_lines(state: &AppState, v: &OrchView, width: usize) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let (board, name) = board_shown(state, v);
    let mut lines = vec![Line::styled(
        format!(
            "{name} · b: {}",
            if v.shared {
                "project board"
            } else {
                "shared board"
            }
        ),
        dim,
    )];
    let Some(h) = state.hub.as_ref() else {
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "Agents started from a profile with orchestration can post here once one runs.",
        ));
        return lines;
    };
    let mut body: Vec<Line<'static>> = Vec::new();
    for m in h.boards.messages(board) {
        body.push(Line::from(vec![
            Span::styled(format!("{:>4}  ", age(m.at)), dim),
            Span::styled(m.from.label(), bold),
        ]));
        for l in wrap(&m.text, width.saturating_sub(6)) {
            body.push(Line::raw(format!("      {l}")));
        }
    }
    if body.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("No messages. w writes one as you.", dim));
    }
    lines.extend(body);
    lines
}

/// The events the Queue tab lists: the selected project's and the shared ones.
pub fn events_shown(state: &AppState) -> Vec<crate::orchestration::events::Key> {
    state.hub.as_ref().map_or_else(Vec::new, |h| {
        h.event_list(state.projects.selected().map(|p| p.id))
    })
}

fn queue_lines(state: &AppState, v: &OrchView, width: usize) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let mut lines = vec![Line::styled("Events · v signal as you · c clear", bold)];
    let Some(h) = state.hub.as_ref() else {
        lines.push(Line::styled("  No events.", dim));
        return lines;
    };
    let keys = events_shown(state);
    if keys.is_empty() {
        lines.push(Line::styled("  No events. v signals one as you.", dim));
    }
    for (i, k) in keys.iter().enumerate() {
        let scope = match k.0 {
            BoardId::Shared => "shared",
            BoardId::Project(_) => "project",
        };
        let waiters = h.events.waiters(k);
        let mut text = match h.events.map.get(k).and_then(|e| e.signal.as_ref()) {
            Some(s) => format!(
                "● {}  ({scope})  signalled {} ago by {}",
                k.1,
                age(s.at),
                s.from.label()
            ),
            None => format!("○ {}  ({scope})  not signalled", k.1),
        };
        if !waiters.is_empty() {
            let ids: Vec<String> = waiters.iter().map(|w| format!("#{w}")).collect();
            text.push_str(&format!("  · waiting: {}", ids.join(", ")));
        }
        let style = if i == v.selected {
            sel
        } else {
            Style::default()
        };
        lines.push(Line::styled(text, style));
        if let Some(m) = h
            .events
            .map
            .get(k)
            .and_then(|e| e.signal.as_ref())
            .and_then(|s| s.message.as_deref())
        {
            for l in wrap(m, width.saturating_sub(6)).into_iter().take(3) {
                lines.push(Line::styled(format!("      {l}"), dim));
            }
        }
    }
    lines
}

fn activity_lines(state: &AppState) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let Some(h) = state.hub.as_ref() else {
        return vec![Line::styled("Nothing yet.", dim)];
    };
    let mut lines: Vec<Line<'static>> = h
        .activity
        .newest()
        .map(|e| {
            let mut spans = vec![
                Span::styled(format!("{:>4}  ", age(e.at)), dim),
                Span::raw(format!(
                    "{}  {}  {}",
                    e.actor.label(),
                    e.action.label(),
                    e.subject
                )),
            ];
            if let Some(o) = &e.outcome {
                spans.push(Span::styled(format!("  ({o})"), dim));
            }
            Line::from(spans)
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled("Nothing yet.", dim));
    }
    lines
}

pub fn draw(f: &mut Frame<'_>, area: Rect, state: &AppState, v: &OrchView) {
    let width = area.width.saturating_sub(4).clamp(30, 110);
    let height = area.height.saturating_sub(2).max(8);
    let [vert] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vert);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .title(format!(
            " Orchestration ── {} ",
            counts(state.hub.as_ref(), state)
        ))
        .title_bottom(" Tab next tab · Esc close ");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [tabs, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    let mut spans = Vec::new();
    for t in Tab::ALL {
        let style = if t == v.tab {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", t.title()), style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), tabs);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let lines = match v.tab {
        Tab::Board => board_lines(state, v, body.width as usize),
        Tab::Activity => activity_lines(state),
        Tab::Requests => vec![Line::styled("No requests.", dim)],
        Tab::Queue => queue_lines(state, v, body.width as usize),
        Tab::Permissions => vec![Line::styled("No standing permissions.", dim)],
    };
    // Board: newest at the bottom, scrolled up by `scroll`. Activity: newest on top.
    let h = body.height as usize;
    let skip = if v.tab == Tab::Board {
        lines.len().saturating_sub(h + v.scroll)
    } else {
        v.scroll.min(lines.len().saturating_sub(1))
    };
    let shown: Vec<Line<'_>> = lines.into_iter().skip(skip).take(h).collect();
    f.render_widget(Paragraph::new(shown), body);
}

/// A small read-only popup; any key closes it.
pub fn draw_info(f: &mut Frame<'_>, area: Rect, title: &str, lines: &[String]) {
    let width = lines
        .iter()
        .map(|l| l.chars().count())
        .chain([title.chars().count() + 4])
        .max()
        .unwrap_or(20)
        .saturating_add(4)
        .min(area.width.saturating_sub(2) as usize) as u16;
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .title(format!(" {title} "))
        .title_bottom(" any key closes ");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let text: Vec<Line<'_>> = lines.iter().map(|l| Line::raw(l.as_str())).collect();
    f.render_widget(Paragraph::new(text), inner);
}
