//! What Fallow keeps on disk and how large it is (007 FR-003b, constitution V): per project, its
//! loops, conversations and worktrees.

use std::path::Path;

/// The size of everything under `path`, in bytes; links are not followed.
pub fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

/// Removes a folder of Fallow's data: its git worktrees are removed from the repository first,
/// so the project's own `.git` does not keep them.
pub fn remove(dir: &Path, repo: Option<&Path>) -> Result<(), String> {
    if let Some(repo) = repo {
        for wt in [dir.join("wt"), dir.to_path_buf()] {
            for e in std::fs::read_dir(&wt).into_iter().flatten().flatten() {
                if e.path().join(".git").is_file() {
                    let _ = crate::verify::git::worktree_remove(repo, &e.path());
                }
            }
        }
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_add_up_and_links_are_not_followed() {
        let d = std::env::temp_dir().join(format!("fallow-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("x"), [0u8; 10]).unwrap();
        std::fs::write(d.join("a/b/y"), [0u8; 32]).unwrap();
        assert_eq!(size(&d), 42);
        assert_eq!(size(&d.join("x")), 10);
        assert_eq!(size(&d.join("missing")), 0);
        remove(&d, None).unwrap();
        assert!(!d.exists());
    }
}
