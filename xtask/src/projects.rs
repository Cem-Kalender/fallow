//! A remembered project list for the startup and idle-memory benchmarks (002 T046, FR-028,
//! FR-029): `count` project folders and a `projects.toml` listing them, in a temporary config
//! directory, so the user's own config is never read or written.

use std::path::{Path, PathBuf};

/// The number of projects the benchmarks list.
pub const COUNT: usize = 50;

/// A TOML basic string.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Creates `<dir>/folders/project-NN` and `<dir>/projects.toml` listing them (the last one
/// selected). Returns the config directory to pass as `--config-dir`.
pub fn write(dir: &Path, count: usize) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut text = format!(
        "# Written by Fallow. Edits are read at the next start.\nselected = {}\n",
        count.saturating_sub(1)
    );
    for i in 0..count {
        let p = dir.join("folders").join(format!("project-{i:02}"));
        std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let path = p.to_str().ok_or("temporary path is not UTF-8")?;
        text.push_str(&format!("\n[[project]]\npath = {}\n", quote(path)));
    }
    let file = dir.join("projects.toml");
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(dir.to_path_buf())
}
