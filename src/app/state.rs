//! Application state: focus, pane sizes and what the panes show.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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

#[derive(Debug, Clone)]
pub struct AppState {
    pub focus: Pane,
    pub sizes: PaneSizes,
    /// The project opened with `PATH`, if any.
    pub project: Option<PathBuf>,
    /// Configuration problems, shown as `file:line: message`.
    pub notices: Vec<String>,
    pub quit: bool,
    /// Event-loop iterations: the liveness counter for `--bench-stats` (R-12).
    pub ticks: u64,
}

impl AppState {
    pub fn new(project: Option<PathBuf>, notices: Vec<String>) -> Self {
        AppState {
            focus: Pane::Files,
            sizes: PaneSizes::default(),
            project,
            notices,
            quit: false,
            ticks: 0,
        }
    }

    /// Applies a key press. Returns true when something visible changed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Windows reports key releases too; only presses (and repeats) act.
        if key.kind == KeyEventKind::Release {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') if ctrl => {
                self.quit = true;
                false
            }
            KeyCode::BackTab => self.focus(self.focus.prev()),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.focus(self.focus.prev())
            }
            KeyCode::Tab => self.focus(self.focus.next()),
            _ => false,
        }
    }

    fn focus(&mut self, pane: Pane) -> bool {
        let changed = self.focus != pane;
        self.focus = pane;
        changed
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
}
