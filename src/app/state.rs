//! Application state: focus, panes, overlays, the latest snapshot and the command dispatcher.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use super::commands::{self, Context};
use super::editing::{EditorPrefs, FindBar, Pending, PromptKind};
use super::events::AppEvent;
use super::keymap;
use super::workspace::{StartMenu, View, Workspace};
use crate::clipboard::Clipboard;
use crate::editor::selection::LineInput;
use crate::editor::watch::FileWatcher;
use crate::editor::{Editor, Project};
use crate::procmon::snapshot::{ProcessKey, Snapshot};
use crate::procmon::worker::Request;
use crate::projects::ProjectList;
use crate::session::SessionManager;
use crate::session::profiles::AgentProfile;
use crate::ui::confirm::Dialog;
use crate::ui::layout::{self, Border};
use crate::ui::processes;

/// The three panes, in `Tab` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Files,
    Editor,
    Processes,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Pane::Files, Pane::Editor, Pane::Processes];

    pub fn title(self) -> &'static str {
        match self {
            Pane::Files => "Files",
            Pane::Editor => "Editor",
            Pane::Processes => "LLM Processes",
        }
    }

    fn index(self) -> usize {
        Pane::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    pub fn next(self) -> Pane {
        Pane::ALL[(self.index() + 1) % Pane::ALL.len()]
    }

    pub fn prev(self) -> Pane {
        Pane::ALL[(self.index() + Pane::ALL.len() - 1) % Pane::ALL.len()]
    }

    pub fn context(self) -> Context {
        match self {
            Pane::Files => Context::Files,
            Pane::Editor => Context::Editor,
            Pane::Processes => Context::Processes,
        }
    }
}

/// Pane sizes for the full layout. The editor takes what the other two leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneSizes {
    /// Width of the Files pane, percent of the terminal width.
    pub files_pct: u16,
    /// Width of the LLM Processes column, percent of the terminal width.
    pub processes_pct: u16,
    /// Height of the System area under the process list, rows including borders.
    pub system_rows: u16,
}

impl Default for PaneSizes {
    fn default() -> Self {
        PaneSizes {
            files_pct: 20,
            processes_pct: 35,
            system_rows: 7,
        }
    }
}

/// A full-screen layer over the panes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Overlay {
    #[default]
    None,
    Help {
        scroll: u16,
    },
    Palette {
        query: String,
        selected: usize,
    },
    /// A confirmation dialog (T069) and what confirming does.
    Confirm {
        dialog: Dialog,
        action: Pending,
    },
    /// A one-line text prompt (open project, go to line, file names).
    Prompt {
        kind: PromptKind,
        input: LineInput,
        error: Option<String>,
    },
    /// The start menu for a new session (002): agent and where it runs.
    Menu(StartMenu),
}

/// Selection and expansion in the process panel.
#[derive(Debug, Clone, Default)]
pub struct PanelState {
    pub selected: usize,
    pub offset: usize,
    pub expanded: HashSet<ProcessKey>,
    pub sort: processes::Sort,
}

const MESSAGE_FOR: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub struct AppState {
    pub focus: Pane,
    pub sizes: PaneSizes,
    /// The editor view's project: the selected project's lazy tree (001 editor).
    pub project: Option<Project>,
    /// The remembered list of projects (002).
    pub projects: ProjectList,
    pub sessions: SessionManager,
    pub profiles: Vec<AgentProfile>,
    pub workspace: Workspace,
    /// The application event channel, for sessions' reader and waiter threads.
    pub events: Option<Sender<AppEvent>>,
    /// Quit was confirmed with sessions running: waiting for them to stop (since then).
    pub quitting: Option<Instant>,
    pub editor: Editor,
    pub find: Option<FindBar>,
    /// The last find query, offered again by the next `Ctrl-F`.
    pub last_query: String,
    pub clipboard: Clipboard,
    pub watcher: FileWatcher,
    /// The watched set must be recomputed (something was opened, closed or expanded).
    pub watch_dirty: bool,
    /// Files that changed on disk under unsaved edits, waiting for their dialog.
    pub conflicts: Vec<PathBuf>,
    pub prefs: EditorPrefs,
    /// Project searches run (for `--bench-stats`).
    pub searches: u64,
    pub last_click: Option<(u16, u16, Instant)>,
    /// A drag that started in the editor text selects.
    pub drag_select: bool,
    /// More input is queued behind the event being handled (a paste delivered as keys).
    pub burst: bool,
    /// Configuration problems and startup notices, shown as `file:line: message`.
    pub notices: Vec<String>,
    pub quit: bool,
    /// Event-loop iterations: the liveness counter for `--bench-stats` (R-12).
    pub ticks: u64,
    pub snapshot: Option<Arc<Snapshot>>,
    pub panel: PanelState,
    pub overlay: Overlay,
    /// A transient hint-bar message and when it was set.
    pub message: Option<(String, Instant)>,
    /// All-users mode is active (FR-031).
    pub all_users: bool,
    /// The terminal size, for mouse hit-testing and paging.
    pub area: Rect,
    pub drag: Option<Border>,
    pub requests: Option<Sender<Request>>,
}

