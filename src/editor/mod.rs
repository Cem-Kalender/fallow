//! The editor (US3): buffers, the lazy file tree, syntax highlighting, project search, file
//! operations and change notifications.

pub mod buffer;
pub mod fileops;
pub mod search;
pub mod selection;
pub mod syntax;
pub mod tree;
pub mod watch;

use std::path::{Path, PathBuf};

use buffer::{Buffer, OpenError};
use tree::LazyTree;

/// The open project (data-model.md "Project"): its root and the lazily listed tree, plus the
/// Files pane's selection.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub tree: LazyTree,
    pub selected: usize,
    pub offset: usize,
}

impl Project {
    /// Opens `root`, listing only its top level.
    pub fn open(root: &Path) -> std::io::Result<Project> {
        std::fs::read_dir(root)?;
        let root = std::path::absolute(root)?;
        Ok(Project {
            tree: LazyTree::open(&root),
            root,
            selected: 0,
            offset: 0,
        })
    }

    /// The selected row's path and whether it is a folder.
    pub fn selected_row(&self) -> Option<tree::Row> {
        self.tree.rows().into_iter().nth(self.selected)
    }

    /// Selects the row for `path` if it is visible.
    pub fn select_path(&mut self, path: &Path) {
        if let Some(i) = self.tree.rows().iter().position(|r| r.path == path) {
            self.selected = i;
        }
    }
}

/// The open buffers and which one is shown.
#[derive(Debug, Default)]
pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub active: usize,
    /// `Esc` released focus: the next `Tab` switches panes (FR-065).
    pub released: bool,
    /// Scroll the view to the cursor before the next frame.
    pub follow: bool,
    /// Buffers opened in this run (for `--bench-stats`).
    pub opened: u64,
}

impl Editor {
    pub fn active(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active)
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(path))
    }

    /// Opens `path`, or switches to it if it is already open. Returns its index.
    pub fn open(&mut self, path: &Path, read_only_bytes: u64) -> Result<usize, OpenError> {
        if let Some(i) = self.index_of(path) {
            self.active = i;
            return Ok(i);
        }
        let mut b = Buffer::open(path, read_only_bytes)?;
        // Read-only files are the very large ones: they are shown without highlighting.
        if !b.read_only {
            b.syntax = b.language.map(syntax::SyntaxState::new);
        }
        self.buffers.push(b);
        self.active = self.buffers.len() - 1;
        self.opened += 1;
        self.follow = true;
        Ok(self.active)
    }

    pub fn close(&mut self, i: usize) {
        if i < self.buffers.len() {
            self.buffers.remove(i);
            if self.active >= self.buffers.len() {
                self.active = self.buffers.len().saturating_sub(1);
            } else if self.active > i {
                self.active -= 1;
            }
        }
    }

    pub fn cycle(&mut self, by: isize) {
        let n = self.buffers.len() as isize;
        if n > 0 {
            self.active = (self.active as isize + by).rem_euclid(n) as usize;
            self.follow = true;
        }
    }

    /// Names of buffers with unsaved changes.
    pub fn dirty_names(&self) -> Vec<String> {
        self.buffers
            .iter()
            .filter(|b| b.dirty())
            .map(|b| {
                b.path
                    .as_ref()
                    .map_or_else(|| b.name(), |p| p.display().to_string())
            })
            .collect()
    }
}
