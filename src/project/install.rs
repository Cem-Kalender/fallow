//! Installing Spec Kit into a project (007 research S-15, FR-002): the files Fallow ships, copied
//! only where the project has none. Nothing is downloaded and nothing existing is overwritten.

use std::path::Path;

use serde::Serialize;

/// The Spec Kit version Fallow ships.
pub const VERSION: &str = "1.0.12";

macro_rules! kit {
    ($($p:literal),* $(,)?) => {
        &[$(($p, include_str!(concat!("../../assets/spec-kit/", $p)))),*]
    };
}

/// Every shipped file, by its path in a project. `LICENSE` goes to `.specify/LICENSE-spec-kit`.
pub const FILES: &[(&str, &str)] = kit![
    ".claude/skills/speckit-analyze/SKILL.md",
    ".claude/skills/speckit-checklist/SKILL.md",
    ".claude/skills/speckit-clarify/SKILL.md",
    ".claude/skills/speckit-constitution/SKILL.md",
    ".claude/skills/speckit-converge/SKILL.md",
    ".claude/skills/speckit-implement/SKILL.md",
    ".claude/skills/speckit-plan/SKILL.md",
    ".claude/skills/speckit-specify/SKILL.md",
    ".claude/skills/speckit-tasks/SKILL.md",
    ".claude/skills/speckit-taskstoissues/SKILL.md",
    ".specify/init-options.json",
    ".specify/integration.json",
    ".specify/memory/constitution.md",
    ".specify/scripts/bash/check-prerequisites.sh",
    ".specify/scripts/bash/common.sh",
    ".specify/scripts/bash/create-new-feature.sh",
    ".specify/scripts/bash/resolve-template.sh",
    ".specify/scripts/bash/setup-plan.sh",
    ".specify/scripts/bash/setup-tasks.sh",
    ".specify/templates/checklist-template.md",
    ".specify/templates/constitution-template.md",
    ".specify/templates/plan-template.md",
    ".specify/templates/spec-template.md",
    ".specify/templates/tasks-template.md",
    "LICENSE",
];

fn target(path: &str) -> &str {
    if path == "LICENSE" {
        ".specify/LICENSE-spec-kit"
    } else {
        path
    }
}

/// What installing did, for the engineer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The Spec Kit version the project had before, if any.
    pub found: Option<String>,
    pub installed: Vec<String>,
    /// Files the project already had, left as they were.
    pub kept: Vec<String>,
}

/// The Spec Kit version a project has, from its `.specify/integration.json` or
/// `.specify/init-options.json`.
pub fn version_in(project: &Path) -> Option<String> {
    let read = |p: &str, key: &str| {
        std::fs::read_to_string(project.join(p))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v[key].as_str().map(str::to_string))
    };
    read(".specify/integration.json", "version")
        .or_else(|| read(".specify/init-options.json", "speckit_version"))
}

/// Copies the shipped files that `project` does not have. With Spec Kit at another version
/// already there, nothing is copied: mixing versions would break its scripts.
pub fn install(project: &Path) -> Result<Report, String> {
    let found = version_in(project);
    let mut r = Report {
        found: found.clone(),
        ..Report::default()
    };
    if let Some(v) = &found {
        if v != VERSION {
            r.kept = FILES.iter().map(|(p, _)| target(p).to_string()).collect();
            return Ok(r);
        }
    }
    for (path, text) in FILES {
        let to = project.join(target(path));
        if to.exists() {
            r.kept.push(target(path).to_string());
            continue;
        }
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        // `create_new`: never overwrite, even if the file appeared since the check.
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&to)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                r.kept.push(target(path).to_string());
                continue;
            }
            Err(e) => return Err(format!("{}: {e}", to.display())),
        };
        use std::io::Write;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("{}: {e}", to.display()))?;
        r.installed.push(target(path).to_string());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_matches_the_shipped_folder() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/spec-kit");
        let mut on_disk = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    on_disk.push(rel);
                }
            }
        }
        on_disk.sort();
        let mut listed: Vec<String> = FILES.iter().map(|(p, _)| p.to_string()).collect();
        listed.sort();
        assert_eq!(
            listed, on_disk,
            "after `cargo xtask spec-kit`, update FILES"
        );
        assert!(
            FILES
                .iter()
                .all(|(p, t)| !p.ends_with(".sh") || !t.contains('\r')),
            "LF scripts"
        );
    }
}