impl AppState {
    pub fn new(project: Option<PathBuf>, mut notices: Vec<String>) -> Self {
        let project = project.and_then(|p| match Project::open(&p) {
            Ok(p) => Some(p),
            Err(e) => {
                notices.push(format!("{}: {e}", p.display()));
                None
            }
        });
        let mut projects = ProjectList::default();
        if let Some(p) = &project {
            projects.push(p.root.clone(), None, true);
        }
        AppState {
            focus: Pane::Files,
            sizes: PaneSizes::default(),
            watch_dirty: project.is_some(),
            project,
            projects,
            sessions: SessionManager::default(),
            profiles: crate::session::profiles::builtins(),
            workspace: Workspace::default(),
            events: None,
            quitting: None,
            editor: Editor::default(),
            find: None,
            last_query: String::new(),
            clipboard: Clipboard::default(),
            watcher: FileWatcher::default(),
            conflicts: Vec::new(),
            prefs: EditorPrefs::default(),
            searches: 0,
            last_click: None,
            drag_select: false,
            burst: false,
            notices,
            quit: false,
            ticks: 0,
            snapshot: None,
            panel: PanelState::default(),
            overlay: Overlay::None,
            message: None,
            all_users: false,
            area: Rect::new(0, 0, 120, 40),
            drag: None,
            requests: None,
        }
    }

    pub fn set_snapshot(&mut self, s: Arc<Snapshot>) {
        self.snapshot = Some(s);
        self.clamp_selection();
    }

    /// The row index of `key` in the LLM panel, if shown.
    pub fn panel_index_of(&self, key: ProcessKey) -> Option<usize> {
        self.rows().iter().position(|r| r.key == key)
    }

    /// The process selected in the LLM panel.
    pub fn selected_process(&self) -> Option<ProcessKey> {
        self.rows().get(self.panel.selected).map(|r| r.key)
    }

