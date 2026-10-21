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
    /// Activity tab: 0 everything, 1 the selected session, 2 its project (`f` cycles).
    pub filter: u8,
    /// Activity tab: only entries containing this (`/`).
    pub query: String,
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
    let now = std::time::Instant::now();
    let connected = state
        .sessions
        .sessions
        .iter()
        .filter(|s| h.connected(s.id))
        .count();
    let waiting = state
        .sessions
        .sessions
        .iter()
        .filter(|s| h.waiting(s.id, now).is_some())
        .count();
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = vec![
        plural(h.pending_requests(), "request", "requests"),
        format!("{waiting} waiting"),
    ];
    let (queued, blocked) = h.queue_counts();
    parts.push(format!("{queued} queued ({blocked} blocked)"));
    let perms = &h.permissions.list;
    let mut p = plural(perms.len(), "permission", "permissions");
    if let Some(first) = perms.iter().min_by_key(|p| p.expires) {
        p.push_str(&format!(
            " (P{} {})",
            first.id,
            time_left(first.expires.saturating_duration_since(now))
        ));
    }
    parts.push(p);
    parts.push(format!("{connected} connected"));
    parts.join(" · ")
}

/// "28:12" or "1:05:00".
pub fn time_left(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The requests the Requests tab lists, oldest first: pending deliveries.
/// One entry of the Requests tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestRef {
    Delivery(u64),
    Task(u32),
    Start(u64),
}

/// The requests the Requests tab lists: pending deliveries, then tasks agents created.
pub fn requests_shown(state: &AppState) -> Vec<RequestRef> {
    state.hub.as_ref().map_or_else(Vec::new, |h| {
        h.deliveries
            .pending()
            .map(|d| RequestRef::Delivery(d.id))
            .chain(h.pending_tasks().map(|t| RequestRef::Task(t.id)))
            .chain(h.pending_starts().map(|s| RequestRef::Start(s.id)))
            .collect()
    })
}

/// The tasks the Queue tab lists, in queue order.
pub fn tasks_shown(state: &AppState) -> Vec<u32> {
    state
        .hub
        .as_ref()
        .map_or_else(Vec::new, |h| h.tasks.list.iter().map(|t| t.id).collect())
}

fn session_name(state: &AppState, id: u32) -> String {
    state.sessions.get(id).map_or(format!("#{id}"), |s| {
        format!("#{} {} · {}", s.id, s.profile.name, s.project_name)
    })
}

fn requests_lines(state: &AppState, v: &OrchView, width: usize) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let Some(h) = state.hub.as_ref() else {
        return vec![Line::styled("No requests.", dim)];
    };
    let ids = requests_shown(state);
    if ids.is_empty() {
        return vec![Line::styled(
            "No requests. Agents ask here before Fallow types into another session.",
            dim,
        )];
    }
    let now = std::time::Instant::now();
    let mut lines = Vec::new();
    let selected = v.selected.min(ids.len() - 1);
    for (i, r) in ids.iter().enumerate() {
        let id = match r {
            RequestRef::Delivery(id) => id,
            RequestRef::Start(sid) => {
                let Some(s) = h.starts.iter().find(|s| s.id == *sid) else {
                    continue;
                };
                let row = format!(
                    "{}  →  start {} in {}   expires in {}",
                    s.from.label(),
                    s.profile_name,
                    s.project_name,
                    time_left(s.expires.saturating_duration_since(now))
                );
                lines.push(Line::styled(
                    row,
                    if i == selected { sel } else { Style::default() },
                ));
                if i == selected {
                    match &s.first_task {
                        Some(t) => {
                            lines.push(Line::styled("  first task:", dim));
                            for l in wrap(t, width.saturating_sub(4)) {
                                lines.push(Line::raw(format!("  {l}")));
                            }
                        }
                        None => lines.push(Line::styled("  no first task", dim)),
                    }
                    lines.push(Line::styled(
                        "  y Start   x Reject   g Grant permission (start up to N)…   ↑↓ Next request",
                        dim,
                    ));
                }
                continue;
            }
            RequestRef::Task(t) => {
                let Some(t) = h.tasks.get(*t) else { continue };
                let left = std::time::Duration::from_secs(h.settings.request_timeout_s)
                    .saturating_sub(now.saturating_duration_since(t.created_at));
                let row = format!(
                    "{}  →  {}   new task T{}   expires in {}",
                    t.created_by.label(),
                    t.target.label(),
                    t.id,
                    time_left(left)
                );
                lines.push(Line::styled(
                    row,
                    if i == selected { sel } else { Style::default() },
                ));
                if i == selected {
                    lines.push(Line::styled("  task text:", dim));
                    for l in wrap(&t.text, width.saturating_sub(4)) {
                        lines.push(Line::raw(format!("  {l}")));
                    }
                    if !t.preconditions.is_empty() {
                        let names: Vec<&str> =
                            t.preconditions.iter().map(|k| k.1.as_str()).collect();
                        lines.push(Line::styled(format!("  after: {}", names.join(", ")), dim));
                    }
                    lines.push(Line::styled(
                        "  y Approve   e Edit   x Reject   g Grant permission…   ↑↓ Next request",
                        dim,
                    ));
                }
                continue;
            }
        };
        let Some(d) = h.deliveries.get(*id) else {
            continue;
        };
        let what = match &d.purpose {
            crate::orchestration::delivery::Purpose::Message => "deliver".to_string(),
            crate::orchestration::delivery::Purpose::Task(t) => format!("task T{t}"),
            crate::orchestration::delivery::Purpose::Wake(k) => format!("wake ({})", k.1),
        };
        let row = format!(
            "{}  →  {}   {what}   expires in {}",
            d.from.label(),
            session_name(state, d.to),
            time_left(d.expires.saturating_duration_since(now))
        );
        lines.push(Line::styled(
            row,
            if i == selected { sel } else { Style::default() },
        ));
        if i == selected {
            lines.push(Line::styled("  text that would be typed:", dim));
            // The label wraps like the text: it is part of what is typed (T055, FR-012).
            for l in wrap(
                &crate::orchestration::delivery::label(d),
                width.saturating_sub(4),
            ) {
                lines.push(Line::styled(format!("  {l}"), bold));
            }
            for l in wrap(&d.text, width.saturating_sub(4)) {
                lines.push(Line::raw(format!("  {l}")));
            }
            lines.push(Line::styled(
                "  y Approve   e Edit   x Reject   g Grant permission…   ↑↓ Next request",
                dim,
            ));
        }
    }
    lines
}

