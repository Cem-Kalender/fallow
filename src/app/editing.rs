//! US3 behavior on top of [`AppState`]: the Files pane and its file operations, the editor and
//! its buffer commands, the find bar, go to line, the clipboard, the prompts and dialogs, and
//! the background sources (search results, change notifications).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::commands::Context;
use super::keymap;
use super::state::{AppState, Overlay, Pane};
use crate::editor::buffer::DiskEvent;
use crate::editor::search::{SearchSession, SearchState};
use crate::editor::selection::LineInput;
use crate::editor::{Project, fileops, watch};
use crate::ui::confirm::{self, Dialog, Outcome};
use crate::ui::{editor as editor_ui, files as files_ui, find as find_ui, goto, layout};

/// The editor preferences from settings.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPrefs {
    pub tab_width: usize,
    pub read_only_bytes: u64,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        EditorPrefs {
            tab_width: 4,
            read_only_bytes: 10 * 1024 * 1024,
        }
    }
}

/// Where the find bar searches (FR-005). Always shown in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    File,
    Project,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::File => "this file",
            Scope::Project => "whole project",
        }
    }
}

/// The find bar at the bottom of the editor.
#[derive(Debug)]
pub struct FindBar {
    pub input: LineInput,
    pub scope: Scope,
    pub session: Option<SearchSession>,
    /// The project results list is shown.
    pub results_open: bool,
    pub selected: usize,
    pub offset: usize,
    /// The last file-scope search found nothing.
    pub no_match: bool,
}

/// A text prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    OpenProject,
    GotoLine,
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    /// Search the active session's scrollback (002).
    SearchSession,
    /// 002 US4: rename a project in the list / point it to another folder.
    RenameProject(crate::projects::ProjectId),
    RelocateProject(crate::projects::ProjectId),
    /// 003: write on a board as the user; the orchestration view comes back afterwards.
    BoardPost(
        crate::orchestration::board::BoardId,
        crate::ui::orchestration::OrchView,
    ),
    /// 003: a new task as the user: `#2 text after:e1,e2` or `profile@project text`.
    NewTask(crate::ui::orchestration::OrchView),
    /// 003: edit a task's text (and approve it, for an agent's request).
    EditTask(u32, bool, crate::ui::orchestration::OrchView),
    /// 003: search the activity log (empty clears).
    SearchActivity(crate::ui::orchestration::OrchView),
    /// 003: edit a delivery's text, then approve it.
    EditDelivery(u64, crate::ui::orchestration::OrchView),
    /// 003: signal an event as the user: "name [message]".
    SignalEvent(
        crate::orchestration::board::BoardId,
        crate::ui::orchestration::OrchView,
    ),
}

impl PromptKind {
    pub fn title(&self) -> &'static str {
        match self {
            PromptKind::OpenProject => "Open project (folder path)",
            PromptKind::GotoLine => "Go to line (line or line:column)",
            PromptKind::NewFile(_) => "New file name",
            PromptKind::NewFolder(_) => "New folder name",
            PromptKind::Rename(_) => "Rename to",
            PromptKind::SearchSession => "Search the session (upward from the view)",
            PromptKind::RenameProject(_) => "Project name",
            PromptKind::RelocateProject(_) => "New folder for this project",
            PromptKind::BoardPost(..) => "Write on the board (as you)",
            PromptKind::SignalEvent(..) => "Signal an event as you: name [message]",
            PromptKind::EditDelivery(..) => "Text to deliver (Enter approves)",
            PromptKind::NewTask(..) => {
                "New task: #2 or claude@api, then the text; after:event waits for it"
            }
            PromptKind::EditTask(..) => "Task text",
            PromptKind::SearchActivity(..) => "Search the activity log (empty shows everything)",
        }
    }
}

/// What a confirmation dialog is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Delete(PathBuf),
    Quit,
    CloseBuffer(usize),
    /// FR-006 "keep mine / load disk" for this buffer's file. Confirm = load disk.
    DiskConflict(PathBuf),
    /// 002 FR-035: closing a session in a worktree. With `offer_remove`, confirm removes the
    /// clean worktree and cancel keeps it; both close the session. Without, confirm closes and
    /// cancel does nothing.
    CloseSession {
        id: u32,
        offer_remove: bool,
    },
    /// 002 FR-012: stop this session gracefully.
    StopSession(u32),
    /// 002 FR-012: force-stop this session (second confirmation).
    ForceStopSession(u32),
    /// 002 US4: remove a project with running sessions from the list.
    RemoveProject(crate::projects::ProjectId),
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

fn is_text_char(key: &KeyEvent) -> Option<char> {
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    match key.code {
        // AltGr arrives as Ctrl+Alt on Windows and produces text.
        KeyCode::Char(c) if (!ctrl && !alt) || (ctrl && alt) => Some(c),
        _ => None,
    }
}

impl AppState {
    // ----- context -----

    /// True while typing goes into a text input (FR-066: `?` is text there).
    pub fn text_input(&self) -> bool {
        match self.overlay {
            Overlay::Palette { .. } | Overlay::Prompt { .. } => true,
            Overlay::Confirm { .. }
            | Overlay::Help { .. }
            | Overlay::Menu(_)
            | Overlay::Orch(_)
            | Overlay::Grant(_)
            | Overlay::Info { .. } => false,
            Overlay::None if self.workspace.view == super::workspace::View::Agents => {
                self.workspace.focus == super::workspace::AgentsFocus::Session
            }
            Overlay::None => {
                self.focus == Pane::Editor
                    && (self.find.is_some()
                        || (self.editor.active().is_some() && !self.editor.released))
            }
        }
    }

    /// The context whose hints and bindings apply.
    pub fn context(&self) -> Context {
        match self.overlay {
            Overlay::Confirm { .. } => Context::Dialog,
            Overlay::Orch(_) => Context::Orchestration,
            Overlay::None if self.workspace.view == super::workspace::View::Agents => {
                self.agents_context()
            }
            Overlay::None if self.focus == Pane::Editor && self.find.is_some() => Context::FindBar,
            _ => self.focus.context(),
        }
    }

