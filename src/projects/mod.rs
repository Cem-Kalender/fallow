//! The project list (002 data-model.md "ProjectList", FR-014, FR-018). Persistence is in
//! `store.rs` (US4).

use std::path::{Path, PathBuf};

use crate::editor::Project as EditorProject;

pub type ProjectId = u32;

/// At most this many projects (data-model.md "Limits").
pub const MAX_PROJECTS: usize = 200;

#[derive(Debug)]
pub struct ProjectEntry {
    pub id: ProjectId,
    pub path: PathBuf,
    pub name: String,
    pub present: bool,
    /// The editor view's lazy tree, created on first use.
    pub editor: Option<EditorProject>,
}

impl ProjectEntry {
    /// The editor project, creating its tree on first use.
    pub fn editor(&mut self) -> std::io::Result<&mut EditorProject> {
        if self.editor.is_none() {
            self.editor = Some(EditorProject::open(&self.path)?);
        }
        Ok(self.editor.as_mut().expect("just created"))
    }
}

#[derive(Debug, Default)]
pub struct ProjectList {
    pub projects: Vec<ProjectEntry>,
    pub selected: Option<usize>,
    next_id: ProjectId,
}

/// Paths compare case-insensitively where the file system usually is (Windows, macOS).
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn folder_name(p: &Path) -> String {
    p.file_name()
        .map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
        .chars()
        .take(64)
        .collect()
}

fn check_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if (1..=64).contains(&n.chars().count()) {
        Ok(n.to_string())
    } else {
        Err("A project name must be 1..=64 chars".into())
    }
}

impl ProjectList {
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn selected(&self) -> Option<&ProjectEntry> {
        self.selected.and_then(|i| self.projects.get(i))
    }

    pub fn selected_mut(&mut self) -> Option<&mut ProjectEntry> {
        self.selected.and_then(|i| self.projects.get_mut(i))
    }

    pub fn get(&self, id: ProjectId) -> Option<&ProjectEntry> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.projects.iter().position(|p| same_path(&p.path, path))
    }

    /// Adds a readable folder and selects it. An existing entry is selected instead.
    pub fn add(&mut self, path: &Path) -> Result<usize, String> {
        let abs = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !abs.is_dir() {
            return Err(format!("{}: not a folder", abs.display()));
        }
        std::fs::read_dir(&abs).map_err(|e| format!("{}: {e}", abs.display()))?;
        if let Some(i) = self.index_of(&abs) {
            self.selected = Some(i);
            return Ok(i);
        }
        if self.projects.len() >= MAX_PROJECTS {
            return Err(format!("The list holds at most {MAX_PROJECTS} projects"));
        }
        Ok(self.push(abs, None, true))
    }

    /// Adds an entry without checking the folder (loading `projects.toml`; missing folders are
    /// kept and shown as missing, FR-018).
    pub fn push(&mut self, path: PathBuf, name: Option<String>, select: bool) -> usize {
        self.next_id += 1;
        let present = path.is_dir();
        let name = name
            .and_then(|n| check_name(&n).ok())
            .unwrap_or_else(|| folder_name(&path));
        self.projects.push(ProjectEntry {
            id: self.next_id,
            path,
            name,
            present,
            editor: None,
        });
        let i = self.projects.len() - 1;
        if select || self.selected.is_none() {
            self.selected = Some(i);
        }
        i
    }

    /// Removes an entry from the list. The folder is never touched.
    pub fn remove(&mut self, i: usize) -> Option<ProjectEntry> {
        if i >= self.projects.len() {
            return None;
        }
        let p = self.projects.remove(i);
        self.selected = match self.selected {
            _ if self.projects.is_empty() => None,
            Some(s) if s > i => Some(s - 1),
            Some(s) => Some(s.min(self.projects.len() - 1)),
            None => None,
        };
        Some(p)
    }

    pub fn rename(&mut self, i: usize, name: &str) -> Result<(), String> {
        let n = check_name(name)?;
        let p = self.projects.get_mut(i).ok_or("no such project")?;
        p.name = n;
        Ok(())
    }

    /// Moves entry `i` up (`by` < 0) or down; the selection follows it.
    pub fn move_by(&mut self, i: usize, by: isize) -> bool {
        let n = self.projects.len() as isize;
        let j = i as isize + by;
        if i as isize >= n || j < 0 || j >= n {
            return false;
        }
        self.projects.swap(i, j as usize);
        if self.selected == Some(i) {
            self.selected = Some(j as usize);
        }
        true
    }

    /// Re-checks whether each folder exists (after the first frame, and on select).
    pub fn refresh_presence(&mut self) {
        for p in &mut self.projects {
            p.present = p.path.is_dir();
        }
    }
}
pub mod store;