    /// Drops an expired message. Returns true if the screen changed.
    pub fn tick(&mut self) -> bool {
        let stops = self.tick_stops();
        if self
            .message
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= MESSAGE_FOR)
        {
            self.message = None;
            return true;
        }
        stops
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    /// Applies a key press. Returns true when something visible changed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Windows reports key releases too; only presses (and repeats) act.
        if key.kind == KeyEventKind::Release {
            return false;
        }
        match &self.overlay {
            Overlay::Help { scroll } => return self.help_key(key, *scroll),
            Overlay::Palette { .. } => return self.palette_key(key),
            Overlay::Confirm { .. } => return self.confirm_key(key),
            Overlay::Prompt { .. } => return self.prompt_key(key),
            Overlay::Menu(_) => return self.menu_key(key),
            Overlay::None => {}
        }
        if self.workspace.view == View::Agents {
            if self.workspace.focus == super::workspace::AgentsFocus::Session {
                return self.workspace_key(key).unwrap_or(false);
            }
            if let Some(changed) = self.after_leave_key(&key) {
                return changed;
            }
            match keymap::lookup(&key, self.agents_context(), false) {
                Some(id) => return self.run(id),
                None => return false,
            }
        }
        let mut context = self.focus.context();
        if self.focus == Pane::Editor {
            if let Some(changed) = self.editor_key(key) {
                return changed;
            }
            // No buffer, or focus released with Esc: Tab switches panes (FR-065).
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                context = Context::Global;
            }
        }
        match keymap::lookup(&key, context, self.text_input()) {
            Some(id) => self.run(id),
            None => false,
        }
    }

    fn help_key(&mut self, key: KeyEvent, scroll: u16) -> bool {
        let page = self.area.height.saturating_sub(6).max(1);
        let scroll = match key.code {
            KeyCode::Char('q' | 'Q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.overlay = Overlay::None;
                return self.run("quit");
            }
            KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.overlay = Overlay::None;
                return true;
            }
            KeyCode::Up => scroll.saturating_sub(1),
            KeyCode::Down => scroll.saturating_add(1),
            KeyCode::PageUp => scroll.saturating_sub(page),
            KeyCode::PageDown | KeyCode::Char(' ') => scroll.saturating_add(page),
            KeyCode::Home => 0,
            KeyCode::End => u16::MAX,
            _ => return false,
        };
        self.overlay = Overlay::Help { scroll };
        true
    }

    fn palette_key(&mut self, key: KeyEvent) -> bool {
        let Overlay::Palette { query, selected } = &mut self.overlay else {
            return false;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Char('p' | 'P') if ctrl => self.overlay = Overlay::None,
            KeyCode::Char('q' | 'Q') if ctrl => {
                self.overlay = Overlay::None;
                return self.run("quit");
            }
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected += 1,
            KeyCode::Backspace => {
                query.pop();
                *selected = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                query.push(c);
                *selected = 0;
            }
            KeyCode::Enter => {
                let matches = crate::ui::palette::matches(query);
                let pick = matches
                    .get((*selected).min(matches.len().saturating_sub(1)))
                    .map(|c| c.id);
                self.overlay = Overlay::None;
                if let Some(id) = pick {
                    self.run(id);
                }
            }
            _ => return false,
        }
        if let Overlay::Palette { query, selected } = &mut self.overlay {
            let n = crate::ui::palette::matches(query).len();
            *selected = (*selected).min(n.saturating_sub(1));
        }
        true
    }

    /// Runs a command from the registry. Returns true when something visible changed.
    pub fn run(&mut self, id: &str) -> bool {
        match id {
            "quit" => self.request_quit(),
            "help" => {
                self.overlay = Overlay::Help { scroll: 0 };
                true
            }
            "palette" => {
                self.overlay = Overlay::Palette {
                    query: String::new(),
                    selected: 0,
                };
                true
            }
            "pane.next" | "pane.prev" | "pane.files" | "pane.editor" | "pane.processes"
                if self.workspace.view == View::Agents =>
            {
                self.run_workspace(id).unwrap_or(false)
            }
            "pane.next" => self.focus(self.focus.next()),
            "pane.prev" => self.focus(self.focus.prev()),
            "pane.files" => self.focus(Pane::Files),
            "pane.editor" => self.focus(Pane::Editor),
            "pane.processes" => self.focus(Pane::Processes),
            "proc.up" => self.move_selection(-1),
            "proc.down" => self.move_selection(1),
            "proc.page_up" => self.move_selection(-(self.page() as isize)),
            "proc.page_down" => self.move_selection(self.page() as isize),
            "proc.home" => self.move_selection(isize::MIN / 2),
            "proc.end" => self.move_selection(isize::MAX / 2),
            "proc.expand" => self.set_expanded(Some(true)),
            "proc.collapse" => self.set_expanded(Some(false)),
            "proc.toggle" => self.set_expanded(None),
            "proc.sort.mem" => self.sort_by(processes::Col::Mem),
            "proc.sort.cpu" => self.sort_by(processes::Col::Cpu),
            "proc.sort.uptime" => self.sort_by(processes::Col::Up),
            "proc.sort.name" => self.sort_by(processes::Col::Name),
            "proc.sort.status" => self.sort_by(processes::Col::Status),
            "proc.sort.confidence" => self.sort_by(processes::Col::Conf),
            other => {
                if let Some(changed) = self.run_workspace(other) {
                    return changed;
                }
                if let Some(changed) = self.run_editing(other) {
                    return changed;
                }
                let title = commands::find(other).map_or(other, |c| c.title);
                self.say(format!("{title}: not available yet"));
                true
            }
        }
    }

    /// Sorts by `column` (again: reverse), keeping the same process selected.
    pub fn sort_by(&mut self, column: processes::Col) -> bool {
        let selected = self.rows().get(self.panel.selected).map(|r| r.key);
        self.panel.sort = self.panel.sort.select(column);
        if let Some(key) = selected {
            if let Some(i) = self.rows().iter().position(|r| r.key == key) {
                self.panel.selected = i;
            }
        }
        true
    }

    fn focus(&mut self, pane: Pane) -> bool {
        let changed = self.focus != pane;
        self.focus = pane;
        changed
    }

    fn rows(&self) -> Vec<processes::Row> {
        self.snapshot.as_deref().map_or_else(Vec::new, |s| {
            processes::rows(s, &self.panel.expanded, self.panel.sort)
        })
    }

    fn page(&self) -> usize {
        let g = layout::geometry(self.area, self.sizes, self.focus);
        (g.list.height as usize).saturating_sub(7).max(1)
    }

    fn clamp_selection(&mut self) {
        let n = self.rows().len();
        self.panel.selected = self.panel.selected.min(n.saturating_sub(1));
    }

    fn move_selection(&mut self, by: isize) -> bool {
        let n = self.rows().len();
        if n == 0 {
            return false;
        }
        let new = (self.panel.selected as isize)
            .saturating_add(by)
            .clamp(0, n as isize - 1) as usize;
        self.select(new)
    }

    /// Selects a row; tells the worker (Windows `on_select` working-directory policy).
    pub fn select(&mut self, index: usize) -> bool {
        let rows = self.rows();
        let Some(row) = rows.get(index) else {
            return false;
        };
        let changed = self.panel.selected != index;
        self.panel.selected = index;
        if let Some(tx) = &self.requests {
            let _ = tx.send(Request::Selected(row.key));
        }
        changed
    }

    /// Expands (`Some(true)`), collapses (`Some(false)`) or toggles the selected group. On a
    /// member row, collapse goes to its group.
    fn set_expanded(&mut self, to: Option<bool>) -> bool {
        let rows = self.rows();
        let Some(row) = rows.get(self.panel.selected) else {
            return false;
        };
        let root = row.group_root;
        let open = self.panel.expanded.contains(&root);
        let want = to.unwrap_or(!open);
        if row.depth > 0 && !want {
            // Collapse from a member: jump to the group's root row.
            if let Some(i) = rows.iter().position(|r| r.key == root && r.depth == 0) {
                self.panel.selected = i;
            }
        }
        if !row.expandable && row.depth == 0 {
            return false;
        }
        if want {
            self.panel.expanded.insert(root);
        } else {
            self.panel.expanded.remove(&root);
        }
        self.clamp_selection();
        want != open
    }

    /// Mouse (FR-063): click focuses a pane and selects a process row, the wheel scrolls the
    /// pane under the pointer, dragging a border resizes panes.
    pub fn handle_mouse(&mut self, m: MouseEvent) -> bool {
        let g = layout::geometry(self.area, self.sizes, self.focus);
        let (x, y) = (m.column, m.row);
        // Border drags (pane resizing) stay here; everything else goes to the editing panes
        // first.
        let border_press = matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && layout::border_at(&g, x, y).is_some();
        if let Some(changed) = self.workspace_mouse(m) {
            return changed;
        }
        if !border_press && self.drag.is_none() {
            if let Some(changed) = self.editing_mouse(m, g.pane_at(x, y)) {
                return changed;
            }
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(b) = layout::border_at(&g, x, y) {
                    self.drag = Some(b);
                    return false;
                }
                if !matches!(self.overlay, Overlay::None) {
                    return false;
                }
                let Some(pane) = g.pane_at(x, y) else {
                    return false;
                };
                let mut changed = self.focus(pane);
                if pane == Pane::Processes {
                    if let Some(col) = processes::header_at(&g.list, self.all_users, x, y) {
                        return self.sort_by(col) || changed;
                    }
                    if let Some(i) = processes::row_at(&g.list, self.panel.selected, y) {
                        changed |= self.select(i) || true;
                    }
                }
                changed
            }
            MouseEventKind::Drag(MouseButton::Left) => match self.drag {
                Some(b) => {
                    let before = self.sizes;
                    layout::drag(&mut self.sizes, b, self.area, x, y);
                    before != self.sizes
                }
                None => false,
            },
            MouseEventKind::Up(_) => {
                self.drag = None;
                false
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let step = if m.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                if let Overlay::Help { scroll } = self.overlay {
                    let s = (scroll as i32 + step).max(0) as u16;
                    self.overlay = Overlay::Help { scroll: s };
                    return true;
                }
                match g.pane_at(x, y) {
                    Some(Pane::Processes) => self.move_selection(step as isize),
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn tab_cycles_forward_and_back() {
        let mut s = AppState::new(None, Vec::new());
        s.workspace.view = View::Editor;
        assert_eq!(s.focus, Pane::Files);
        assert!(s.handle_key(press(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(s.focus, Pane::Editor);
        s.handle_key(press(KeyCode::Tab, KeyModifiers::NONE));
        s.handle_key(press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(s.focus, Pane::Files, "wraps around");
        s.handle_key(press(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(s.focus, Pane::Processes);
        // The kitty protocol can report Shift-Tab as Tab with Shift.
        s.handle_key(press(KeyCode::Tab, KeyModifiers::SHIFT));
        assert_eq!(s.focus, Pane::Editor);
    }

    #[test]
    fn tab_cycles_the_agent_workspace() {
        use crate::app::workspace::AgentsFocus;
        let mut s = AppState::new(None, Vec::new());
        assert_eq!(s.workspace.focus, AgentsFocus::Projects);
        s.handle_key(press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(s.workspace.focus, AgentsFocus::Sessions);
        s.handle_key(press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(s.workspace.focus, AgentsFocus::Processes);
        s.handle_key(press(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(s.workspace.focus, AgentsFocus::Sessions);
        s.handle_key(press(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(s.workspace.view, View::Editor);
        s.handle_key(press(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(s.workspace.view, View::Agents);
        assert_eq!(
            s.workspace.focus,
            AgentsFocus::Sessions,
            "focus is kept per view"
        );
    }

    #[test]
    fn the_leave_key_leaves_and_an_immediate_second_press_goes_back() {
        use crate::app::workspace::AgentsFocus;
        use crate::session::profiles::builtins;
        let mut s = AppState::new(None, Vec::new());
        let sess = crate::session::Session::detached(1, 1, builtins().remove(0));
        s.sessions.sessions.push(sess);
        s.workspace.active = Some(1);
        s.workspace.focus = AgentsFocus::Session;
        let leave = press(KeyCode::Char(']'), KeyModifiers::CONTROL);
        s.handle_key(leave);
        assert_eq!(s.workspace.focus, AgentsFocus::Sessions, "one press leaves");
        s.handle_key(leave);
        assert_eq!(
            s.workspace.focus,
            AgentsFocus::Session,
            "a second press goes back"
        );
        // Leaving, another key, then the leave key again: no double press.
        s.handle_key(leave);
        s.handle_key(press(KeyCode::Down, KeyModifiers::NONE));
        s.workspace.focus = AgentsFocus::Session;
        s.handle_key(leave);
        assert_eq!(s.workspace.focus, AgentsFocus::Sessions);
        // Re-entering a session cancels a pending double press (fast switching).
        s.workspace.focus = AgentsFocus::Session;
        s.workspace.leave_armed = Some(std::time::Instant::now());
        s.run("session.focus");
        s.handle_key(leave);
        assert_eq!(s.workspace.focus, AgentsFocus::Sessions);
    }

    #[test]
    fn ctrl_q_quits_and_releases_are_ignored() {
        let mut s = AppState::new(None, Vec::new());
        let mut release = press(KeyCode::Tab, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!s.handle_key(release));
        assert_eq!(s.focus, Pane::Files);
        s.handle_key(press(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!s.quit, "plain q does not quit");
        s.handle_key(press(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(s.quit);
    }

    #[test]
    fn help_and_palette_open_and_close() {
        let mut s = AppState::new(None, Vec::new());
        s.handle_key(press(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(s.overlay, Overlay::Help { scroll: 0 });
        s.handle_key(press(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(s.overlay, Overlay::None);
        s.handle_key(press(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert!(s.text_input());
        // '?' is typed into the palette filter, not opening help.
        s.handle_key(press(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(
            s.overlay,
            Overlay::Palette {
                query: "?".into(),
                selected: 0
            }
        );
        for _ in 0..1 {
            s.handle_key(press(KeyCode::Backspace, KeyModifiers::NONE));
        }
        for c in "help".chars() {
            s.handle_key(press(KeyCode::Char(c), KeyModifiers::NONE));
        }
        s.handle_key(press(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            s.overlay,
            Overlay::Help { scroll: 0 },
            "palette entry Help opens help"
        );
    }

    #[test]
    fn unavailable_commands_say_so() {
        let mut s = AppState::new(None, Vec::new());
        s.workspace.view = View::Editor;
        s.focus = Pane::Processes;
        s.handle_key(press(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(s.message.as_ref().unwrap().0, "Pause: not available yet");
    }
}
