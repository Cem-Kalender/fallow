//! The agent workspace (002 FR-019, contracts/ui-commands.md "Views"): projects | sessions |
//! LLM processes, with the active session's terminal in the middle column.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

use super::layout::{self, Mode};
use super::{processes, summary, terminal_pane};
use crate::app::state::{AppState, Overlay};
use crate::app::workspace::{AgentsFocus, StartMenu};

/// Where the workspace's parts are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    pub projects: Rect,
    pub list: Rect,
    pub session: Rect,
    pub procs: Rect,
    pub system: Rect,
    pub hints: Rect,
    pub tabs: Rect,
}

/// Rows of the session list (with borders).
fn list_rows(state: &AppState) -> u16 {
    let n = state.listed_sessions().len() as u16;
    if state.workspace.focus == AgentsFocus::Session {
        3
    } else {
        (n + 2).clamp(3, 8)
    }
}

pub fn geometry(state: &AppState) -> Geometry {
    let area = state.area;
    let mut g = Geometry::default();
    match layout::mode(area) {
        Mode::TooSmall => g,
        Mode::Tabs => {
            // Small terminal: the focused part takes the whole body; a focused session always
            // gets the full width.
            let [tabs, body, hints] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .areas(area);
            g.tabs = tabs;
            g.hints = hints;
            match state.workspace.focus {
                AgentsFocus::Projects => g.projects = body,
                AgentsFocus::Processes => {
                    let rows = state.sizes.system_rows.min(body.height.saturating_sub(3));
                    [g.procs, g.system] =
                        Layout::vertical([Constraint::Min(3), Constraint::Length(rows)])
                            .areas(body);
                }
                AgentsFocus::Sessions | AgentsFocus::Session => {
                    let [list, session] = Layout::vertical([
                        Constraint::Length(list_rows(state)),
                        Constraint::Min(1),
                    ])
                    .areas(body);
                    g.list = list;
                    g.session = session;
                }
            }
            g
        }
        Mode::Full => {
            let [main, hints] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
            g.hints = hints;
            let s = state.sizes;
            let [projects, middle, procs] = Layout::horizontal([
                Constraint::Percentage(s.files_pct.min(25)),
                Constraint::Fill(1),
                Constraint::Percentage(s.processes_pct),
            ])
            .areas(main);
            let [list, session] =
                Layout::vertical([Constraint::Length(list_rows(state)), Constraint::Min(3)])
                    .areas(middle);
            let [plist, system] =
                Layout::vertical([Constraint::Min(3), Constraint::Length(s.system_rows)])
                    .areas(procs);
            g.projects = projects;
            g.list = list;
            g.session = session;
            g.procs = plist;
            g.system = system;
            g
        }
    }
}

/// The inner area of the session pane: the agent's terminal size.
/// The active session's pane and, in split view, the second session's pane (FR-011).
pub fn session_panes(state: &AppState) -> (Rect, Option<Rect>) {
    let r = geometry(state).session;
    match state.workspace.split {
        Some(_) if r.width >= 40 => {
            let [a, b] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(r);
            (a, Some(b))
        }
        _ => (r, None),
    }
}

fn inner(r: Rect) -> Rect {
    if r.width < 3 || r.height < 3 {
        return Rect::new(0, 0, 80, 24);
    }
    Rect::new(r.x + 1, r.y + 1, r.width - 2, r.height - 2)
}

/// The terminal area of the split session, if shown.
pub fn split_rect(state: &AppState) -> Option<Rect> {
    session_panes(state).1.map(inner)
}

pub fn session_rect(state: &AppState) -> Rect {
    let r = session_panes(state).0;
    if r.width < 3 || r.height < 3 {
        // Before the first layout: a sensible default for the agent.
        return Rect::new(0, 0, 80, 24);
    }
    Rect::new(r.x + 1, r.y + 1, r.width - 2, r.height - 2)
}

/// The list row under screen row `y` of a bordered list.
pub fn list_row(area: Rect, y: u16) -> Option<usize> {
    (y > area.y && y + 1 < area.bottom()).then(|| (y - area.y - 1) as usize)
}

fn block(state: &AppState, title: String, focused: bool) -> Block<'static> {
    let _ = state;
    let style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::bordered().title(title).border_style(style)
}

