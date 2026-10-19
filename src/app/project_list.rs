//! The project list's commands and its persistence (002 US4: FR-014 to FR-018, contracts/
//! ui-commands.md "Projects list", contracts/config.md "projects.toml").

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::editing::{Pending, PromptKind};
use super::state::{AppState, Overlay};
use super::workspace::{AgentsFocus, View};
use crate::editor::selection::LineInput;
use crate::projects::ProjectId;
use crate::projects::store;

/// A selection change is written this long after the last one, so moving through the list
/// doesn't rewrite the file on every key.
const SELECTION_SAVE_DELAY: Duration = Duration::from_secs(1);

/// Where and whether the project list is saved.
#[derive(Debug, Default)]
pub struct ProjectStore {
    /// Fallow's config directory (`None`: nothing is saved).
    pub dir: Option<PathBuf>,
    /// False until the file has been read, and while it can't be parsed (it would be
    /// overwritten).
    pub writable: bool,
    /// The selection as last written.
    saved_selected: Option<usize>,
    /// When the selection last changed without being written.
    selection_changed: Option<Instant>,
}

impl AppState {
    /// Reads `projects.toml`, after the first frame (FR-028). The folder given on the command
    /// line is added if new and selected (FR-017).
    pub fn load_projects(&mut self) {
        let Some(dir) = self.store.dir.clone() else {
            return;
        };
        let loaded = store::read(&dir);
        self.store.writable = loaded.writable;
        if let Some(first) = loaded.notices.first() {
            self.say(first.to_string());
        }
        self.notices
            .extend(loaded.notices.iter().map(ToString::to_string));
        let changed = self.projects.restore(loaded.entries, loaded.selected);
        self.store.saved_selected = self.projects.selected;
        if changed {
            self.save_projects();
        }
        self.workspace.list_selected = 0;
    }

    /// Writes the list now. A failure is reported; the in-memory list stays as it is.
    pub fn save_projects(&mut self) {
        self.store.selection_changed = None;
        if !self.store.writable {
            return;
        }
        let Some(dir) = self.store.dir.clone() else {
            return;
        };
        match store::write(&dir, &self.projects) {
            Ok(()) => self.store.saved_selected = self.projects.selected,
            Err(e) => {
                let msg = format!(
                    "{}: cannot save the project list: {e}",
                    dir.join(store::PROJECTS_FILE).display()
                );
                if !self.notices.contains(&msg) {
                    self.notices.push(msg.clone());
                }
                self.say(msg);
            }
        }
    }

    /// Writes a changed selection once it has settled, or right away with `now` (on quit).
    pub fn save_selection(&mut self, now: bool) {
        if !self.store.writable || self.projects.selected == self.store.saved_selected {
            self.store.selection_changed = None;
            return;
        }
        let since = *self
            .store
            .selection_changed
            .get_or_insert_with(Instant::now);
        if now || since.elapsed() >= SELECTION_SAVE_DELAY {
            self.save_projects();
        }
    }

    fn selected_project_id(&self) -> Option<ProjectId> {
        self.projects.selected().map(|p| p.id)
    }

    fn project_index(&self, id: ProjectId) -> Option<usize> {
        self.projects.projects.iter().position(|p| p.id == id)
    }

    /// Selects row `i` of the project list.
    pub(super) fn select_project(&mut self, i: usize) -> bool {
        if i >= self.projects.len() || self.projects.selected == Some(i) {
            return false;
        }
        self.projects.selected = Some(i);
        if let Some(p) = self.projects.selected_mut() {
            p.present = p.path.is_dir();
        }
        self.workspace.list_selected = 0;
        self.show_listed_session();
        true
    }

    /// Project-list commands. `None` if `id` is not one.
    pub(super) fn run_project_list(&mut self, id: &str) -> Option<bool> {
        Some(match id {
            "projects.home" => self.select_project(0),
            "projects.end" => self.select_project(self.projects.len().saturating_sub(1)),
            "project.remove" => self.ask_remove_project(),
            "project.rename" => {
                let Some(p) = self.projects.selected() else {
                    return Some(false);
                };
                let (id, name) = (p.id, p.name.clone());
                self.ask(PromptKind::RenameProject(id), &name)
            }
            "project.relocate" => {
                let Some(p) = self.projects.selected() else {
                    return Some(false);
                };
                let (id, path) = (p.id, p.path.display().to_string());
                self.ask(PromptKind::RelocateProject(id), &path)
            }
            "project.move_up" | "project.move_down" => {
                let by = if id == "project.move_up" { -1 } else { 1 };
                let Some(i) = self.projects.selected else {
                    return Some(false);
                };
                if !self.projects.move_by(i, by) {
                    return Some(false);
                }
                self.save_projects();
                true
            }
            "project.edit" => {
                if self.projects.selected.is_none() {
                    return Some(false);
                }
                self.workspace.focus = AgentsFocus::Projects;
                if self.workspace.view == View::Agents {
                    self.toggle_view();
                }
                true
            }
            "project.filter" => {
                let current = self.selected_project_id();
                self.workspace.process_filter = match (self.workspace.process_filter, current) {
                    (Some(f), Some(c)) if f == c => None,
                    (_, c) => c,
                };
                self.panel.selected = 0;
                match self.process_filter() {
                    Some(f) => self.say(format!("LLM processes: only {} (f again: all)", f.name)),
                    None => self.say("LLM processes: all"),
                }
                true
            }
            _ => return None,
        })
    }

