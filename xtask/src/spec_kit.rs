//! `cargo xtask spec-kit <project>`: refresh the Spec Kit files Fallow ships (007 research S-15).
//!
//! `<project>` is a folder a developer initialised with Spec Kit's own installer, at the version
//! Fallow should ship. Its files replace `assets/spec-kit/`, which Fallow embeds and copies into the
//! projects the engineer adds. Fallow itself never downloads Spec Kit. The MIT `LICENSE` in
//! `assets/spec-kit/` is kept, and must be checked by hand when the version changes.
//!
//! The new copy is built in a sibling folder and swapped in only when it is complete, so a failed
//! refresh leaves the shipped files as they were.

use std::fs;
use std::io;
use std::path::Path;

/// What is taken from `.specify/`: what the skills and the bash scripts read. Not the PowerShell
/// scripts (no skill calls them), nor the installer's manifests and workflows, which describe the
/// developer's own install.
const SPECIFY_ITEMS: &[&str] = &[
    "scripts/bash",
    "templates",
    "init-options.json",
    "integration.json",
];

pub fn run(args: &[String]) -> Result<(), String> {
    let [src] = args else {
        return Err("usage: cargo xtask spec-kit <project initialised with Spec Kit>".into());
    };
    let src = fs::canonicalize(src).map_err(|e| format!("{src}: {e}"))?;
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
    fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
    let assets = fs::canonicalize(&assets).map_err(|e| e.to_string())?;
    if src.starts_with(&assets) {
        return Err("the source must be a project outside assets/".into());
    }
    if !src.join(".specify").is_dir() || !src.join(".claude/skills").is_dir() {
        return Err(format!(
            "{} has no .specify/ and .claude/skills/: initialise it with Spec Kit first",
            src.display()
        ));
    }
    let root = assets.join("spec-kit");
    let next = assets.join("spec-kit.new");
    if next.exists() {
        fs::remove_dir_all(&next).map_err(|e| e.to_string())?;
    }
    let built = build(&src, &next, &root.join("LICENSE"));
    if let Err(e) = built {
        let _ = fs::remove_dir_all(&next);
        return Err(e);
    }
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    fs::rename(&next, &root).map_err(|e| e.to_string())?;
    println!("refreshed {}", root.display());
    Ok(())
}

/// Builds the copy in `dst`, checking that everything it needs is there.
fn build(src: &Path, dst: &Path, license: &Path) -> Result<(), String> {
    let specify = dst.join(".specify");
    fs::create_dir_all(specify.join("memory")).map_err(|e| e.to_string())?;
    for item in SPECIFY_ITEMS {
        let from = src.join(".specify").join(item);
        if !from.exists() {
            return Err(format!("{} is missing", from.display()));
        }
        let to = specify.join(item);
        if let Some(p) = to.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    // A new project starts from the constitution template, never from this project's own.
    fs::copy(
        src.join(".specify/templates/constitution-template.md"),
        specify.join("memory/constitution.md"),
    )
    .map_err(|e| format!("constitution template: {e}"))?;
    let skills = dst.join(".claude/skills");
    fs::create_dir_all(&skills).map_err(|e| e.to_string())?;
    let mut n = 0;
    for entry in fs::read_dir(src.join(".claude/skills")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("speckit-") {
            copy(&entry.path(), &skills.join(&name)).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    if n == 0 {
        return Err("no speckit-* skills in .claude/skills".into());
    }
    fs::copy(license, dst.join("LICENSE")).map_err(|e| {
        format!(
            "{}: {e} (add Spec Kit's MIT license there first)",
            license.display()
        )
    })?;
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
