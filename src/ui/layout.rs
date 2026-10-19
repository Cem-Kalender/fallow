//! Pane layout (contracts/ui-commands.md "Layout"): three panes plus the hint bar, collapsing to
//! tabs below 100×24 and to a "terminal too small" notice below 40×10. The same geometry is
//! used to draw and to hit-test the mouse (FR-063).

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::{confirm, editor, files, help, hints, palette, processes, prompt, summary};
use crate::app::state::{AppState, Overlay, Pane, PaneSizes};

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

/// Where everything is. Panes not shown (in tab mode) have empty rects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    pub tabs: Rect,
    pub files: Rect,
    pub editor: Rect,
    pub list: Rect,
    pub system: Rect,
    pub hints: Rect,
    pub full: bool,
}

impl Geometry {
    pub fn pane_at(&self, x: u16, y: u16) -> Option<Pane> {
        let inside = |r: Rect| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
        if inside(self.files) {
            Some(Pane::Files)
        } else if inside(self.editor) {
            Some(Pane::Editor)
        } else if inside(self.list) || inside(self.system) {
            Some(Pane::Processes)
        } else {
            None
        }
    }
}

/// A draggable border (full layout only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Border {
    FilesEditor,
    EditorProcesses,
    ListSystem,
}

pub fn geometry(area: Rect, s: PaneSizes, focus: Pane) -> Geometry {
    match mode(area) {
        Mode::TooSmall => Geometry::default(),
        Mode::Tabs => {
            let [tabs, body, hints] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .areas(area);
            let mut g = Geometry {
                tabs,
                hints,
                ..Default::default()
            };
            match focus {
                Pane::Files => g.files = body,
                Pane::Editor => g.editor = body,
                Pane::Processes => {
                    let rows = s.system_rows.min(body.height.saturating_sub(3));
                    [g.list, g.system] =
                        Layout::vertical([Constraint::Min(3), Constraint::Length(rows)])
                            .areas(body);
                }
            }
            g
        }
        Mode::Full => {
            let [main, hints] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
            let [files, editor, procs] = Layout::horizontal([
                Constraint::Percentage(s.files_pct),
                Constraint::Fill(1),
                Constraint::Percentage(s.processes_pct),
            ])
            .areas(main);
            let [list, system] =
                Layout::vertical([Constraint::Min(3), Constraint::Length(s.system_rows)])
                    .areas(procs);
            Geometry {
                files,
                editor,
                list,
                system,
                hints,
                full: true,
                ..Default::default()
            }
        }
    }
}

/// The border under the pointer, if any.
pub fn border_at(g: &Geometry, x: u16, y: u16) -> Option<Border> {
    if !g.full {
        return None;
    }
    let main_bottom = g.files.bottom();
    if y < main_bottom && (x + 1 == g.files.right() || x == g.editor.x) {
        return Some(Border::FilesEditor);
    }
    if y < main_bottom && (x + 1 == g.editor.right() || x == g.list.x) {
        return Some(Border::EditorProcesses);
    }
    if y == g.system.y && x > g.system.x && x + 1 < g.system.right() {
        return Some(Border::ListSystem);
    }
    None
}

/// Applies a border drag to the pane sizes, keeping every pane usable.
pub fn drag(s: &mut PaneSizes, b: Border, area: Rect, x: u16, y: u16) {
    let w = area.width.max(1) as u32;
    let pct = |col: u32| (col * 100 / w) as u16;
    match b {
        Border::FilesEditor => {
            s.files_pct = pct(x as u32 + 1).clamp(10, 50).min(80 - s.processes_pct);
        }
        Border::EditorProcesses => {
            s.processes_pct = pct(w.saturating_sub(x as u32))
                .clamp(20, 60)
                .min(80 - s.files_pct);
        }
        Border::ListSystem => {
            let main_bottom = area.height.saturating_sub(1);
            let max = main_bottom.saturating_sub(6).max(4);
            s.system_rows = main_bottom.saturating_sub(y).clamp(4, max);
        }
    }
}

pub fn draw(f: &mut Frame<'_>, state: &AppState) {
    let area = f.area();
    if mode(area) == Mode::TooSmall {
        return draw_too_small(f, area);
    }
    let g = geometry(area, state.sizes, state.focus);
    if !g.full {
        f.render_widget(tab_line(state.focus), g.tabs);
    }
    if g.files.width > 0 {
        files::draw(f, state, g.files);
    }
    if g.editor.width > 0 {
        editor::draw(f, state, g.editor);
    }
    if g.list.width > 0 {
        processes::draw(f, state, g.list);
        summary::draw(f, state, g.system);
    }
    hints::draw(f, state, g.hints);
    match &state.overlay {
        Overlay::None => {}
        Overlay::Help { scroll } => help::draw(f, area, *scroll),
        Overlay::Palette { query, selected } => palette::draw(f, area, query, *selected),
        Overlay::Confirm { dialog, .. } => confirm::draw(f, area, dialog),
        Overlay::Prompt { kind, input, error } => {
            prompt::draw(f, area, kind, input, error.as_deref())
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

/// A pane's block, highlighted when focused.
pub fn pane_block(state: &AppState, pane: Pane, title: Line<'static>) -> Block<'static> {
    let border = if state.focus == pane {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::bordered().title(title).border_style(border)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    pub fn render(w: u16, h: u16, state: &AppState) -> String {
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
        assert!(
            screen.contains("scanning…"),
            "before the first snapshot\n{screen}"
        );
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
        assert!(screen.contains("scanning…"));
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
    fn notices_are_shown() {
        let s = AppState::new(None, vec!["settings.toml:3: bad value".into()]);
        assert!(render(120, 40, &s).contains("settings.toml:3: bad value"));
    }

    #[test]
    fn borders_drag_within_limits() {
        let area = Rect::new(0, 0, 120, 40);
        let mut sizes = PaneSizes::default();
        let g = geometry(area, sizes, Pane::Files);
        assert_eq!(
            border_at(&g, g.files.right() - 1, 5),
            Some(Border::FilesEditor)
        );
        assert_eq!(border_at(&g, g.list.x, 5), Some(Border::EditorProcesses));
        assert_eq!(
            border_at(&g, g.system.x + 3, g.system.y),
            Some(Border::ListSystem)
        );
        assert_eq!(border_at(&g, 50, 5), None);
        drag(&mut sizes, Border::FilesEditor, area, 0, 5);
        assert_eq!(sizes.files_pct, 10);
        drag(&mut sizes, Border::EditorProcesses, area, 40, 5);
        assert_eq!(sizes.processes_pct, 60);
        drag(&mut sizes, Border::ListSystem, area, 100, 2);
        assert!((4..=33).contains(&sizes.system_rows));
    }
}
