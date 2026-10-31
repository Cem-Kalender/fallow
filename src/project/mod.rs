//! Projects (007 data-model.md, Project and Feature; research S-15, S-16;
//! contracts/storage.md): the folders the engineer added, kept in `projects.json`, with Spec Kit
//! installed into each where its files are missing, and their features.

pub mod commands;
pub mod data;
pub mod install;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::run::events::Level;

/// The default cost and time per task, from which a lock's budget is suggested (FR-016a).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    pub usd: f64,
    pub minutes: u64,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget {
            usd: 0.25,
            minutes: 10,
        }
    }
}

/// A lock of a feature (data-model.md, Lock).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockRec {
    pub n: u32,
    pub commit: String,
    pub at: String,
    pub pipeline: String,
    pub level: Level,
    pub budget_usd: f64,
    pub budget_minutes: u64,
    #[serde(default)]
    pub affected: Vec<String>,
}

/// A feature of a project, as Fallow knows it (data-model.md, Feature). Its stage is derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// `specs/NNN-name`.
    pub dir: String,
    pub title: String,
    pub created: String,
    /// The branch it started from: the merge target. None in a plain folder.
    pub base_branch: Option<String>,
    /// `NNN-name`, for documents, locks and answers.
    pub branch: Option<String>,
    /// The spec session.
    pub conversation: Option<String>,
    /// The Spec Kit step the session is at.
    #[serde(default = "first_step")]
    pub step: String,
    pub pipeline: Option<String>,
    pub level: Option<Level>,
    #[serde(default)]
    pub locks: Vec<LockRec>,
    /// The loop, once locked.
    pub run: Option<u64>,
    /// Unlocked to change the spec (FR-009).
    #[serde(default)]
    pub unlocked: bool,
    /// How its loop ended, kept when the loop's data is deleted (FR-003b): `merged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    /// The analyze step ran, and what it found (FR-008).
    #[serde(default)]
    pub analyzed: bool,
    #[serde(default)]
    pub findings: Vec<crate::speckit::lock::Finding>,
}

fn first_step() -> String {
    "specify".into()
}

impl Feature {
    /// `NNN-name`.
    pub fn name(&self) -> &str {
        self.dir.rsplit('/').next().unwrap_or(&self.dir)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub added: String,
    #[serde(default = "standard")]
    pub pipeline: String,
    #[serde(default)]
    pub level: Level,
    #[serde(default)]
    pub approved_commands: Vec<String>,
    #[serde(default)]
    pub budget_per_task: Budget,
    #[serde(default = "yes")]
    pub notifications: bool,
    /// The project conversation (FR-034).
    pub conversation: Option<String>,
    #[serde(default)]
    pub features: Vec<Feature>,
}

fn standard() -> String {
    "standard".into()
}

fn yes() -> bool {
    true
}

/// Git or a plain folder, read each time (data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Git,
    Folder,
    Missing,
}

impl Project {
    pub fn kind(&self) -> Kind {
        kind_of(&self.path)
    }

    pub fn feature(&self, name: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.name() == name)
    }

    pub fn feature_mut(&mut self, name: &str) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.name() == name)
    }

    /// Every `specs/NNN-*` folder, the ones Fallow started and the others.
    pub fn spec_dirs(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.path.join("specs"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| feature_number(n).is_some())
            .map(|n| format!("specs/{n}"))
            .collect();
        out.sort();
        out
    }

    /// The next feature folder for `title` (`specs/009-discount-codes`): Spec Kit's sequential
    /// numbering, after every existing folder and every feature Fallow knows.
    pub fn next_dir(&self, title: &str) -> String {
        let n = self
            .spec_dirs()
            .iter()
            .map(String::as_str)
            .chain(self.features.iter().map(|f| f.dir.as_str()))
            .filter_map(|d| feature_number(d.rsplit('/').next().unwrap_or(d)))
            .max()
            .unwrap_or(0)
            + 1;
        let slug = slug(title);
        format!(
            "specs/{n:03}-{}",
            if slug.is_empty() { "feature" } else { &slug }
        )
    }
}

/// `007-fallow-studio` gives 7.
pub fn feature_number(name: &str) -> Option<u32> {
    let (n, rest) = name.split_once('-')?;
    (n.len() == 3 && !rest.is_empty())
        .then(|| n.parse().ok())
        .flatten()
}

/// A short name for ids and branches: lower case, words joined by `-`, at most 4 words.
pub fn slug(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
}

/// Whether `path` is the top of a git repository, a plain folder, or gone.
pub fn kind_of(path: &Path) -> Kind {
    if !path.is_dir() {
        return Kind::Missing;
    }
    let top = crate::platform::command("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    match (
        top.and_then(|t| t.canonicalize().ok()),
        path.canonicalize().ok(),
    ) {
        (Some(t), Some(p)) if t == p => Kind::Git,
        _ => Kind::Folder,
    }
}

/// The projects, kept in `projects.json` (contracts/storage.md), written atomically.
#[derive(Debug)]
pub struct Store {
    file: PathBuf,
    pub projects: Vec<Project>,
}

impl Store {
    /// Loads `dir/projects.json`, or starts empty.
    pub fn load(dir: &Path) -> Result<Store, String> {
        let file = dir.join("projects.json");
        let projects = match std::fs::read_to_string(&file) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        Ok(Store { file, projects })
    }

    /// Writes the file whole, through a temporary file and a rename.
    pub fn save(&self) -> Result<(), String> {
        let tmp = self.file.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.projects).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.file).map_err(|e| format!("{}: {e}", self.file.display()))
    }

    pub fn get(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Adds the folder at `path` (FR-001) and installs Spec Kit where its files are missing
    /// (FR-002). A folder already added is refused.
    pub fn add(&mut self, path: &Path, now: &str) -> Result<(Project, install::Report), String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.is_dir() {
            return Err(format!("{} is not a folder", path.display()));
        }
        // Windows' canonical form starts with \\?\; keep the plain path for display and git.
        let path = PathBuf::from(path.display().to_string().trim_start_matches(r"\\?\"));
        if let Some(p) = self.projects.iter().find(|p| p.path == path) {
            return Err(format!(
                "{} is already the project {}",
                path.display(),
                p.name
            ));
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        let base = match slug(&name) {
            s if s.is_empty() => "project".to_string(),
            s => s,
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.projects.iter().any(|p| p.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        let report = install::install(&path)?;
        let p = Project {
            id,
            path,
            name,
            added: now.to_string(),
            pipeline: standard(),
            level: Level::Normal,
            approved_commands: Vec::new(),
            budget_per_task: Budget::default(),
            notifications: true,
            conversation: None,
            features: Vec::new(),
        };
        self.projects.push(p.clone());
        self.save()?;
        Ok((p, report))
    }

    /// Removes the project from Fallow (FR-001): nothing in its folder is touched.
    pub fn remove(&mut self, id: &str) -> Result<Project, String> {
        let i = self
            .projects
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| format!("there is no project {id}"))?;
        let p = self.projects.remove(i);
        self.save()?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_slugs() {
        assert_eq!(feature_number("007-fallow-studio"), Some(7));
        assert_eq!(feature_number("7-x"), None);
        assert_eq!(feature_number("abc-x"), None);
        assert_eq!(
            slug("Discount codes, before tax!"),
            "discount-codes-before-tax"
        );
        assert_eq!(slug("  "), "");
    }
}
