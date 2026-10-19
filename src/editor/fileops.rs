//! File operations from the Files pane (FR-068, FR-034, T070): new file, new folder, rename and
//! delete. These are the only writes Fallow makes outside the files the user saves, and each
//! one is an explicit user action. Delete additionally needs a [`ConfirmedToken`] for the exact
//! path, which only the confirmation dialog can produce.

use std::io;
use std::path::{Path, PathBuf};

use crate::ui::confirm::{ConfirmedToken, Dialog};

/// Where a new item goes: inside the selected folder, or next to the selected file.
pub fn target_dir(selected: &Path, selected_is_dir: bool) -> PathBuf {
    if selected_is_dir {
        selected.to_path_buf()
    } else {
        selected
            .parent()
            .map_or_else(|| selected.to_path_buf(), Path::to_path_buf)
    }
}

/// Checks a new name: one path component, not empty, not `.` or `..`.
pub fn validate_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name is empty".into());
    }
    if name == "." || name == ".." {
        return Err("Name cannot be . or ..".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain / or \\".into());
    }
    if cfg!(windows) && name.chars().any(|c| "<>:\"|?*".contains(c)) {
        return Err("Name cannot contain < > : \" | ? *".into());
    }
    Ok(())
}

fn exists_error(p: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} already exists", p.display()),
    )
}

/// Creates an empty file. Never overwrites.
pub fn new_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name).map_err(io::Error::other)?;
    let p = dir.join(name.trim());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                exists_error(&p)
            } else {
                e
            }
        })?;
    Ok(p)
}

pub fn new_folder(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name).map_err(io::Error::other)?;
    let p = dir.join(name.trim());
    if p.exists() {
        return Err(exists_error(&p));
    }
    std::fs::create_dir(&p)?;
    Ok(p)
}

/// Renames within the same folder. Never overwrites.
pub fn rename(from: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name).map_err(io::Error::other)?;
    let to = from.parent().map_or_else(
        || PathBuf::from(new_name.trim()),
        |d| d.join(new_name.trim()),
    );
    if to == from {
        return Ok(to);
    }
    // A case-only rename on a case-insensitive file system reports the target as existing.
    let case_only = to
        .to_string_lossy()
        .eq_ignore_ascii_case(&from.to_string_lossy());
    if to.exists() && !case_only {
        return Err(exists_error(&to));
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Files and folders inside `dir`, recursively (for the delete dialog). Symlinks are counted,
/// not followed.
pub fn count_items(dir: &Path) -> (usize, usize) {
    let (mut files, mut folders) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    folders += 1;
                    stack.push(e.path());
                }
                Ok(_) => files += 1,
                Err(_) => {}
            }
        }
    }
    (files, folders)
}

/// The delete confirmation (FR-068): full path, counts for a folder, and the no-trash warning.
pub fn delete_dialog(path: &Path) -> Dialog {
    let is_dir = path.is_dir();
    let mut body = vec![path.display().to_string()];
    if is_dir {
        let (files, folders) = count_items(path);
        body.push(format!(
            "This folder contains {files} file{} and {folders} folder{}.",
            if files == 1 { "" } else { "s" },
            if folders == 1 { "" } else { "s" }
        ));
    }
    body.push("Deletion is permanent. v1 has no trash integration.".into());
    Dialog::new(
        if is_dir {
            "Delete folder"
        } else {
            "Delete file"
        },
        &path.display().to_string(),
        body,
        "Delete",
    )
}

/// Deletes `path` (a folder with everything in it). The token must be for this exact path.
pub fn delete(path: &Path, token: ConfirmedToken) -> io::Result<()> {
    if token.subject() != path.display().to_string() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the confirmation was for a different path",
        ));
    }
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