fn permissions_lines(state: &AppState, v: &OrchView) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let mut lines = vec![Line::styled(
        "a Grant new · Delete Revoke · Shift-X Revoke all",
        dim,
    )];
    let Some(h) = state.hub.as_ref() else {
        lines.push(Line::styled("No standing permissions.", dim));
        return lines;
    };
    if h.permissions.list.is_empty() {
        lines.push(Line::styled("No standing permissions.", dim));
    }
    let now = std::time::Instant::now();
    for (i, p) in h.permissions.list.iter().enumerate() {
        let text = format!(
            "P{}  {} → {}   {}   used {}   {} left",
            p.id,
            crate::orchestration::scope_label(p.source),
            crate::orchestration::scope_label(p.target),
            p.actions_label(),
            p.uses,
            time_left(p.expires.saturating_duration_since(now))
        );
        lines.push(Line::styled(
            text,
            if i == v.selected {
                sel
            } else {
                Style::default()
            },
        ));
    }
    lines
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
    let mut lines = vec![Line::styled(
        "Tasks · n new · e edit · Shift-↑↓ move · Space pause · Delete cancel",
        bold,
    )];
    let Some(h) = state.hub.as_ref() else {
        lines.push(Line::styled("  No tasks. n queues one.", dim));
        lines.push(Line::raw(""));
        lines.push(Line::styled("Events · v signal as you · c clear", bold));
        lines.push(Line::styled("  No events.", dim));
        return lines;
    };
    let tasks = tasks_shown(state);
    if tasks.is_empty() {
        lines.push(Line::styled("  No tasks. n queues one.", dim));
    }
    for (i, id) in tasks.iter().enumerate() {
        let Some(t) = h.tasks.get(*id) else { continue };
        let mut text = format!(
            "T{}  {:<20} {}  {}",
            t.id,
            t.state.label(),
            t.assigned
                .map_or_else(|| t.target.label(), |a| format!("#{a}")),
            t.text.lines().next().unwrap_or("")
        );
        if t.state == crate::orchestration::tasks::State::Blocked {
            let unmet: Vec<&str> = t
                .preconditions
                .iter()
                .filter(|k| {
                    h.events
                        .map
                        .get(*k)
                        .and_then(|e| e.signal.as_ref())
                        .is_none()
                })
                .map(|k| k.1.as_str())
                .collect();
            text.push_str(&format!("  (blocked: {})", unmet.join(", ")));
        }
        if t.paused {
            text.push_str("  [paused]");
        }
        if let Some(n) = &t.note {
            text.push_str(&format!("  — {}", n.lines().next().unwrap_or("")));
        }
        let style = if i == v.selected {
            sel
        } else {
            Style::default()
        };
        lines.push(Line::styled(text, style));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Events · v signal as you · c clear", bold));
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
        let style = if i + tasks.len() == v.selected {
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

/// What the Activity filter means now: (session, project name), and its label.
fn activity_filter(state: &AppState, v: &OrchView) -> (Option<u32>, Option<String>, String) {
    let active = state.workspace.active.and_then(|id| state.sessions.get(id));
    match (v.filter, active) {
        (1, Some(s)) => (Some(s.id), None, format!("session #{}", s.id)),
        (2, Some(s)) => (
            None,
            Some(s.project_name.clone()),
            format!("project {}", s.project_name),
        ),
        _ => (None, None, "everything".into()),
    }
}

fn activity_lines(state: &AppState, v: &OrchView) -> Vec<Line<'static>> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let Some(h) = state.hub.as_ref() else {
        return vec![Line::styled("Nothing yet.", dim)];
    };
    let (session, project, label) = activity_filter(state, v);
    let mut head = format!("f filter: {label} · / search");
    if !v.query.is_empty() {
        head.push_str(&format!(": {:?}", v.query));
    }
    let query = v.query.to_lowercase();
    let mut lines: Vec<Line<'static>> = vec![Line::styled(head, dim)];
    lines.extend(
        h.activity
            .filtered(session, project.as_deref())
            .filter(|e| {
                query.is_empty()
                    || format!(
                        "{} {} {} {}",
                        e.actor.label(),
                        e.action.label(),
                        e.subject,
                        e.outcome.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query)
            })
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
            }),
    );
    if lines.len() == 1 {
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
    let lines = match v.tab {
        Tab::Board => board_lines(state, v, body.width as usize),
        Tab::Activity => activity_lines(state, v),
        Tab::Requests => requests_lines(state, v, body.width as usize),
        Tab::Queue => queue_lines(state, v, body.width as usize),
        Tab::Permissions => permissions_lines(state, v),
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

/// The grant dialog (contracts/ui-commands.md "Grant dialog").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantForm {
    pub from: u32,
    pub from_any: bool,
    pub to: u32,
    pub to_any: bool,
    pub deliver: bool,
    pub tasks: bool,
    /// Sessions it may start (0: none).
    pub starts: u8,
    pub minutes: u64,
    /// The field with focus: 0 from, 1 to, 2 deliver, 3 tasks, 4 starts, 5 minutes.
    pub field: usize,
    /// The request to approve once granted.
    pub request: Option<RequestRef>,
    pub back: OrchView,
    pub error: Option<String>,
}

pub const GRANT_FIELDS: usize = 6;

pub fn draw_grant(f: &mut Frame<'_>, area: Rect, state: &AppState, g: &GrantForm) {
    let width = 80.min(area.width.saturating_sub(2));
    let height = (12 + u16::from(g.error.is_some())).min(area.height.saturating_sub(2));
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .title(" Grant standing permission ")
        .title_bottom(" ↑↓ field · ←→ change · Space toggle · Enter grant · Esc cancel ");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let project = |id: u32| {
        state
            .sessions
            .get(id)
            .map_or("?".to_string(), |s| s.project_name.clone())
    };
    let radio = |on: bool| if on { "(•)" } else { "( )" };
    let check = |on: bool| if on { "[x]" } else { "[ ]" };
    let rows = [
        format!(
            "From:   {} session {}     {} any session of {}",
            radio(!g.from_any),
            session_name(state, g.from),
            radio(g.from_any),
            project(g.from)
        ),
        format!(
            "To:     {} session {}     {} any session of {}",
            radio(!g.to_any),
            session_name(state, g.to),
            radio(g.to_any),
            project(g.to)
        ),
        format!("Allow:  {} deliver messages", check(g.deliver)),
        format!("        {} create tasks", check(g.tasks)),
        format!(
            "        {} start sessions, up to [{}]",
            check(g.starts > 0),
            g.starts
        ),
        format!("For:    [{}] minutes (max 480)", g.minutes),
    ];
    let mut lines = vec![Line::raw("")];
    for (i, row) in rows.into_iter().enumerate() {
        let style = if i == g.field {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::styled(format!(" {row}"), style));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " Never covers stopping, pausing or killing a session.",
        Style::default().add_modifier(Modifier::DIM),
    ));
    if let Some(e) = &g.error {
        lines.push(Line::styled(
            format!(" {e}"),
            Style::default().fg(ratatui::style::Color::Red),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