pub fn draw(f: &mut Frame<'_>, state: &AppState) {
    let g = geometry(state);
    if g.tabs.height > 0 {
        let names = ["Projects", "Sessions", "LLM Processes"];
        let cur = match state.workspace.focus {
            AgentsFocus::Projects => 0,
            AgentsFocus::Sessions | AgentsFocus::Session => 1,
            AgentsFocus::Processes => 2,
        };
        let spans: Vec<Span<'_>> = names
            .iter()
            .enumerate()
            .flat_map(|(i, n)| {
                let st = if i == cur {
                    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default()
                };
                [Span::styled(format!(" {n} "), st), Span::raw(" ")]
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), g.tabs);
    }
    if g.projects.width > 0 {
        draw_projects(f, state, g.projects);
    }
    if g.list.width > 0 {
        draw_sessions(f, state, g.list);
    }
    if g.session.width > 0 {
        let (a, b) = session_panes(state);
        terminal_pane::draw(f, state, a, state.workspace.active, true);
        if let (Some(b), Some(id)) = (b, state.workspace.split) {
            terminal_pane::draw(f, state, b, Some(id), false);
        }
    }
    if g.procs.width > 0 {
        processes::draw(f, state, g.procs);
        summary::draw(f, state, g.system);
    }
}

fn draw_projects(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    let focused = state.workspace.focus == AgentsFocus::Projects;
    let b = block(state, " Projects ".into(), focused);
    if state.projects.is_empty() {
        let text = vec![
            Line::raw("No projects yet"),
            Line::raw(""),
            Line::styled(
                "Ctrl-O: add a folder",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        f.render_widget(Paragraph::new(text).block(b), area);
        return;
    }
    let items: Vec<ListItem<'_>> = state
        .projects
        .projects
        .iter()
        .map(|p| {
            let n = state
                .sessions
                .sessions
                .iter()
                .filter(|s| s.project == p.id)
                .count();
            let waiting = state
                .sessions
                .sessions
                .iter()
                .filter(|s| {
                    s.project == p.id && s.state == crate::session::SessionState::NeedsAttention
                })
                .count();
            let mut spans = vec![Span::raw(p.name.clone())];
            if n > 0 {
                spans.push(Span::styled(
                    format!("  {n}"),
                    Style::default().fg(Color::Cyan),
                ));
            }
            if waiting > 0 {
                spans.push(Span::styled(
                    format!(" ●{waiting}"),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if !p.present {
                spans.push(Span::styled("  missing", Style::default().fg(Color::Red)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut st = ListState::default().with_selected(state.projects.selected);
    let hl = if focused {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    f.render_stateful_widget(List::new(items).block(b).highlight_style(hl), area, &mut st);
}

fn draw_sessions(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    let name = state
        .projects
        .selected()
        .map_or_else(|| "—".to_string(), |p| p.name.clone());
    let focused = state.workspace.focus == AgentsFocus::Sessions;
    let title = if state.workspace.overview {
        " All sessions (Ctrl-T or Esc: this project) ".to_string()
    } else {
        format!(" Sessions: {name} ")
    };
    let b = block(state, title, focused);
    let ids = state.listed_sessions();
    if ids.is_empty() {
        let hint = if state.projects.selected().is_some() {
            "No sessions. n: start an agent in this project"
        } else {
            "Add a project with Ctrl-O, then n to start an agent"
        };
        f.render_widget(
            Paragraph::new(Line::styled(
                hint,
                Style::default().add_modifier(Modifier::DIM),
            ))
            .block(b),
            area,
        );
        return;
    }
    let items: Vec<ListItem<'_>> = ids
        .iter()
        .filter_map(|id| state.sessions.get(*id))
        .map(|s| {
            let since = s.state_since.elapsed().as_secs();
            let state_style = match s.state {
                crate::session::SessionState::NeedsAttention => Style::default().fg(Color::Yellow),
                crate::session::SessionState::Failed(_) => Style::default().fg(Color::Red),
                crate::session::SessionState::Exited(_) => {
                    Style::default().add_modifier(Modifier::DIM)
                }
                _ => Style::default().fg(Color::Green),
            };
            let metrics = state
                .session_metrics(s.id)
                .map_or(String::new(), |(bytes, cpu, n)| {
                    format!("  {} {:.0}% {n}p", crate::ui::processes::bytes(bytes), cpu)
                });
            let project = if state.workspace.overview {
                format!("{}  ", s.project_name)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("#{} {}  {project}", s.id, s.profile.name)),
                Span::styled(
                    s.workdir
                        .branch()
                        .map_or(String::new(), |b| format!("{b}  ")),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(s.state.label(), state_style),
                Span::styled(
                    format!("  {}:{:02}", since / 60, since % 60),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(metrics, Style::default().add_modifier(Modifier::DIM)),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(
        state
            .workspace
            .list_selected
            .min(ids.len().saturating_sub(1)),
    ));
    let hl = if focused {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    f.render_stateful_widget(List::new(items).block(b).highlight_style(hl), area, &mut st);
}

/// The start menu overlay (contracts/ui-commands.md "Start menu").
pub fn draw_menu(f: &mut Frame<'_>, area: Rect, m: &StartMenu) {
    let width = 64.min(area.width.saturating_sub(2)).max(24);
    let rows = m.agents.len()
        + if m.wheres.is_empty() {
            0
        } else {
            m.wheres.len() + 2
        }
        + usize::from(m.error.is_some()) * 2;
    let height = (rows as u16 + 4).min(area.height.saturating_sub(2)).max(5);
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .title(" New session ")
        .title_bottom(" ↑↓ choose · Tab agent/where · Enter start · Esc cancel ");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line<'_>> = vec![Line::styled("Agent:", bold)];
    for (i, a) in m.agents.iter().enumerate() {
        let mut style = if a.available.is_ok() {
            Style::default()
        } else {
            dim
        };
        if i == m.agent {
            style = if m.on_where {
                style.add_modifier(Modifier::BOLD)
            } else {
                sel
            };
        }
        let text = match &a.available {
            Ok(()) => format!("  {}", a.label),
            Err(e) => format!("  {}  ({e})", a.label),
        };
        lines.push(Line::styled(text, style));
    }
    if !m.wheres.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Where:", bold));
        let profile = m.agents.get(m.agent).map_or("agent", |a| a.id.as_str());
        for (i, w) in m.wheres.iter().enumerate() {
            let mark = if i == m.where_sel { "●" } else { "○" };
            let style = if i == m.where_sel && m.on_where {
                sel
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                format!("  {mark} {}", w.label(profile)),
                style,
            ));
        }
    }
    if let Some(e) = &m.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(e.clone(), Style::default().fg(Color::Red)));
    }
    f.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// The bytes reporting a mouse event to an agent that enabled mouse reporting, at pane cell
/// (`col`, `row`). `None` when the agent didn't ask for this kind of event.
pub fn mouse_report(screen: &vt100::Screen, m: &MouseEvent, col: u16, row: u16) -> Option<Vec<u8>> {
    use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
    let mode = screen.mouse_protocol_mode();
    let (button, release, motion) = match m.kind {
        MouseEventKind::Down(b) => (btn(b), false, false),
        MouseEventKind::Up(b) => (btn(b), true, false),
        MouseEventKind::Drag(b) => (btn(b) + 32, false, true),
        MouseEventKind::Moved => (35, false, true),
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        _ => return None,
    };
    let wanted = match mode {
        M::None => false,
        M::Press => !release && !motion,
        M::PressRelease => !motion,
        M::ButtonMotion => !matches!(m.kind, MouseEventKind::Moved),
        M::AnyMotion => true,
    };
    if !wanted {
        return None;
    }
    let (x, y) = (col + 1, row + 1);
    Some(match screen.mouse_protocol_encoding() {
        E::Sgr => format!("\x1b[<{button};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes(),
        _ => {
            let b = if release { 3 } else { button };
            if x > 223 || y > 223 {
                return None;
            }
            vec![0x1b, b'[', b'M', 32 + b as u8, 32 + x as u8, 32 + y as u8]
        }
    })
}

fn btn(b: MouseButton) -> u16 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// True while the menu or another overlay covers the workspace.
pub fn covered(state: &AppState) -> bool {
    !matches!(state.overlay, Overlay::None)
}
