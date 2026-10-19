//! Change notifications (FR-006, FR-042, R-8, T079).
//!
//! Watches are **non-recursive** and cover only what is open: the folders that hold open files
//! and the folders expanded in the tree. Never a whole project tree. An open file is watched
//! through its folder rather than by itself, because agents often save by writing a temporary
//! file and renaming it over the original, which a watch on the original file would miss
//! (Linux inotify watches the replaced inode). The watcher is created on first use, never
//! before the first frame.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
    rx: Option<Receiver<notify::Result<notify::Event>>>,
    watched: BTreeSet<PathBuf>,
    /// Set when the watcher could not be created; change detection is then off.
    pub error: Option<String>,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWatcher")
            .field("watched", &self.watched)
            .field("error", &self.error)
            .finish()
    }
}

/// What changed, by path.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Paths that were created, modified, removed or renamed.
    pub paths: BTreeSet<PathBuf>,
}

impl FileWatcher {
    /// The folders currently watched.
    pub fn watched(&self) -> &BTreeSet<PathBuf> {
        &self.watched
    }

    /// Makes the watched set equal to `dirs`. Returns false if watching is unavailable.
    pub fn sync(&mut self, dirs: BTreeSet<PathBuf>) -> bool {
        if dirs == self.watched {
            return true;
        }
        if self.watcher.is_none() {
            if dirs.is_empty() || self.error.is_some() {
                return self.error.is_none();
            }
            let (tx, rx) = channel();
            match notify::recommended_watcher(tx) {
                Ok(w) => {
                    self.watcher = Some(w);
                    self.rx = Some(rx);
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    return false;
                }
            }
        }
        let Some(w) = &mut self.watcher else {
            return false;
        };
        for gone in self.watched.difference(&dirs) {
            let _ = w.unwatch(gone);
        }
        let mut now = BTreeSet::new();
        for d in &dirs {
            if self.watched.contains(d) || w.watch(d, RecursiveMode::NonRecursive).is_ok() {
                now.insert(d.clone());
            }
        }
        self.watched = now;
        true
    }

    /// Drains pending notifications.
    pub fn poll(&mut self) -> Changes {
        let mut out = Changes::default();
        let Some(rx) = &self.rx else {
            return out;
        };
        loop {
            match rx.try_recv() {
                Ok(Ok(ev)) => {
                    if matches!(ev.kind, EventKind::Access(_)) {
                        continue;
                    }
                    out.paths.extend(ev.paths);
                }
                Ok(Err(_)) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

/// The folders to watch for these open files and expanded folders.
pub fn watch_set<'a>(
    open_files: impl IntoIterator<Item = &'a Path>,
    expanded: impl IntoIterator<Item = PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut s: BTreeSet<PathBuf> = expanded.into_iter().collect();
    for f in open_files {
        if let Some(p) = f.parent() {
            if p.is_dir() {
                s.insert(p.to_path_buf());
            }
        }
    }
    s
}
