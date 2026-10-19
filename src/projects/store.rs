//! `projects.toml` (002 contracts/config.md): read once after the first frame, written atomically
//! after every change to the list. It holds paths, names and the selected index only (FR-026,
//! FR-027).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use super::{MAX_PROJECTS, ProjectList, check_name, folder_name, same_path};
use crate::config::merge::line_of;
use crate::config::{Level, Notice};

pub const PROJECTS_FILE: &str = "projects.toml";

const HEADER: &str = "# Written by Fallow. Edits are read at the next start.\n";

/// What `projects.toml` held.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    /// Path and name (if the user renamed the project), in the file's order.
    pub entries: Vec<(PathBuf, Option<String>)>,
    pub selected: Option<usize>,
    pub notices: Vec<Notice>,
    /// False when the file could not be read or parsed at all. The list is then not written
    /// until the file is fixed, so a typo never wipes the user's list.
    pub writable: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawFile {
    selected: Option<Spanned<toml::Value>>,
    #[serde(default)]
    project: Vec<Spanned<toml::Table>>,
}

/// Parses the file's text. Invalid entries are skipped with `file:line: message`.
pub fn parse(file: &str, text: &str) -> Loaded {
    let mut out = Loaded {
        writable: true,
        ..Loaded::default()
    };
    let notice = |line: Option<usize>, level, message: String| Notice {
        file: file.to_string(),
        line,
        level,
        message,
    };
    let raw: RawFile = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            out.notices.push(notice(
                line,
                Level::Error,
                format!(
                    "{} (the project list is not saved until this is fixed)",
                    e.message()
                ),
            ));
            out.writable = false;
            return out;
        }
    };
    for entry in raw.project {
        let line = Some(line_of(text, entry.span().start));
        let t = entry.into_inner();
        let Some(path) = t.get("path").and_then(toml::Value::as_str) else {
            out.notices.push(notice(
                line,
                Level::Error,
                "project needs a path (a string); entry skipped".into(),
            ));
            continue;
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            out.notices.push(notice(
                line,
                Level::Error,
                format!("{}: path must be absolute; entry skipped", path.display()),
            ));
            continue;
        }
        if out.entries.iter().any(|(p, _)| same_path(p, &path)) {
            out.notices.push(notice(
                line,
                Level::Warning,
                format!(
                    "{}: listed twice; the later entry is skipped",
                    path.display()
                ),
            ));
            continue;
        }
        let name = match t.get("name") {
            None => None,
            Some(v) => match v.as_str().map(check_name) {
                Some(Ok(n)) => Some(n),
                _ => {
                    out.notices.push(notice(
                        line,
                        Level::Warning,
                        "name must be a string of 1..=64 chars; the folder name is used".into(),
                    ));
                    None
                }
            },
        };
        if out.entries.len() >= MAX_PROJECTS {
            out.notices.push(notice(
                line,
                Level::Warning,
                format!("the list holds at most {MAX_PROJECTS} projects; the rest are skipped"),
            ));
            break;
        }
        out.entries.push((path, name));
    }
    if let Some(sel) = raw.selected {
        let line = Some(line_of(text, sel.span().start));
        match sel.into_inner().as_integer() {
            Some(i) if i >= 0 && (i as usize) < out.entries.len() => {
                out.selected = Some(i as usize);
            }
            _ => out.notices.push(notice(
                line,
                Level::Warning,
                "selected must be the index of a listed project; ignored".into(),
            )),
        }
    }
    out
}

/// Reads `projects.toml` in `dir`. A missing file is an empty, writable list.
pub fn read(dir: &Path) -> Loaded {
    let path = dir.join(PROJECTS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&path.display().to_string(), &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded {
            writable: true,
            ..Loaded::default()
        },
        Err(e) => Loaded {
            notices: vec![Notice {
                file: path.display().to_string(),
                line: None,
                level: Level::Error,
                message: format!(
                    "cannot read: {e} (the project list is not saved until this is fixed)"
                ),
            }],
            ..Loaded::default()
        },
    }
}

/// A TOML basic string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The file's text for `list`: paths, names the user chose, and the selected index. Paths
/// that are not valid Unicode can't be written to TOML and are left out.
pub fn render(list: &ProjectList) -> String {
    let mut out = String::from(HEADER);
    let written: Vec<usize> = (0..list.projects.len())
        .filter(|&i| list.projects[i].path.to_str().is_some())
        .collect();
    if let Some(sel) = list.selected {
        if let Some(i) = written.iter().position(|&w| w == sel) {
            out.push_str(&format!("selected = {i}\n"));
        }
    }
    for &i in &written {
        let p = &list.projects[i];
        let path = p.path.to_str().expect("filtered above");
        out.push_str(&format!("\n[[project]]\npath = {}\n", quote(path)));
        if p.name != folder_name(&p.path) {
            out.push_str(&format!("name = {}\n", quote(&p.name)));
        }
    }
    out
}

/// Writes `projects.toml` in `dir` atomically: a temporary file in the same directory, then a
/// rename over the old file.
pub fn write(dir: &Path, list: &ProjectList) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{PROJECTS_FILE}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(render(list).as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, dir.join(PROJECTS_FILE))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