    fn editor_areas(&self) -> editor_ui::Areas {
        let g = layout::geometry(self.area, self.sizes, self.focus);
        editor_ui::areas(g.editor, self)
    }

    fn files_rows_visible(&self) -> usize {
        let g = layout::geometry(self.area, self.sizes, self.focus);
        (g.files.height as usize).saturating_sub(2).max(1)
    }

    // ----- dispatch -----

    /// Handles a key while the editor has focus. Returns `None` if the key was not for it.
    pub(super) fn editor_key(&mut self, key: KeyEvent) -> Option<bool> {
        if self.find.is_some() {
            return Some(self.find_key(key));
        }
        self.editor.active()?;
        if self.editor.released {
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                return None;
            }
            self.editor.released = false;
        }
        match keymap::lookup(&key, Context::Editor, true) {
            Some("editor.select") | None => {}
            Some(id) => return Some(self.run(id)),
        }
        let page = self.editor_areas().text.height.max(1) as isize;
        let tab = self.prefs.tab_width;
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let read_only = self.editor.active().is_some_and(|b| b.read_only);
        let b = self.editor.active_mut()?;
        let edits = matches!(
            key.code,
            KeyCode::Enter | KeyCode::Backspace | KeyCode::Delete
        ) || is_text_char(&key).is_some();
        if edits && read_only {
            self.say("Read-only: the file is over the size threshold");
            return Some(true);
        }
        match key.code {
            KeyCode::Left => b.left(shift),
            KeyCode::Right => b.right(shift),
            KeyCode::Up => b.vertical(-1, shift, tab),
            KeyCode::Down => b.vertical(1, shift, tab),
            KeyCode::PageUp => b.vertical(-page, shift, tab),
            KeyCode::PageDown => b.vertical(page, shift, tab),
            KeyCode::Home if ctrl => b.doc_start(shift),
            KeyCode::End if ctrl => b.doc_end(shift),
            KeyCode::Home => b.home(shift),
            KeyCode::End => b.end(shift),
            // Inside a burst of queued keys (a paste delivered as key events), Enter is a
            // literal line break: no auto-indent (FR-008).
            KeyCode::Enter if self.burst => b.newline_plain(),
            KeyCode::Enter => b.newline(),
            KeyCode::Backspace => b.backspace(),
            KeyCode::Delete => b.delete_forward(),
            _ => match is_text_char(&key) {
                Some(c) => b.type_char(c),
                None => return Some(false),
            },
        }
        self.editor.follow = true;
        Some(true)
    }

    /// Runs a US3 command. `None` if `id` is not one.
    pub(super) fn run_editing(&mut self, id: &str) -> Option<bool> {
        let tab = self.prefs.tab_width;
        Some(match id {
            "project.open" => self.prompt(PromptKind::OpenProject, ""),
            "files.up" => self.files_move(-1),
            "files.down" => self.files_move(1),
            "files.page_up" => self.files_move(-(self.files_rows_visible() as isize)),
            "files.page_down" => self.files_move(self.files_rows_visible() as isize),
            "files.home" => self.files_move(isize::MIN / 2),
            "files.end" => self.files_move(isize::MAX / 2),
            "files.open" => self.files_open(),
            "files.collapse" => self.files_collapse(),
            "files.expand" => self.files_expand(),
            "files.new_file" | "files.new_folder" => self.files_new(id == "files.new_file"),
            "files.rename" => self.files_rename(),
            "files.delete" => self.files_delete(),
            "editor.save" => self.save(),
            "editor.undo" => self.with_buffer(|b| {
                b.undo();
            }),
            "editor.redo" => self.with_buffer(|b| {
                b.redo();
            }),
            "editor.find" => self.open_find(),
            "editor.goto_line" => {
                if self.editor.active().is_none() {
                    self.say("Go to line: no file open");
                    true
                } else {
                    self.prompt(PromptKind::GotoLine, "")
                }
            }
            "editor.close" => self.close_buffer(),
            "editor.next_buffer" => {
                self.editor.cycle(1);
                true
            }
            "editor.prev_buffer" => {
                self.editor.cycle(-1);
                true
            }
            "editor.indent" => self.with_buffer(|b| b.indent(tab)),
            "editor.outdent" => self.with_buffer(|b| b.outdent(tab)),
            "editor.release" => {
                match self.editor.active_mut() {
                    Some(b) if b.selection().is_some() => {
                        let h = b.sel.head;
                        b.sel.anchor = h;
                    }
                    _ => self.editor.released = true,
                }
                true
            }
            "editor.select" => {
                self.say("Extend the selection with Shift and the arrow keys, or drag the mouse");
                true
            }
            "editor.select_all" => self.with_buffer(|b| b.select_all()),
            "editor.copy" => self.copy(false),
            "editor.cut" => self.copy(true),
            "editor.paste" => self.paste_key(),
            "find.scope" | "find.next" | "find.prev" | "find.close" => {
                if self.find.is_none() {
                    self.say("Open the find bar with Ctrl-F first");
                    true
                } else {
                    self.find_command(id, id == "find.next")
                }
            }
            "dialog.confirm" | "dialog.cancel" => {
                self.say("No dialog is open");
                true
            }
            _ => return None,
        })
    }

    fn with_buffer(&mut self, f: impl FnOnce(&mut crate::editor::buffer::Buffer)) -> bool {
        match self.editor.active_mut() {
            Some(b) => {
                f(b);
                self.editor.follow = true;
                true
            }
            None => {
                self.say("No file open");
                true
            }
        }
    }

    // ----- prompts and dialogs -----

    fn prompt(&mut self, kind: PromptKind, initial: &str) -> bool {
        let initial = match (&kind, initial) {
            (PromptKind::OpenProject, "") => self
                .project
                .as_ref()
                .map(|p| p.root.display().to_string())
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|d| d.display().to_string())
                })
                .unwrap_or_default(),
            _ => initial.to_string(),
        };
        self.overlay = Overlay::Prompt {
            kind,
            input: LineInput::selected(&initial),
            error: None,
        };
        true
    }

    pub(super) fn prompt_key(&mut self, key: KeyEvent) -> bool {
        let Overlay::Prompt { kind, input, error } = &mut self.overlay else {
            return false;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Char('q' | 'Q') if ctrl && !alt => {
                self.overlay = Overlay::None;
                return self.run("quit");
            }
            KeyCode::Char('p' | 'P') if ctrl && !alt => {
                self.overlay = Overlay::None;
                return self.run("palette");
            }
            KeyCode::Enter => {
                let (kind, text) = (kind.clone(), input.text().to_string());
                match self.submit_prompt(&kind, &text) {
                    Ok(()) => {
                        if matches!(self.overlay, Overlay::Prompt { .. }) {
                            self.overlay = Overlay::None;
                        }
                    }
                    Err(e) => {
                        if let Overlay::Prompt { error, .. } = &mut self.overlay {
                            *error = Some(e);
                        }
                    }
                }
            }
            _ => {
                *error = None;
                return text_key(
                    input,
                    key,
                    ctrl,
                    alt,
                    shift,
                    &mut self.clipboard,
                    &mut self.message,
                );
            }
        }
        true
    }

    fn submit_prompt(&mut self, kind: &PromptKind, text: &str) -> Result<(), String> {
        match kind {
            PromptKind::SearchSession => self.search_session(text),
            PromptKind::RenameProject(id) => self.rename_project(*id, text),
            PromptKind::RelocateProject(id) => self.relocate_project(*id, text),
            PromptKind::SearchActivity(view) => {
                let mut v = view.clone();
                v.query = text.trim().to_string();
                v.scroll = 0;
                self.overlay = Overlay::Orch(v);
                Ok(())
            }
            PromptKind::NewTask(view) => {
                let id = self.submit_new_task(text)?;
                self.overlay = Overlay::Orch(view.clone());
                self.say(format!("Queued T{id}"));
                Ok(())
            }
            PromptKind::EditTask(id, approve, view) => {
                let hub = self.hub.as_mut().ok_or("No orchestration")?;
                let text = crate::orchestration::board::clean_text(
                    text,
                    crate::orchestration::tasks::TEXT_MAX,
                )
                .map_err(|(_, m)| m)?;
                hub.edit_task(*id, text)?;
                if *approve {
                    hub.decide_task(*id, true);
                }
                self.overlay = Overlay::Orch(view.clone());
                Ok(())
            }
            PromptKind::EditDelivery(id, view) => {
                let hub = self.hub.as_mut().ok_or("No orchestration")?;
                let text = crate::orchestration::board::clean_text(
                    text,
                    crate::orchestration::delivery::TEXT_MAX,
                )
                .map_err(|(_, m)| m)?;
                hub.approve(*id, Some(text))?;
                self.overlay = Overlay::Orch(view.clone());
                Ok(())
            }
            PromptKind::SignalEvent(scope, view) => {
                let hub = self
                    .hub
                    .as_mut()
                    .ok_or("Start an agent session with orchestration first")?;
                let text = text.trim();
                let (name, message) = match text.split_once(char::is_whitespace) {
                    Some((n, m)) => (n, Some(m.trim().to_string()).filter(|m| !m.is_empty())),
                    None => (text, None),
                };
                if !crate::orchestration::events::valid_name(name) {
                    return Err("Event names: letters, digits and . _ : / # - (up to 64)".into());
                }
                if message
                    .as_ref()
                    .is_some_and(|m| m.len() > crate::orchestration::events::MESSAGE_MAX)
                {
                    return Err("The message is longer than 2 KiB".into());
                }
                hub.signal(
                    &(*scope, name.to_string()),
                    crate::orchestration::Party::User,
                    message,
                )?;
                self.overlay = Overlay::Orch(view.clone());
                Ok(())
            }
            PromptKind::BoardPost(board, view) => {
                let hub = self
                    .hub
                    .as_mut()
                    .ok_or("Start an agent session with orchestration first")?;
                hub.post_user(*board, text)?;
                self.overlay = Overlay::Orch(view.clone());
                Ok(())
            }
            PromptKind::GotoLine => {
                let (line, col) = goto::parse(text)?;
                let rows = self.editor_areas().text.height as usize;
                let b = self.editor.active_mut().ok_or("No file open")?;
                b.goto(line, col)?;
                b.center_on_cursor(rows);
                self.editor.follow = true;
                Ok(())
            }
            PromptKind::OpenProject => {
                let path = expand_home(text.trim());
                if path.as_os_str().is_empty() {
                    return Err("Enter a folder path".into());
                }
                let p = Project::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                // 002: the folder joins the project list and is selected.
                if self.projects.add(&p.root).is_err() {
                    self.projects.push(p.root.clone(), None, true);
                }
                self.save_projects();
                self.project = Some(p);
                self.focus = Pane::Files;
                self.watch_dirty = true;
                Ok(())
            }
            PromptKind::NewFile(dir) | PromptKind::NewFolder(dir) => {
                let made = if matches!(kind, PromptKind::NewFile(_)) {
                    fileops::new_file(dir, text)
                } else {
                    fileops::new_folder(dir, text)
                }
                .map_err(|e| e.to_string())?;
                self.after_fileop(dir, Some(&made));
                self.say(format!("Created {}", made.display()));
                Ok(())
            }
            PromptKind::Rename(from) => {
                let to = fileops::rename(from, text).map_err(|e| e.to_string())?;
                for b in &mut self.editor.buffers {
                    let Some(p) = b.path.clone() else { continue };
                    if let Ok(rest) = p.strip_prefix(from) {
                        let np = if rest.as_os_str().is_empty() {
                            to.clone()
                        } else {
                            to.join(rest)
                        };
                        b.renamed(np);
                    }
                }
                if let Some(dir) = from.parent() {
                    self.after_fileop(dir, Some(&to));
                }
                self.say(format!("Renamed to {}", to.display()));
                Ok(())
            }
        }
    }

    pub(super) fn confirm_key(&mut self, key: KeyEvent) -> bool {
        let Overlay::Confirm { dialog, .. } = &mut self.overlay else {
            return false;
        };
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl-Q in a dialog cancels it rather than quitting past it.
            self.overlay = Overlay::None;
            return true;
        }
        let outcome = dialog.key(key);
        self.finish_dialog(outcome)
    }

    fn finish_dialog(&mut self, outcome: Outcome) -> bool {
        let Overlay::Confirm { action, .. } = &self.overlay else {
            return false;
        };
        let action = action.clone();
        match outcome {
            Outcome::Pending => return true,
            Outcome::Cancelled => {
                self.overlay = Overlay::None;
                if let Pending::CloseSession {
                    id,
                    offer_remove: true,
                } = &action
                {
                    self.close_session(*id);
                    self.say("Closed; the worktree was kept");
                }
                if let Pending::DiskConflict(p) = &action {
                    if let Some(i) = self.editor.index_of(p) {
                        self.editor.buffers[i].keep_mine();
                    }
                    self.say("Kept your version; saving will overwrite the file on disk");
                }
            }
            Outcome::Confirmed(token) => {
                self.overlay = Overlay::None;
                match action {
                    Pending::StopSession(id) => match self.sessions.stop(id, token) {
                        Ok(()) => self.say("Stopping… (force stop is offered after 5 s)"),
                        Err(e) => self.say(e),
                    },
                    Pending::ForceStopSession(id) => match self.sessions.force_stop(id, token) {
                        Ok(()) => self.say("Force stopped"),
                        Err(e) => self.say(e),
                    },
                    Pending::CloseSession { id, offer_remove } => {
                        if offer_remove {
                            let target = self.sessions.get(id).and_then(|s| match &s.workdir {
                                crate::session::Workdir::Worktree(w) => self
                                    .projects
                                    .get(s.project)
                                    .map(|p| (p.path.clone(), w.clone())),
                                crate::session::Workdir::ProjectFolder => None,
                            });
                            if let Some((project, w)) = target {
                                match crate::session::worktree::remove(&project, &w.path) {
                                    Ok(()) => self.say(format!(
                                        "Removed the worktree; the branch {} is kept",
                                        w.branch
                                    )),
                                    Err(e) => self.say(format!("The worktree was kept: {e}")),
                                }
                            }
                        }
                        self.close_session(id);
                    }
                    Pending::Quit => {
                        if self.sessions.running() == 0 {
                            self.quit = true;
                        } else {
                            // Spec Edge Cases: stop every agent gracefully, then quit (tick_stops
                            // forces what is left after 5 s).
                            self.sessions.stop_all_confirmed(token);
                            self.quitting = Some(Instant::now());
                            self.say("Stopping sessions, then quitting…");
                        }
                    }
                    Pending::CloseBuffer(i) => self.editor.close(i),
                    Pending::RemoveProject(id) => self.remove_project(id),
                    Pending::Delete(path) => match fileops::delete(&path, token) {
                        Ok(()) => {
                            for b in &mut self.editor.buffers {
                                if b.path.as_deref().is_some_and(|p| p.starts_with(&path)) {
                                    b.deleted();
                                }
                            }
                            if let Some(dir) = path.parent() {
                                self.after_fileop(dir, None);
                            }
                            self.say(format!("Deleted {}", path.display()));
                        }
                        Err(e) => self.say(format!("Delete failed: {e}")),
                    },
                    Pending::DiskConflict(p) => {
                        if let Some(i) = self.editor.index_of(&p) {
                            match self.editor.buffers[i].load_disk() {
                                Ok(()) => self.say(
                                    "Loaded the file from disk; Ctrl-Z brings your version back",
                                ),
                                Err(e) => self.say(format!("Could not load the file: {e}")),
                            }
                        }
                    }
                }
            }
        }
        self.watch_dirty = true;
        self.show_next_conflict();
        true
    }

    fn confirm(&mut self, dialog: Dialog, action: Pending) {
        self.overlay = Overlay::Confirm { dialog, action };
    }

    /// Ctrl-Q (FR-007): warns and lists the dirty buffers.
    pub(super) fn request_quit(&mut self) -> bool {
        if self.quitting.is_some() {
            // Already stopping sessions on the way out.
            return false;
        }
        let dirty = self.editor.dirty_names();
        let running: Vec<String> = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state.running())
            .map(|s| {
                let procs = self.session_metrics(s.id).map_or(1, |m| m.2);
                format!(
                    "  {}  ({procs} process{})",
                    s.label(),
                    if procs == 1 { "" } else { "es" }
                )
            })
            .collect();
        if dirty.is_empty() && running.is_empty() {
            self.quit = true;
            return false;
        }
        let mut body = Vec::new();
        if !running.is_empty() {
            body.push("These sessions are running and will be stopped:".to_string());
            body.extend(running);
        }
        if !dirty.is_empty() {
            body.push("These files have unsaved changes:".to_string());
            body.extend(dirty.iter().map(|d| format!("  {d}")));
            body.push("Quitting discards them.".into());
        }
        let title = if dirty.is_empty() {
            "Quit and stop sessions?"
        } else {
            "Quit without saving?"
        };
        let mut d = Dialog::new(title, "quit", body, "Quit");
        d.cancel_label = "Cancel".into();
        self.confirm(d, Pending::Quit);
        true
    }

    fn close_buffer(&mut self) -> bool {
        let i = self.editor.active;
        let Some(b) = self.editor.active() else {
            self.say("No file open");
            return true;
        };
        if b.dirty() {
            let name = b.name();
            let d = Dialog::new(
                "Close without saving?",
                &name,
                vec![
                    format!("{name} has unsaved changes."),
                    "Closing discards them.".into(),
                ],
                "Close",
            );
            self.confirm(d, Pending::CloseBuffer(i));
        } else {
            self.editor.close(i);
            self.watch_dirty = true;
        }
        true
    }

    // ----- Files pane -----

    fn files_move(&mut self, by: isize) -> bool {
        let Some(p) = &mut self.project else {
            return false;
        };
        let n = p.tree.rows().len();
        if n == 0 {
            return false;
        }
        let new = (p.selected as isize)
            .saturating_add(by)
            .clamp(0, n as isize - 1) as usize;
        let changed = new != p.selected;
        p.selected = new;
        changed
    }

    fn no_project(&mut self) -> bool {
        self.say("No project open: Ctrl-O opens one");
        true
    }

    fn files_open(&mut self) -> bool {
        let Some(row) = self.project.as_ref().and_then(Project::selected_row) else {
            return self.no_project();
        };
        if row.is_dir {
            if let Some(p) = &mut self.project {
                p.tree.set_expanded(&row.path, !row.expanded);
            }
            self.watch_dirty = true;
            return true;
        }
        self.open_file(&row.path)
    }

    /// Opens a file in the editor and focuses it.
    pub fn open_file(&mut self, path: &Path) -> bool {
        match self.editor.open(path, self.prefs.read_only_bytes) {
            Ok(i) => {
                self.workspace.view = super::workspace::View::Editor;
                if self.editor.buffers[i].read_only {
                    self.say(format!(
                        "{} is over {} MB: opened read-only",
                        self.editor.buffers[i].name(),
                        self.prefs.read_only_bytes / (1024 * 1024)
                    ));
                }
                self.focus = Pane::Editor;
                self.editor.released = false;
                self.watch_dirty = true;
            }
            Err(e) => self.say(format!("{}: {e}", path.display())),
        }
        true
    }

    fn files_collapse(&mut self) -> bool {
        let Some(p) = &mut self.project else {
            return false;
        };
        let rows = p.tree.rows();
        let Some(row) = rows.get(p.selected) else {
            return false;
        };
        if row.is_dir && row.expanded && row.depth > 0 {
            p.tree.set_expanded(&row.path, false);
            self.watch_dirty = true;
            return true;
        }
        if let Some(parent) = row.path.parent() {
            if let Some(i) = rows.iter().position(|r| r.path == parent) {
                p.selected = i;
                return true;
            }
        }
        false
    }

    fn files_expand(&mut self) -> bool {
        let Some(p) = &mut self.project else {
            return false;
        };
        let Some(row) = p.tree.rows().into_iter().nth(p.selected) else {
            return false;
        };
        if !row.is_dir {
            return false;
        }
        if !row.expanded {
            p.tree.set_expanded(&row.path, true);
            self.watch_dirty = true;
        } else if p
            .tree
            .rows()
            .get(p.selected + 1)
            .is_some_and(|r| r.depth > row.depth)
        {
            p.selected += 1;
        }
        true
    }

    fn files_new(&mut self, file: bool) -> bool {
        let Some(row) = self.project.as_ref().and_then(Project::selected_row) else {
            return self.no_project();
        };
        let dir = fileops::target_dir(&row.path, row.is_dir);
        let kind = if file {
            PromptKind::NewFile(dir)
        } else {
            PromptKind::NewFolder(dir)
        };
        self.prompt(kind, "")
    }

    fn files_rename(&mut self) -> bool {
        let Some(row) = self.project.as_ref().and_then(Project::selected_row) else {
            return self.no_project();
        };
        if row.depth == 0 {
            self.say("The project folder itself cannot be renamed here");
            return true;
        }
        let name = row.name.clone();
        self.prompt(PromptKind::Rename(row.path), &name)
    }

    fn files_delete(&mut self) -> bool {
        let Some(row) = self.project.as_ref().and_then(Project::selected_row) else {
            return self.no_project();
        };
        if row.depth == 0 {
            self.say("The project folder itself cannot be deleted here");
            return true;
        }
        let d = fileops::delete_dialog(&row.path);
        self.confirm(d, Pending::Delete(row.path));
        true
    }

    /// Refreshes `dir` in the tree after a file operation and selects `select`.
    fn after_fileop(&mut self, dir: &Path, select: Option<&Path>) {
        if let Some(p) = &mut self.project {
            p.tree.refresh(dir);
            if let Some(s) = select {
                p.tree.reveal(s);
                p.select_path(s);
            }
            let n = p.tree.rows().len();
            p.selected = p.selected.min(n.saturating_sub(1));
        }
        self.watch_dirty = true;
    }

    // ----- buffer commands -----

    fn save(&mut self) -> bool {
        let Some(b) = self.editor.active_mut() else {
            self.say("No file open");
            return true;
        };
        let name = b.name();
        match b.save() {
            Ok(()) => self.say(format!("Saved {name}")),
            Err(e) => self.say(format!("Save failed: {e}")),
        }
        self.watch_dirty = true;
        true
    }

    fn copy(&mut self, cut: bool) -> bool {
        let text = match self.editor.active_mut() {
            Some(b) if cut && !b.read_only => b.cut(),
            Some(b) => b.selected_text(),
            None => None,
        };
        let Some(text) = text else {
            // FR-008: copy with no selection does nothing and points to the quit key.
            self.say("Quit: Ctrl-Q");
            return true;
        };
        if self.clipboard.copy(&text).failed() {
            self.say("Copy failed: clipboard unavailable");
        }
        self.editor.follow = true;
        true
    }

    fn paste_key(&mut self) -> bool {
        match self.clipboard.paste() {
            Some(t) => self.paste(&t),
            None => {
                self.say("Clipboard unavailable: use your terminal's paste (Ctrl-Shift-V, Cmd-V or right-click)");
                true
            }
        }
    }

    /// A bracketed paste (FR-008): inserted literally where the focus is.
    pub fn paste(&mut self, text: &str) -> bool {
        if self.session_paste(text) {
            return true;
        }
        if matches!(self.overlay, Overlay::None)
            && self.workspace.view == super::workspace::View::Agents
            && self.workspace.focus == super::workspace::AgentsFocus::Session
        {
            return false;
        }
        match &mut self.overlay {
            Overlay::Prompt { input, error, .. } => {
                input.insert(text);
                *error = None;
                return true;
            }
            Overlay::Palette { query, selected } => {
                query.push_str(&text.replace(['\n', '\r'], " "));
                *selected = 0;
                return true;
            }
            Overlay::Confirm { .. }
            | Overlay::Help { .. }
            | Overlay::Menu(_)
            | Overlay::Orch(_)
            | Overlay::Grant(_)
            | Overlay::Info { .. } => return false,
            Overlay::None => {}
        }
        if self.focus != Pane::Editor {
            return false;
        }
        if let Some(f) = &mut self.find {
            f.input.insert(text);
            f.no_match = false;
            return true;
        }
        match self.editor.active_mut() {
            Some(b) if b.read_only => {
                self.say("Read-only: the file is over the size threshold");
                true
            }
            Some(b) => {
                b.insert(text);
                self.editor.released = false;
                self.editor.follow = true;
                true
            }
            None => false,
        }
    }

    // ----- find bar (FR-005) -----

    fn open_find(&mut self) -> bool {
        if self.editor.active().is_none() && self.project.is_none() {
            self.say("Find: open a file or a project first");
            return true;
        }
        let prev = self.last_query.clone();
        let from_sel = self
            .editor
            .active()
            .and_then(|b| b.selected_text())
            .filter(|t| !t.contains('\n') && t.chars().count() <= 200);
        let q = from_sel.unwrap_or(prev);
        let scope = if self.editor.active().is_some() {
            Scope::File
        } else {
            Scope::Project
        };
        self.find = Some(FindBar {
            input: LineInput::selected(&q),
            scope,
            session: None,
            results_open: false,
            selected: 0,
            offset: 0,
            no_match: false,
        });
        self.focus = Pane::Editor;
        self.editor.released = false;
        true
    }

    fn find_key(&mut self, key: KeyEvent) -> bool {
        if let Some(id) = keymap::lookup(&key, Context::FindBar, true) {
            if id.starts_with("find.") {
                return self.find_command(id, key.code == KeyCode::Enter);
            }
            return self.run(id);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let Some(f) = &mut self.find else {
            return false;
        };
        match key.code {
            KeyCode::PageDown | KeyCode::PageUp if f.results_open => {
                let page = self.editor_areas().results.height.saturating_sub(1).max(1) as isize;
                let by = if key.code == KeyCode::PageDown {
                    page
                } else {
                    -page
                };
                return self.move_result(by);
            }
            _ => {}
        }
        let Some(f) = &mut self.find else {
            return false;
        };
        let before = f.input.text().to_string();
        let changed = text_key(
            &mut f.input,
            key,
            ctrl,
            alt,
            shift,
            &mut self.clipboard,
            &mut self.message,
        );
        if f.input.text() != before {
            f.no_match = false;
        }
        changed
    }

    /// `enter`: the command came from `Enter` (or the palette), not from an arrow key.
    fn find_command(&mut self, id: &str, enter: bool) -> bool {
        let has_project = self.project.is_some();
        let has_buffer = self.editor.active().is_some();
        let Some(f) = &mut self.find else {
            return false;
        };
        match id {
            "find.scope" => {
                let to = match f.scope {
                    Scope::File => Scope::Project,
                    Scope::Project => Scope::File,
                };
                if to == Scope::Project && !has_project {
                    self.say("No project open: Ctrl-O opens one");
                } else if to == Scope::File && !has_buffer {
                    self.say("No file open");
                } else {
                    f.scope = to;
                    f.no_match = false;
                    f.results_open = to == Scope::Project && f.session.is_some();
                }
                true
            }
            "find.close" => {
                match &mut f.session {
                    Some(s) if s.running() => s.cancel(),
                    _ => {
                        self.last_query = f.input.text().to_string();
                        self.find = None;
                    }
                }
                true
            }
            "find.next" | "find.prev" => {
                let forward = id == "find.next";
                let query = f.input.text().to_string();
                if f.scope == Scope::File {
                    let rows = self.editor_areas().text.height as usize;
                    let Some(f) = &mut self.find else {
                        return false;
                    };
                    if query.is_empty() {
                        return false;
                    }
                    let Some(b) = self.editor.buffers.get_mut(self.editor.active) else {
                        return false;
                    };
                    f.no_match = !b.find(&query, forward);
                    let (line, _) = b.line_col();
                    if line < b.top || line >= b.top + rows.max(1) {
                        b.center_on_cursor(rows);
                    }
                    self.editor.follow = true;
                    self.last_query = query;
                    return true;
                }
                // Project scope: arrows move in the results list; Enter opens the selected
                // result if the list shows this query's results, and runs the search otherwise.
                let current =
                    f.session.as_ref().is_some_and(|s| s.query == query) && f.results_open;
                if !enter {
                    if current {
                        return self.move_result(if forward { 1 } else { -1 });
                    }
                    return false;
                }
                if current && f.session.as_ref().is_some_and(|s| !s.results.is_empty()) {
                    return self.open_result(None);
                }
                if query.is_empty() {
                    return false;
                }
                let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
                    self.say("No project open: Ctrl-O opens one");
                    return true;
                };
                let Some(f) = &mut self.find else {
                    return false;
                };
                f.session = Some(SearchSession::start(&root, &query));
                f.results_open = true;
                f.selected = 0;
                f.offset = 0;
                self.searches += 1;
                self.last_query = query;
                true
            }
            _ => false,
        }
    }

    fn move_result(&mut self, by: isize) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        let n = f.session.as_ref().map_or(0, |s| s.results.len());
        if n == 0 {
            return false;
        }
        f.selected = (f.selected as isize + by).clamp(0, n as isize - 1) as usize;
        true
    }

    /// Opens a project result (the selected one, or `index`) at its line.
    fn open_result(&mut self, index: Option<usize>) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        let i = index.unwrap_or(f.selected);
        let Some(hit) = f.session.as_ref().and_then(|s| s.results.get(i)).cloned() else {
            return false;
        };
        f.selected = i;
        self.last_query = f.input.text().to_string();
        let rows = self.editor_areas().text.height as usize;
        self.find = None;
        if !self.open_file(&hit.path) {
            return true;
        }
        if let Some(b) = self.editor.active_mut() {
            if b.path.as_deref() == Some(&*hit.path) {
                let line = (hit.line as usize).saturating_sub(1);
                let col = if line < b.len_lines() {
                    let text = b.text().line(line).to_string();
                    text.get(..hit.col.min(text.len()))
                        .map_or(0, |s| s.chars().count())
                } else {
                    0
                };
                b.set_cursor(line, col, false);
                b.center_on_cursor(rows);
                self.editor.follow = true;
            }
        }
        true
    }

    // ----- mouse (FR-063) -----

    fn double_click(&mut self, x: u16, y: u16) -> bool {
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(lx, ly, at)| {
            lx == x && ly == y && now.duration_since(at) < DOUBLE_CLICK
        });
        self.last_click = if double { None } else { Some((x, y, now)) };
        double
    }

    /// Mouse events over the Files pane and the editor. `None` if not handled here.
    pub(super) fn editing_mouse(&mut self, m: MouseEvent, pane: Option<Pane>) -> Option<bool> {
        let g = layout::geometry(self.area, self.sizes, self.focus);
        let (x, y) = (m.column, m.row);
        if let Overlay::Confirm { dialog, .. } = &mut self.overlay {
            if let MouseEventKind::Down(MouseButton::Left) = m.kind {
                let (_, cancel, ok) = confirm::geometry(self.area, dialog);
                let inside = |r: ratatui::layout::Rect| {
                    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
                };
                let outcome = if inside(cancel) {
                    dialog.click(confirm::Button::Cancel)
                } else if inside(ok) {
                    dialog.click(confirm::Button::Confirm)
                } else {
                    return Some(false);
                };
                return Some(self.finish_dialog(outcome));
            }
            return Some(false);
        }
        if !matches!(self.overlay, Overlay::None) {
            return None;
        }
        match (m.kind, pane) {
            (MouseEventKind::Down(MouseButton::Left), Some(Pane::Files)) => {
                let double = self.double_click(x, y);
                self.focus = Pane::Files;
                let Some(p) = &mut self.project else {
                    return Some(true);
                };
                if let Some(i) = files_ui::row_at(g.files, p.offset, y) {
                    if i < p.tree.rows().len() {
                        p.selected = i;
                        if double {
                            self.files_open();
                        }
                    }
                }
                Some(true)
            }
            (MouseEventKind::Down(MouseButton::Left), Some(Pane::Editor)) => {
                let double = self.double_click(x, y);
                self.focus = Pane::Editor;
                self.editor.released = false;
                let a = editor_ui::areas(g.editor, self);
                if let Some(f) = &self.find {
                    if f.results_open && a.results.height > 0 {
                        if let Some(i) = find_ui::row_at(a.results, f.offset, y) {
                            let n = f.session.as_ref().map_or(0, |s| s.results.len());
                            if i < n {
                                if let Some(f) = &mut self.find {
                                    f.selected = i;
                                }
                                if double {
                                    return Some(self.open_result(Some(i)));
                                }
                            }
                        }
                        return Some(true);
                    }
                }
                if contains(a.text, x, y) {
                    let tab = self.prefs.tab_width;
                    let shift = m.modifiers.contains(KeyModifiers::SHIFT);
                    if let Some(b) = self.editor.active_mut() {
                        let line = b.top + (y - a.text.y) as usize;
                        let dcol = b.left + (x - a.text.x) as usize;
                        b.set_cursor_display(line, dcol, tab, shift);
                        self.drag_select = true;
                    }
                }
                Some(true)
            }
            (MouseEventKind::Drag(MouseButton::Left), _) if self.drag_select => {
                let a = editor_ui::areas(g.editor, self);
                let tab = self.prefs.tab_width;
                let b = self.editor.active_mut()?;
                // Dragging past the top or bottom edge scrolls.
                let rel = y as isize - a.text.y as isize;
                if rel < 0 {
                    b.top = b.top.saturating_sub(1);
                } else if rel >= a.text.height as isize {
                    b.top = (b.top + 1).min(b.len_lines().saturating_sub(1));
                }
                let row = rel.clamp(0, a.text.height.saturating_sub(1) as isize) as usize;
                let dcol = b.left + x.saturating_sub(a.text.x) as usize;
                b.set_cursor_display(b.top + row, dcol, tab, true);
                Some(true)
            }
            (MouseEventKind::Up(_), _) if self.drag_select => {
                self.drag_select = false;
                Some(false)
            }
            (MouseEventKind::ScrollDown | MouseEventKind::ScrollUp, Some(p)) => {
                let down = m.kind == MouseEventKind::ScrollDown;
                match p {
                    Pane::Files => Some(self.files_move(if down { 3 } else { -3 })),
                    Pane::Editor => {
                        if let Some(f) = &self.find {
                            if f.results_open {
                                return Some(self.move_result(if down { 3 } else { -3 }));
                            }
                        }
                        let b = self.editor.active_mut()?;
                        let last = b.len_lines().saturating_sub(1);
                        b.top = if down {
                            (b.top + 3).min(last)
                        } else {
                            b.top.saturating_sub(3)
                        };
                        self.editor.follow = false;
                        Some(true)
                    }
                    Pane::Processes => None,
                }
            }
            _ => None,
        }
    }

    // ----- background work -----

    /// True while something needs a quick wake-up (a running search).
    pub fn busy(&self) -> bool {
        self.find
            .as_ref()
            .and_then(|f| f.session.as_ref())
            .is_some_and(SearchSession::running)
    }

    /// Drains search results and change notifications. Returns true if the screen changed.
    pub fn poll_background(&mut self) -> bool {
        let mut changed = false;
        if let Some(s) = self.find.as_mut().and_then(|f| f.session.as_mut()) {
            changed |= s.poll();
            if s.state == SearchState::Failed {
                let e = s.error.clone().unwrap_or_default();
                self.say(format!("Search failed: {e}"));
            }
        }
        if self.watch_dirty {
            self.watch_dirty = false;
            let files: Vec<PathBuf> = self
                .editor
                .buffers
                .iter()
                .filter_map(|b| b.path.clone())
                .collect();
            let dirs = self
                .project
                .as_ref()
                .map(|p| p.tree.expanded_dirs())
                .unwrap_or_default();
            let set = watch::watch_set(files.iter().map(PathBuf::as_path), dirs);
            if !self.watcher.sync(set) {
                if let Some(e) = self.watcher.error.take() {
                    self.say(format!("Change detection unavailable: {e}"));
                    changed = true;
                }
            }
        }
        let ch = self.watcher.poll();
        if !ch.paths.is_empty() {
            changed |= self.apply_changes(&ch.paths);
        }
        changed
    }

    /// Applies change notifications: FR-006 transitions for open files, tree refreshes for
    /// expanded folders.
    pub fn apply_changes<'a>(&mut self, paths: impl IntoIterator<Item = &'a PathBuf>) -> bool {
        let mut changed = false;
        let mut dirs = std::collections::BTreeSet::new();
        for p in paths {
            if let Some(i) = self.editor.index_of(p) {
                let b = &mut self.editor.buffers[i];
                let name = b.name();
                match b.check_disk() {
                    DiskEvent::Unchanged => {}
                    DiskEvent::Reloaded => {
                        self.say(format!("{name} changed on disk and was reloaded"));
                        changed = true;
                    }
                    DiskEvent::Deleted => {
                        self.say(format!(
                            "{name} was deleted or renamed on disk; your buffer is kept, unsaved"
                        ));
                        changed = true;
                    }
                    DiskEvent::Conflict => {
                        self.conflicts.push(p.clone());
                        changed = true;
                    }
                }
            }
            if let Some(parent) = p.parent() {
                dirs.insert(parent.to_path_buf());
            }
            dirs.insert(p.clone());
        }
        if let Some(pr) = &mut self.project {
            let expanded = pr.tree.expanded_dirs();
            for d in dirs {
                if expanded.contains(&d) {
                    pr.tree.refresh(&d);
                    changed = true;
                }
            }
            let n = pr.tree.rows().len();
            pr.selected = pr.selected.min(n.saturating_sub(1));
        }
        changed |= self.show_next_conflict();
        changed
    }

    /// Opens the "keep mine / load disk" dialog for the next queued conflict.
    fn show_next_conflict(&mut self) -> bool {
        if !matches!(self.overlay, Overlay::None) {
            return false;
        }
        while let Some(p) = self.conflicts.first().cloned() {
            self.conflicts.remove(0);
            let Some(i) = self.editor.index_of(&p) else {
                continue;
            };
            if self.editor.buffers[i].disk_state != crate::editor::buffer::DiskState::ChangedOnDisk
            {
                continue;
            }
            let mut d = Dialog::new(
                "File changed on disk",
                &p.display().to_string(),
                vec![
                    p.display().to_string(),
                    "The file changed on disk while you have unsaved edits.".into(),
                    "Keep mine: keep your edits (saving overwrites the file).".into(),
                    "Load disk: replace your edits with the file (Ctrl-Z brings them back).".into(),
                ],
                "Load disk",
            );
            d.cancel_label = "Keep mine".into();
            self.confirm(d, Pending::DiskConflict(p));
            return true;
        }
        false
    }

    /// Before drawing: scrolls the active buffer to its cursor and refreshes highlighting for
    /// the visible lines.
    pub fn prepare_frame(&mut self) {
        self.prepare_workspace();
        let a = self.editor_areas();
        let tab = self.prefs.tab_width;
        let follow = std::mem::take(&mut self.editor.follow);
        if let Some(f) = &mut self.find {
            let rows = a.results.height.saturating_sub(1).max(1) as usize;
            if f.selected < f.offset {
                f.offset = f.selected;
            } else if f.selected >= f.offset + rows {
                f.offset = f.selected + 1 - rows;
            }
        }
        if let Some(p) = &mut self.project {
            let rows =
                files_ui::visible_rows(layout::geometry(self.area, self.sizes, self.focus).files);
            if p.selected < p.offset {
                p.offset = p.selected;
            } else if rows > 0 && p.selected >= p.offset + rows {
                p.offset = p.selected + 1 - rows;
            }
        }
        let Some(b) = self.editor.active_mut() else {
            return;
        };
        if follow && a.text.height > 0 {
            b.scroll_to_cursor(a.text.height as usize, a.text.width as usize, tab);
        }
        let visible = b.top..b.top + a.text.height as usize;
        let rev = b.rev;
        let text = b.text().clone();
        if let Some(s) = &mut b.syntax {
            s.update(&text, rev, visible);
        }
    }
}