    fn ask(&mut self, kind: PromptKind, initial: &str) -> bool {
        self.overlay = Overlay::Prompt {
            kind,
            input: LineInput::selected(initial),
            error: None,
        };
        true
    }

    /// `Delete`/`d`: removes the selected project from the list, after a confirmation when it
    /// has running sessions (US4 scenario 2). The folder is never touched.
    fn ask_remove_project(&mut self) -> bool {
        let Some(p) = self.projects.selected() else {
            return false;
        };
        let (id, name) = (p.id, p.name.clone());
        let running = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.project == id && s.state.running())
            .count();
        if running == 0 {
            self.remove_project(id);
            return true;
        }
        let d = crate::ui::confirm::Dialog::new(
            "Remove project from the list",
            &format!("remove project {name}"),
            vec![
                format!(
                    "{name} has {running} running session{}.",
                    if running == 1 { "" } else { "s" }
                ),
                "They keep running and stay in the session overview (Ctrl-T).".into(),
                "The folder on disk is not changed.".into(),
            ],
            "Remove",
        );
        self.overlay = Overlay::Confirm {
            dialog: d,
            action: Pending::RemoveProject(id),
        };
        true
    }

    pub(super) fn remove_project(&mut self, id: ProjectId) {
        let Some(i) = self.project_index(id) else {
            return;
        };
        let Some(p) = self.projects.remove(i) else {
            return;
        };
        if self.workspace.process_filter == Some(id) {
            self.workspace.process_filter = None;
        }
        self.workspace.list_selected = 0;
        self.show_listed_session();
        self.save_projects();
        self.say(format!(
            "Removed {} from the list; the folder is unchanged",
            p.name
        ));
    }

    pub(super) fn rename_project(&mut self, id: ProjectId, name: &str) -> Result<(), String> {
        let i = self
            .project_index(id)
            .ok_or("The project is no longer listed")?;
        self.projects.rename(i, name)?;
        let name = self.projects.projects[i].name.clone();
        for s in self
            .sessions
            .sessions
            .iter_mut()
            .filter(|s| s.project == id)
        {
            s.project_name = name.clone();
        }
        self.save_projects();
        Ok(())
    }

    pub(super) fn relocate_project(&mut self, id: ProjectId, path: &str) -> Result<(), String> {
        let i = self
            .project_index(id)
            .ok_or("The project is no longer listed")?;
        let path = super::editing::expand_home(path.trim());
        self.projects.relocate(i, &path)?;
        self.save_projects();
        self.say("The project now points to the new folder");
        Ok(())
    }

    /// The LLM panel's project filter, if set (FR-016).
    pub fn process_filter(&self) -> Option<crate::ui::processes::ProjectFilter> {
        let id = self.workspace.process_filter?;
        let p = self.projects.get(id)?;
        let mut dirs = vec![p.path.clone()];
        let mut sessions = Vec::new();
        for s in self.sessions.sessions.iter().filter(|s| s.project == id) {
            sessions.push(s.id);
            if let crate::session::Workdir::Worktree(w) = &s.workdir {
                dirs.push(w.path.clone());
            }
        }
        Some(crate::ui::processes::ProjectFilter {
            name: p.name.clone(),
            dirs,
            sessions,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::UNIX_EPOCH;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::procmon::snapshot::{
        CwdState, Footprint, FootprintMetric, ProcessGroup, ProcessKey, ProcessRecord, Snapshot,
        SystemSummary,
    };
    use crate::session::profiles::builtins;

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    fn plain(c: char) -> KeyEvent {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// A state listing folders `a`, `b` and `c` (selected: `a`), with focus on the list.
    fn three(tag: &str) -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!("fallow-pl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = AppState::new(None, Vec::new());
        for n in ["a", "b", "c"] {
            std::fs::create_dir_all(root.join(n)).unwrap();
            s.projects.add(&root.join(n)).unwrap();
        }
        s.projects.selected = Some(0);
        s.workspace.focus = AgentsFocus::Projects;
        (s, root)
    }

    fn names(s: &AppState) -> Vec<&str> {
        s.projects
            .projects
            .iter()
            .map(|p| p.name.as_str())
            .collect()
    }

    #[test]
    fn reorder_rename_and_home_end() {
        let (mut s, root) = three("reorder");
        s.handle_key(key(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(names(&s), vec!["b", "a", "c"]);
        assert_eq!(s.projects.selected, Some(1), "the selection moves with it");
        s.handle_key(key(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(names(&s), vec!["a", "b", "c"]);
        s.handle_key(key(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(s.projects.selected, Some(2));
        s.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(s.projects.selected, Some(0));

        s.handle_key(key(KeyCode::F(2), KeyModifiers::NONE));
        assert!(matches!(
            s.overlay,
            Overlay::Prompt {
                kind: PromptKind::RenameProject(_),
                ..
            }
        ));
        for c in "API".chars() {
            s.handle_key(plain(c));
        }
        s.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(s.overlay, Overlay::None);
        assert_eq!(names(&s), vec!["API", "b", "c"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_asks_only_with_running_sessions_and_keeps_the_folder() {
        let (mut s, root) = three("remove");
        s.handle_key(plain('d'));
        assert_eq!(s.overlay, Overlay::None, "no sessions: removed at once");
        assert_eq!(names(&s), vec!["b", "c"]);
        assert!(root.join("a").is_dir(), "the folder is untouched");

        let b = s.projects.projects[0].id;
        let sess = crate::session::Session::detached(1, b, builtins().remove(0));
        s.sessions.sessions.push(sess);
        s.handle_key(key(KeyCode::Delete, KeyModifiers::NONE));
        assert!(
            matches!(&s.overlay, Overlay::Confirm { action: Pending::RemoveProject(id), .. } if *id == b)
        );
        s.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(names(&s), vec!["b", "c"], "cancel keeps it");
        s.remove_project(b);
        assert_eq!(names(&s), vec!["c"]);
        assert_eq!(s.sessions.sessions.len(), 1, "its session keeps running");
        assert!(root.join("b").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn e_opens_the_selected_project_in_the_editor_view() {
        let (mut s, root) = three("edit");
        s.projects.selected = Some(1);
        s.handle_key(plain('e'));
        assert_eq!(s.workspace.view, View::Editor);
        let open = s.project.as_ref().map(|p| p.root.clone()).unwrap();
        assert!(open.ends_with("b"), "{}", open.display());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_folders_are_kept_and_can_be_pointed_elsewhere() {
        let (mut s, root) = three("missing");
        std::fs::rename(root.join("a"), root.join("a2")).unwrap();
        s.projects.refresh_presence();
        assert!(!s.projects.projects[0].present);
        let id = s.projects.projects[0].id;
        s.relocate_project(id, &root.join("a2").display().to_string())
            .unwrap();
        let p = &s.projects.projects[0];
        assert!(p.present);
        assert_eq!(
            p.name, "a2",
            "an unrenamed project takes the new folder's name"
        );
        assert!(
            s.relocate_project(id, &root.join("b").display().to_string())
                .is_err(),
            "already listed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn k(pid: u32) -> ProcessKey {
        ProcessKey { pid, start_time: 1 }
    }

    fn snapshot(cwds: &[(u32, Option<&Path>)], sessions: &[(u32, u32)]) -> Snapshot {
        let mut records = HashMap::new();
        let mut groups = Vec::new();
        for (pid, cwd) in cwds {
            records.insert(
                k(*pid),
                ProcessRecord {
                    key: k(*pid),
                    parent: None,
                    name: format!("p{pid}").into(),
                    cmdline: None,
                    owner: None,
                    cpu_samples: [0.0; 5],
                    footprint: None,
                    cwd: cwd.map_or(CwdState::Unknown, |p| CwdState::Known(p.to_path_buf())),
                },
            );
            groups.push(ProcessGroup {
                root: k(*pid),
                members: Vec::new(),
                nested: Vec::new(),
                total_footprint: 0,
                total_cpu: 0.0,
            });
        }
        Snapshot {
            taken_at: UNIX_EPOCH,
            groups,
            llm: HashMap::new(),
            records,
            summary: SystemSummary {
                total_ram: 1,
                available_ram: 1,
                llm_ram: 0,
                local_model_ram: 0,
                fallow_ram: Footprint {
                    bytes: 1,
                    metric: FootprintMetric::Pss,
                },
                low_memory: false,
            },
            notes: Vec::new(),
            sessions: sessions.iter().map(|(p, id)| (k(*p), *id)).collect(),
        }
    }

    #[test]
    fn f_filters_the_llm_panel_to_the_project() {
        let (mut s, root) = three("filter");
        let a = s.projects.projects[0].id;
        let sess = crate::session::Session::detached(7, a, builtins().remove(0));
        s.sessions.sessions.push(sess);
        let inside = root.join("a").join("src");
        let elsewhere = root.join("b");
        s.set_snapshot(Arc::new(snapshot(
            &[
                (1, Some(&inside)),
                (2, Some(&elsewhere)),
                (3, None),
                (4, None),
            ],
            &[(4, 7)],
        )));
        assert_eq!(s.rows().len(), 4);
        s.handle_key(plain('f'));
        let pids: Vec<u32> = s.rows().iter().map(|r| r.key.pid).collect();
        assert_eq!(pids.len(), 2, "{pids:?}");
        assert!(pids.contains(&1), "cwd inside the project");
        assert!(pids.contains(&4), "the project's session agent");
        let screen = crate::ui::layout::tests::render(160, 40, &{
            s.area = ratatui::layout::Rect::new(0, 0, 160, 40);
            std::mem::replace(&mut s, AppState::new(None, Vec::new()))
        });
        assert!(screen.contains("only a (f: all)"), "{screen}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
