//! The project file tree (FR-002, T067). A node lists its children only when it is expanded,
//! so opening a project reads one directory, never the whole tree (FR-042).

use std::io;
use std::path::{Path, PathBuf};

/// One entry. `children` is `None` until the folder is first expanded.
#[derive(Debug, Clone)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub children: Option<Vec<Node>>,
    /// Why the folder could not be listed, shown under it.
    pub error: Option<String>,
}

impl Node {
    fn new(path: PathBuf, is_dir: bool) -> Node {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Node {
            path,
            name,
            is_dir,
            expanded: false,
            children: None,
            error: None,
        }
    }
}

/// A visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// Folders that are never listed: version-control internals.
const SKIP: &[&str] = &[".git", ".hg", ".svn"];

/// Lists `dir`: folders first, then files, each by case-insensitive name.
pub fn list(dir: &Path) -> io::Result<Vec<Node>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        if SKIP.iter().any(|s| name == *s) {
            continue;
        }
        // Follows symlinks for the folder test, so a linked folder can be expanded.
        let is_dir = entry.path().is_dir();
        out.push(Node::new(entry.path(), is_dir));
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct LazyTree {
    pub root: Node,
}

impl LazyTree {
    /// A tree for `root` with its top level listed and expanded.
    pub fn open(root: &Path) -> LazyTree {
        let mut t = LazyTree {
            root: Node::new(root.to_path_buf(), true),
        };
        t.root.name = root.display().to_string();
        let root = t.root.path.clone();
        t.set_expanded(&root, true);
        t
    }

    fn find_mut<'a>(node: &'a mut Node, path: &Path) -> Option<&'a mut Node> {
        if node.path == path {
            return Some(node);
        }
        if !path.starts_with(&node.path) {
            return None;
        }
        node.children
            .as_mut()?
            .iter_mut()
            .find_map(|c| LazyTree::find_mut(c, path))
    }

    pub fn node(&self, path: &Path) -> Option<&Node> {
        fn find<'a>(node: &'a Node, path: &Path) -> Option<&'a Node> {
            if node.path == path {
                return Some(node);
            }
            if !path.starts_with(&node.path) {
                return None;
            }
            node.children.as_ref()?.iter().find_map(|c| find(c, path))
        }
        find(&self.root, path)
    }

    /// Expands or collapses a folder, listing it the first time. Returns true if it changed.
    pub fn set_expanded(&mut self, path: &Path, open: bool) -> bool {
        let Some(n) = LazyTree::find_mut(&mut self.root, path) else {
            return false;
        };
        if !n.is_dir || n.expanded == open {
            return false;
        }
        n.expanded = open;
        if open && n.children.is_none() {
            match list(&n.path) {
                Ok(c) => {
                    n.children = Some(c);
                    n.error = None;
                }
                Err(e) => {
                    n.children = Some(Vec::new());
                    n.error = Some(e.to_string());
                }
            }
        }
        true
    }

    /// Re-lists a folder that is already listed, keeping what is expanded below it. Used after
    /// file operations and change notifications.
    pub fn refresh(&mut self, dir: &Path) {
        let Some(n) = LazyTree::find_mut(&mut self.root, dir) else {
            return;
        };
        if n.children.is_none() {
            return;
        }
        let old = n.children.take().unwrap_or_default();
        match list(&n.path) {
            Ok(mut fresh) => {
                for f in &mut fresh {
                    if let Some(o) = old
                        .iter()
                        .find(|o| o.path == f.path && o.is_dir == f.is_dir)
                    {
                        f.expanded = o.expanded;
                        f.children = o.children.clone();
                        f.error = o.error.clone();
                    }
                }
                n.children = Some(fresh);
                n.error = None;
            }
            Err(e) => {
                n.children = Some(Vec::new());
                n.error = Some(e.to_string());
            }
        }
    }

    /// The visible rows, depth-first. The root itself is row 0.
    pub fn rows(&self) -> Vec<Row> {
        fn walk(n: &Node, depth: usize, out: &mut Vec<Row>) {
            out.push(Row {
                path: n.path.clone(),
                name: n.name.clone(),
                depth,
                is_dir: n.is_dir,
                expanded: n.expanded,
            });
            if n.expanded {
                for c in n.children.iter().flatten() {
                    walk(c, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, 0, &mut out);
        out
    }

    /// Folders currently expanded (watched for changes, R-8).
    pub fn expanded_dirs(&self) -> Vec<PathBuf> {
        fn walk(n: &Node, out: &mut Vec<PathBuf>) {
            if n.is_dir && n.expanded {
                out.push(n.path.clone());
                for c in n.children.iter().flatten() {
                    walk(c, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// Expands every folder from the root down to `path`'s parent, so `path` is visible.
    pub fn reveal(&mut self, path: &Path) {
        let mut chain: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|a| a.starts_with(&self.root.path))
            .map(Path::to_path_buf)
            .collect();
        chain.reverse();
        for dir in chain {
            self.set_expanded(&dir, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_listing_and_order() {
        let d = std::env::temp_dir().join(format!("fallow-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("b_dir/inner")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("a.txt"), "").unwrap();
        std::fs::write(d.join("B.txt"), "").unwrap();
        let mut t = LazyTree::open(&d);
        let names: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(&names[1..], ["b_dir", "a.txt", "B.txt"]);
        assert!(
            t.node(&d.join("b_dir")).unwrap().children.is_none(),
            "not listed yet"
        );
        assert!(t.set_expanded(&d.join("b_dir"), true));
        assert_eq!(t.rows().len(), 5);
        std::fs::write(d.join("b_dir/new.rs"), "").unwrap();
        t.refresh(&d);
        assert_eq!(
            t.rows().len(),
            5,
            "refreshing the root keeps b_dir as it was"
        );
        t.refresh(&d.join("b_dir"));
        assert_eq!(t.rows().len(), 6);
        assert_eq!(t.expanded_dirs().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