fn contains(r: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// `~` and `~/…` expand to the home directory.
pub(super) fn expand_home(p: &str) -> PathBuf {
    let home = || std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if p == "~" {
        if let Some(h) = home() {
            return PathBuf::from(h);
        }
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(h) = home() {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(p)
}

/// Editing keys for a one-line input, including selection and the clipboard (FR-008: these
/// work in the find box and name prompts too).
fn text_key(
    input: &mut LineInput,
    key: KeyEvent,
    ctrl: bool,
    alt: bool,
    shift: bool,
    clipboard: &mut crate::clipboard::Clipboard,
    message: &mut Option<(String, Instant)>,
) -> bool {
    let mut say = |t: &str| *message = Some((t.to_string(), Instant::now()));
    match key.code {
        KeyCode::Left => input.left(shift),
        KeyCode::Right => input.right(shift),
        KeyCode::Home => input.home(shift),
        KeyCode::End => input.end(shift),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Char('a' | 'A') if ctrl && !alt => input.select_all(),
        KeyCode::Char('c' | 'C' | 'x' | 'X') if ctrl && !alt => {
            let cut = matches!(key.code, KeyCode::Char('x' | 'X'));
            let text = if cut {
                input.cut()
            } else {
                input.selected_text()
            };
            match text {
                Some(t) => {
                    if clipboard.copy(&t).failed() {
                        say("Copy failed: clipboard unavailable");
                    }
                }
                None => say("Quit: Ctrl-Q"),
            }
        }
        KeyCode::Char('v' | 'V') if ctrl && !alt => match clipboard.paste() {
            Some(t) => input.insert(&t),
            None => say("Clipboard unavailable: use your terminal's paste"),
        },
        _ => match is_text_char(&key) {
            Some(c) => input.insert(&c.to_string()),
            None => return false,
        },
    }
    true
}
