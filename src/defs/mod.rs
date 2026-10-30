//! Agent definitions and pipelines (007 contracts/agent-definition.md, contracts/pipeline.md;
//! research S-11, S-12): parsed, checked and resolved from what Fallow ships, the engineer's
//! `~/.fallow/`, and a project's `.fallow/`. The later source wins by name.

pub mod agent;
pub mod pipeline;

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

pub use agent::{AgentDef, Kind, Source};
pub use pipeline::Pipeline;

/// The agents Fallow ships (FR-025).
pub const DEFAULT_AGENTS: &[(&str, &str)] = &[
    ("lead", include_str!("../../assets/agents/lead.md")),
    ("builder", include_str!("../../assets/agents/builder.md")),
    ("reviewer", include_str!("../../assets/agents/reviewer.md")),
    (
        "test-writer",
        include_str!("../../assets/agents/test-writer.md"),
    ),
    (
        "security-reviewer",
        include_str!("../../assets/agents/security-reviewer.md"),
    ),
    (
        "docs-writer",
        include_str!("../../assets/agents/docs-writer.md"),
    ),
];

/// The pipelines Fallow ships (FR-025).
pub const DEFAULT_PIPELINES: &[(&str, &str)] = &[
    (
        "standard",
        include_str!("../../assets/pipelines/standard.toml"),
    ),
    (
        "test-first",
        include_str!("../../assets/pipelines/test-first.toml"),
    ),
    ("solo", include_str!("../../assets/pipelines/solo.toml")),
];

/// Files read from one place: (name, file shown in problems, text).
type Files = Vec<(String, String, String)>;
/// The defaults of one kind: (folder, extension, shipped files).
type Shipped<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// The runtimes Fallow knows in this version (contracts/runtime.md).
pub const RUNTIMES: &[&str] = &["claude", "fake"];

/// What a library holds after loading: the definitions in force, and the files that could not be
/// used, with why.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Library {
    pub agents: BTreeMap<String, AgentDef>,
    pub pipelines: BTreeMap<String, Pipeline>,
    pub problems: Vec<String>,
    /// Defaults an update changed that the engineer had edited: written as `<name>.new`.
    pub updates: Vec<String>,
}

fn read_dir_files(dir: &Path, ext: &str) -> Files {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some(ext) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push((stem.to_string(), path.display().to_string(), text));
        }
    }
    out.sort();
    out
}

impl Library {
    /// Loads the definitions: shipped, then `user` (`~/.fallow`), then `project` (its folder).
    /// A file that does not parse is reported and leaves the earlier definition in force.
    pub fn load(user: Option<&Path>, project: Option<&Path>) -> Library {
        let mut lib = Library::default();
        let mut sources: Vec<(Source, Files)> = vec![(
            Source::Default,
            DEFAULT_AGENTS
                .iter()
                .map(|(n, t)| (n.to_string(), format!("(built in) {n}.md"), t.to_string()))
                .collect(),
        )];
        if let Some(u) = user {
            sources.push((Source::User, read_dir_files(&u.join("agents"), "md")));
        }
        if let Some(p) = project {
            sources.push((
                Source::Project,
                read_dir_files(&p.join(".fallow").join("agents"), "md"),
            ));
        }
        for (source, files) in sources {
            for (stem, file, text) in files {
                match agent::parse(&file, &stem, &text, source, RUNTIMES) {
                    Ok(d) => {
                        // A copied default that is unchanged keeps the source "default".
                        let d = match (source, DEFAULT_AGENTS.iter().find(|(n, _)| *n == stem)) {
                            (Source::User, Some((_, shipped))) if *shipped == text => AgentDef {
                                source: Source::Default,
                                ..d
                            },
                            _ => d,
                        };
                        lib.agents.insert(stem, d);
                    }
                    Err(ps) => lib.problems.extend(ps.iter().map(|p| p.to_string())),
                }
            }
        }
        let mut psources: Vec<(Source, Files)> = vec![(
            Source::Default,
            DEFAULT_PIPELINES
                .iter()
                .map(|(n, t)| (n.to_string(), format!("(built in) {n}.toml"), t.to_string()))
                .collect(),
        )];
        if let Some(u) = user {
            psources.push((Source::User, read_dir_files(&u.join("pipelines"), "toml")));
        }
        if let Some(p) = project {
            psources.push((
                Source::Project,
                read_dir_files(&p.join(".fallow").join("pipelines"), "toml"),
            ));
        }
        let kinds: BTreeMap<String, Kind> = lib
            .agents
            .iter()
            .map(|(n, d)| (n.clone(), d.kind))
            .collect();
        let kind_of = |n: &str| kinds.get(n).copied();
        for (source, files) in psources {
            for (stem, file, text) in files {
                match pipeline::parse(&file, &stem, &text, source, &kind_of) {
                    Ok(p) => {
                        let p = match (source, DEFAULT_PIPELINES.iter().find(|(n, _)| *n == stem)) {
                            (Source::User, Some((_, shipped))) if *shipped == text => Pipeline {
                                source: Source::Default,
                                ..p
                            },
                            _ => p,
                        };
                        lib.pipelines.insert(stem, p);
                    }
                    Err(ps) => lib.problems.extend(ps),
                }
            }
        }
        lib
    }

