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

/// The default per task before T086.
const OLD_DEFAULT: Budget = Budget {
    usd: 0.25,
    minutes: 10,
};

impl Default for Budget {
    fn default() -> Budget {
        Budget {
            usd: 0.50,
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
    /// 010: what the loop does when it ends verified.
    #[serde(default)]
    pub when_verified: crate::run::events::WhenVerified,
    /// 011: updates in a row that may be overtaken before the engineer decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_limit: Option<u32>,
    /// 011: the loop this one waited for ("Run after").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_after: Option<u64>,
    /// 011: the target commit a "Lock again" was reconciled against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciled_target: Option<String>,
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
    /// The documents changed after analyze ran, so it must run again (008 T042).
    #[serde(default)]
    pub analyze_outdated: bool,
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
    /// 1 once `budget_per_task` moved off the old default (T086); new projects start at 1.
    #[serde(default)]
    pub budget_version: u32,
    #[serde(default = "yes")]
    pub notifications: bool,
    /// The project conversation (FR-034).
    pub conversation: Option<String>,
    #[serde(default)]
    pub features: Vec<Feature>,
    /// The lead new sessions start with (009); none is the agent named `lead`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    /// 010: the remote as last seen, shown at once after a restart, with its time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_seen: Option<crate::publish::Remote>,
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
        let mut projects: Vec<Project> = match std::fs::read_to_string(&file) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        // A project saved with the old default ($0.25 and 10 minutes per task, too low with real
        // agents: T086) takes the new one, once; an amount the engineer set is kept.
        let mut moved = false;
        for p in projects.iter_mut().filter(|p| p.budget_version == 0) {
            if p.budget_per_task == OLD_DEFAULT {
                p.budget_per_task = Budget::default();
            }
            p.budget_version = 1;
            moved = true;
        }
        let store = Store { file, projects };
        if moved {
            store.save()?;
        }
        Ok(store)
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
            budget_version: 1,
            notifications: true,
            conversation: None,
            features: Vec::new(),
            lead: None,
            remote_seen: None,
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

    #[test]
    fn a_project_on_the_old_default_budget_takes_the_new_one() {
        let dir = std::env::temp_dir().join(format!("fallow-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::create_dir_all(dir.join("mine")).unwrap();
        let mut s = Store::load(&dir).unwrap();
        s.add(&dir.join("old"), "2026-10-30T00:00:00Z").unwrap();
        s.add(&dir.join("mine"), "2026-10-30T00:00:00Z").unwrap();
        s.projects[0].budget_per_task = OLD_DEFAULT;
        s.projects[0].budget_version = 0;
        s.projects[1].budget_version = 0;
        s.projects[1].budget_per_task = Budget {
            usd: 0.25,
            minutes: 20,
        };
        s.save().unwrap();
        let s = Store::load(&dir).unwrap();
        assert_eq!(s.projects[0].budget_per_task, Budget::default());
        assert_eq!(
            s.projects[1].budget_per_task.minutes, 20,
            "the engineer's own"
        );
        // Once moved, the old amount set again on purpose stays.
        let mut s = s;
        s.projects[0].budget_per_task = OLD_DEFAULT;
        s.save().unwrap();
        assert_eq!(
            Store::load(&dir).unwrap().projects[0].budget_per_task,
            OLD_DEFAULT
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_and_a_lock_from_before_010_still_read() {
        let p: Project = serde_json::from_str(
            r#"{"id":"shop","path":"C:/x","name":"shop","added":"2026-10-30T00:00:00Z","conversation":null}"#,
        )
        .unwrap();
        assert!(p.remote_seen.is_none());
        let l: LockRec = serde_json::from_str(
            r#"{"n":1,"commit":"c","at":"t","pipeline":"standard","level":"free","budget_usd":1.0,"budget_minutes":10}"#,
        )
        .unwrap();
        assert_eq!(l.when_verified, crate::run::events::WhenVerified::Wait);
        let mut p = p;
        p.remote_seen = Some(crate::publish::Remote {
            state: "known".into(),
            remote: Some("origin".into()),
            service: Some("github".into()),
            repo: Some(crate::publish::host::Repo {
                owner: "acme".into(),
                name: "shop".into(),
            }),
            target: Some("main".into()),
            protected: "yes".into(),
            why: None,
            account: None,
            checked_at: Some("t".into()),
            stale: false,
            checking: false,
        });
        let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.remote_seen, p.remote_seen);
    }
}
