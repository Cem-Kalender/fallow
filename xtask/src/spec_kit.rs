//! `cargo xtask spec-kit <project>`: refresh the Spec Kit files Fallow ships (007 research S-15).
//!
//! `<project>` is a folder a developer initialised with Spec Kit's own installer, at the version
//! Fallow should ship. Its files replace `assets/spec-kit/`, which Fallow embeds and copies into the
//! projects the engineer adds. Fallow itself never downloads Spec Kit. The MIT `LICENSE` in
//! `assets/spec-kit/` is kept, and must be checked by hand when the version changes.

use std::fs;
use std::io;
use std::path::Path;

/// What is taken from the project: under `.specify/`, then the skills.
const SPECIFY_ITEMS: &[&str] = &[
    "scripts",
    "templates",
    "integrations",
    "workflows",
    "init-options.json",
    "integration.json",
];

pub fn run(args: &[String]) -> Result<(), String> {
    let [src] = args else {
        return Err("usage: cargo xtask spec-kit <project initialised with Spec Kit>".into());
    };
    let src = Path::new(src);
    if !src.join(".specify").is_dir() || !src.join(".claude/skills").is_dir() {
        return Err(format!(
            "{} has no .specify/ and .claude/skills/: initialise it with Spec Kit first",
            src.display()
        ));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/spec-kit");
    let license = fs::read(root.join("LICENSE")).ok();
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    let dst = root.join(".specify");
    fs::create_dir_all(dst.join("memory")).map_err(|e| e.to_string())?;
    for item in SPECIFY_ITEMS {
        let from = src.join(".specify").join(item);
        if from.exists() {
            copy(&from, &dst.join(item)).map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    // A new project starts from the constitution template, never from this project's own.
    fs::copy(
        src.join(".specify/templates/constitution-template.md"),
        dst.join("memory/constitution.md"),
    )
    .map_err(|e| format!("constitution template: {e}"))?;
    let skills = root.join(".claude/skills");
    fs::create_dir_all(&skills).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src.join(".claude/skills")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("speckit-") {
            copy(&entry.path(), &skills.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    if let Some(l) = license {
        fs::write(root.join("LICENSE"), l).map_err(|e| e.to_string())?;
    } else {
        eprintln!("warning: assets/spec-kit/LICENSE is missing; add Spec Kit's MIT license");
    }
    println!("refreshed {}", root.display());
    Ok(())
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}
