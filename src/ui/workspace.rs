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
use crate::app::workspace::{AgentsFocus, MenuItem};

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
    let n = state.project_sessions().len() as u16;
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
pub fn session_rect(state: &AppState) -> Rect {
    let r = geometry(state).session;
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
        terminal_pane::draw(f, state, g.session);
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
            let mut spans = vec![Span::raw(p.name.clone())];
            if n > 0 {
                spans.push(Span::styled(
                    format!("  {n}"),
                    Style::default().fg(Color::Cyan),
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
    let b = block(state, format!(" Sessions: {name} "), focused);
    let ids = state.project_sessions();
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
            ListItem::new(Line::from(vec![
                Span::raw(format!("#{} {}  ", s.id, s.profile.name)),
                Span::styled(s.state.label(), state_style),
                Span::styled(
                    format!("  {}:{:02}", since / 60, since % 60),
                    Style::default().add_modifier(Modifier::DIM),
                ),
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

/// The profile menu overlay.
pub fn draw_menu(f: &mut Frame<'_>, area: Rect, items: &[MenuItem], selected: usize) {
    let width = 60.min(area.width.saturating_sub(2)).max(20);
    let height = (items.len() as u16 + 4)
        .min(area.height.saturating_sub(2))
        .max(5);
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    f.render_widget(Clear, r);
    let rows: Vec<ListItem<'_>> = items
        .iter()
        .map(|i| match &i.available {
            Ok(()) => ListItem::new(i.label.clone()),
            Err(e) => ListItem::new(Line::styled(
                format!("{}  ({e})", i.label),
                Style::default().add_modifier(Modifier::DIM),
            )),
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(
        List::new(rows)
            .block(
                Block::bordered()
                    .title(" New session: choose an agent ")
                    .title_bottom(" Enter start · Esc cancel "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        r,
        &mut st,
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