    /// The definition an agent step uses.
    pub fn agent(&self, name: &str) -> Option<&AgentDef> {
        self.agents.get(name)
    }
}

/// A 64-bit FNV-1a hash, to tell whether a shipped file changed.
fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Copies the shipped defaults into `user` (`~/.fallow`) on first start, and never overwrites
/// the engineer's changes (FR-025). When an update changes a default:
/// - a copy the engineer never edited is replaced;
/// - an edited copy is kept, and the new default is written beside it as `<name>.md.new`.
///
/// Returns the names written as `.new`.
pub fn install_defaults(user: &Path) -> Result<Vec<String>, String> {
    let record_path = user.join(".shipped.json");
    let mut record: BTreeMap<String, String> = std::fs::read_to_string(&record_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut news = Vec::new();
    let groups: [Shipped; 2] = [
        ("agents", "md", DEFAULT_AGENTS),
        ("pipelines", "toml", DEFAULT_PIPELINES),
    ];
    for (sub, ext, files) in groups {
        let dir = user.join(sub);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for (name, text) in files {
            let key = format!("{sub}/{name}.{ext}");
            let path = dir.join(format!("{name}.{ext}"));
            let shipped = fnv(text);
            let write =
                |p: &Path| std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()));
            match std::fs::read_to_string(&path) {
                Err(_) => {
                    // Missing: first start, or the engineer deleted it; either way, put it back
                    // only on first start (no record yet).
                    if !record.contains_key(&key) {
                        write(&path)?;
                    }
                }
                Ok(current) => {
                    let last = record.get(&key).cloned();
                    if last.as_deref() == Some(shipped.as_str()) {
                        continue;
                    }
                    let untouched = last.as_deref() == Some(fnv(&current).as_str());
                    if untouched || current == *text {
                        write(&path)?;
                    } else {
                        write(&dir.join(format!("{name}.{ext}.new")))?;
                        news.push(key.clone());
                    }
                }
            }
            record.insert(key, shipped);
        }
    }
    std::fs::write(
        &record_path,
        serde_json::to_string_pretty(&record).unwrap_or_default(),
    )
    .map_err(|e| format!("{}: {e}", record_path.display()))?;
    Ok(news)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-defs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_built_in_library_has_the_defaults() {
        let lib = Library::load(None, None);
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert_eq!(lib.agents.len(), 6);
        assert_eq!(lib.pipelines.len(), 3);
        assert!(lib.pipelines["standard"].verifying);
        assert!(!lib.pipelines["solo"].verifying);
    }

    #[test]
    fn a_project_overrides_by_name_and_a_broken_file_is_reported() {
        let user = tmp("user");
        let proj = tmp("proj");
        install_defaults(&user).unwrap();
        std::fs::create_dir_all(proj.join(".fallow/agents")).unwrap();
        std::fs::write(
            proj.join(".fallow/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: stricter\nruntime: claude\nmodel: opus\nkind: checker\nreturns: verdict\n---\nCheck harder.\n",
        )
        .unwrap();
        std::fs::write(proj.join(".fallow/agents/broken.md"), "no frontmatter\n").unwrap();
        std::fs::write(
            user.join("agents/ui-checker.md"),
            "---\nname: ui-checker\ndescription: walks the stories\nruntime: claude\nkind: checker\nreturns: verdict\n---\nOpen it.\n",
        )
        .unwrap();
        let lib = Library::load(Some(&user), Some(&proj));
        assert_eq!(lib.agents["reviewer"].source, Source::Project);
        assert_eq!(lib.agents["reviewer"].model, "opus");
        assert_eq!(
            lib.agents["builder"].source,
            Source::Default,
            "an unchanged copy"
        );
        assert_eq!(lib.agents["ui-checker"].source, Source::User);
        assert!(
            lib.problems.iter().any(|p| p.contains("broken.md")),
            "{:?}",
            lib.problems
        );
        let _ = std::fs::remove_dir_all(&user);
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn defaults_are_copied_once_and_edits_are_never_overwritten() {
        let user = tmp("install");
        assert!(install_defaults(&user).unwrap().is_empty());
        let builder = user.join("agents/builder.md");
        assert!(builder.exists());
        std::fs::write(&builder, "edited by the engineer").unwrap();
        std::fs::remove_file(user.join("agents/docs-writer.md")).unwrap();
        assert!(install_defaults(&user).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(&builder).unwrap(),
            "edited by the engineer"
        );
        assert!(
            !user.join("agents/docs-writer.md").exists(),
            "a deleted default stays deleted"
        );
        // An update that changes the shipped builder: simulate by forgetting the recorded hash.
        let rec = user.join(".shipped.json");
        let mut r: BTreeMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        r.insert("agents/builder.md".into(), "0000000000000000".into());
        r.insert("agents/reviewer.md".into(), fnv(DEFAULT_AGENTS[2].1));
        std::fs::write(&rec, serde_json::to_string(&r).unwrap()).unwrap();
        let news = install_defaults(&user).unwrap();
        assert_eq!(news, ["agents/builder.md"]);
        assert!(user.join("agents/builder.md.new").exists());
        assert_eq!(
            std::fs::read_to_string(&builder).unwrap(),
            "edited by the engineer"
        );
        let _ = std::fs::remove_dir_all(&user);
    }
}
