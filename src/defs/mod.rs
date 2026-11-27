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
    (
        "assistant",
        include_str!("../../assets/agents/assistant.md"),
    ),
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
    // 018 (contracts/pipeline.md "The defaults"): builders over [P] tasks.
    (
        "parallel",
        include_str!("../../assets/pipelines/parallel.toml"),
    ),
];

/// Files read from one place: (name, file shown in problems, text).
type Files = Vec<(String, String, String)>;
/// The defaults of one kind: (folder, extension, shipped files).
type Shipped<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// The runtimes Fallow knows in this version (contracts/runtime.md).
pub const RUNTIMES: &[&str] = &["claude", "fake"];

/// 017: the runtimes Fallow knows, each with how it names its models (contracts/
/// runtime-model-naming.md), as definitions are checked against them.
pub fn runtimes() -> Vec<(&'static str, crate::agent::ModelNaming)> {
    RUNTIMES
        .iter()
        .filter_map(|n| crate::agent::registry::model_naming(n).map(|m| (*n, m)))
        .collect()
}

/// 017 (the template, decided for the engineer): the runtimes a new agent can use on this
/// machine, those [`configured`](crate::agent::registry::configured) in `settings`: the test
/// runtime `fake` only with an `[agents.fake]` profile. `claude` when none is.
pub fn usable_runtimes(
    settings: &crate::config::Settings,
) -> Vec<(&'static str, crate::agent::ModelNaming)> {
    let all = runtimes();
    let usable: Vec<_> = all
        .iter()
        .filter(|(n, _)| crate::agent::registry::configured(n, settings))
        .copied()
        .collect();
    if usable.is_empty() {
        all.into_iter().filter(|(n, _)| *n == "claude").collect()
    } else {
        usable
    }
}

/// What a library holds after loading: the definitions in force, and the files that could not be
/// used, with why.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Library {
    pub agents: BTreeMap<String, AgentDef>,
    pub pipelines: BTreeMap<String, Pipeline>,
    pub problems: Vec<String>,
    /// Defaults an update changed that the engineer had edited: written as `<name>.new`.
    pub updates: Vec<String>,
    /// 017 (research R-3): each agent file that could not be used, in load order.
    pub unusable: Vec<Unusable>,
}

/// 017: an agent file Fallow could not use: its text, its problems at their lines, and which
/// definition of that name is in force meanwhile (`None`: none is).
#[derive(Debug, Clone, Serialize)]
pub struct Unusable {
    pub name: String,
    pub file: String,
    pub source: Source,
    pub text: String,
    pub problems: Vec<crate::speckit::Problem>,
    pub in_force: Option<Source>,
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
        let runtimes = runtimes();
        for (source, files) in sources {
            for (stem, file, text) in files {
                match agent::parse(&file, &stem, &text, source, &runtimes) {
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
                    Err(ps) => {
                        lib.problems.extend(ps.iter().map(|p| p.to_string()));
                        lib.unusable.push(Unusable {
                            name: stem,
                            file,
                            source,
                            text,
                            problems: ps,
                            in_force: None,
                        });
                    }
                }
            }
        }
        for u in &mut lib.unusable {
            u.in_force = lib.agents.get(&u.name).map(|d| d.source);
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
        // The agents as loaded, to tell a pipeline's step why an agent cannot be used (FR-019).
        let agents = Library {
            agents: lib.agents.clone(),
            unusable: lib.unusable.clone(),
            ..Library::default()
        };
        let kind_of = |n: &str| agents.kind_of(n);
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

    /// 017 (FR-019, research R-3): why the agent `name` cannot be used: there is no file of
    /// that name, or its file (the latest place's) has problems, the first of which is given.
    pub fn why_not(&self, name: &str) -> String {
        match self.unusable.iter().rev().find(|u| u.name == name) {
            None => format!("there is no agent named {name:?}"),
            Some(u) => {
                let first = u
                    .problems
                    .first()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| u.file.clone());
                let more = match u.problems.len() {
                    0 | 1 => String::new(),
                    n => format!(" (and {} more)", n - 1),
                };
                format!("{name} cannot be used: {first}{more}")
            }
        }
    }

    /// The kind of the agent `name` in force, or why it cannot be used ([`Library::why_not`]).
    pub fn kind_of(&self, name: &str) -> Result<Kind, String> {
        self.agents
            .get(name)
            .map(|a| a.kind)
            .ok_or_else(|| self.why_not(name))
    }
}

/// A 64-bit FNV-1a hash, to tell whether a shipped file changed: what `.shipped.json` records
/// for each shipped file (017 T030: public, so a test can write a record as an earlier version did).
pub fn fingerprint(s: &str) -> String {
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
            let shipped = fingerprint(text);
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
                    let untouched = last.as_deref() == Some(fingerprint(&current).as_str());
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
    fn a_new_agent_is_offered_the_runtimes_set_up_here() {
        let names = |s: &crate::config::Settings| -> Vec<&str> {
            usable_runtimes(s).iter().map(|(n, _)| *n).collect()
        };
        let parse = |text: &str| {
            let mut n = Vec::new();
            let s = crate::config::Settings::parse("settings.toml", text, &mut n);
            assert!(n.is_empty(), "{n:?}");
            s
        };
        // No `[agents.fake]`: the test runtime is not offered.
        assert_eq!(names(&crate::config::Settings::default()), ["claude"]);
        assert_eq!(names(&parse("")), ["claude"]);
        // With its profile, it is.
        assert_eq!(
            names(&parse("[agents.fake]\ncommand = \"fake-agent\"\n")),
            ["claude", "fake"]
        );
        // None set up (no runtime has a profile): still claude.
        let mut none = crate::config::Settings::default();
        none.agents.clear();
        assert_eq!(names(&none), ["claude"]);
    }

    #[test]
    fn the_built_in_library_has_the_defaults() {
        let lib = Library::load(None, None);
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert_eq!(lib.agents.len(), 7);
        assert_eq!(lib.pipelines.len(), 4);
        assert!(lib.pipelines["standard"].verifying);
        assert!(!lib.pipelines["solo"].verifying);
        // 018: the parallel default, verifying, with three builders.
        assert!(lib.pipelines["parallel"].verifying);
        assert_eq!(lib.pipelines["parallel"].caps.builders, 3);
    }

    #[test]
    fn a_project_overrides_by_name_and_a_broken_file_is_reported() {
        let user = tmp("user");
        let proj = tmp("proj");
        install_defaults(&user).unwrap();
        std::fs::create_dir_all(proj.join(".fallow/agents")).unwrap();
        std::fs::write(
            proj.join(".fallow/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: stricter\nruntime: claude\nmodel: claude-opus-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\nCheck harder.\n",
        )
        .unwrap();
        std::fs::write(proj.join(".fallow/agents/broken.md"), "no frontmatter\n").unwrap();
        std::fs::write(
            user.join("agents/ui-checker.md"),
            "---\nname: ui-checker\ndescription: walks the stories\nruntime: claude\nmodel: claude-haiku-4-5\nkind: checker\nmay_run: []\nreturns: verdict\n---\nOpen it.\n",
        )
        .unwrap();
        let lib = Library::load(Some(&user), Some(&proj));
        assert_eq!(lib.agents["reviewer"].source, Source::Project);
        assert_eq!(lib.agents["reviewer"].model, "claude-opus-5-5");
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
    fn a_file_that_cannot_be_used_is_kept_with_what_is_in_force() {
        let user = tmp("unusable-user");
        let proj = tmp("unusable-proj");
        std::fs::create_dir_all(user.join("agents")).unwrap();
        std::fs::create_dir_all(proj.join(".fallow/agents")).unwrap();
        let reviewer = DEFAULT_AGENTS
            .iter()
            .find(|(n, _)| *n == "reviewer")
            .unwrap()
            .1;
        let broken = reviewer.replace("kind: checker", "kind: judge");
        assert_ne!(broken, reviewer);
        // A broken copy of a shipped agent: the default stays in force.
        std::fs::write(user.join("agents/reviewer.md"), &broken).unwrap();
        // A broken agent of the engineer's own: nothing is in force.
        std::fs::write(
            user.join("agents/ui-checker.md"),
            broken.replace("name: reviewer", "name: ui-checker"),
        )
        .unwrap();
        // A broken project copy over a valid copy of the engineer's: theirs is in force.
        let builder = DEFAULT_AGENTS
            .iter()
            .find(|(n, _)| *n == "builder")
            .unwrap()
            .1;
        let mine = builder.replace("description: ", "description: mine, ");
        assert_ne!(mine, builder);
        std::fs::write(user.join("agents/builder.md"), &mine).unwrap();
        std::fs::write(
            proj.join(".fallow/agents/builder.md"),
            builder.replace("kind: maker", "kind: judge"),
        )
        .unwrap();
        let lib = Library::load(Some(&user), Some(&proj));
        let u = |n: &str| lib.unusable.iter().find(|u| u.name == n).unwrap();
        assert_eq!(u("reviewer").in_force, Some(Source::Default));
        assert_eq!(u("reviewer").source, Source::User);
        assert_eq!(u("reviewer").text, broken, "its text as it is on disk");
        assert!(u("reviewer").file.ends_with("reviewer.md"));
        assert_eq!(
            u("reviewer").problems[0].line,
            6,
            "{:?}",
            u("reviewer").problems
        );
        assert_eq!(u("ui-checker").in_force, None);
        assert_eq!(u("builder").in_force, Some(Source::User));
        assert_eq!(u("builder").source, Source::Project);
        assert_eq!(lib.agents["reviewer"].source, Source::Default);
        // Why each name cannot be used.
        let why = lib.why_not("ui-checker");
        assert!(
            why.starts_with("ui-checker cannot be used: ")
                && why.contains("ui-checker.md:6: ")
                && why.contains("judge"),
            "{why}"
        );
        assert_eq!(lib.why_not("nobody"), "there is no agent named \"nobody\"");
        assert!(lib.kind_of("ui-checker").is_err());
        assert_eq!(lib.kind_of("reviewer"), Ok(Kind::Checker));
        let two = DEFAULT_AGENTS[0]
            .1
            .replace("kind: lead", "kind: judge")
            .replace("runtime: claude", "runtime: gpt")
            .replace("name: lead", "name: two");
        std::fs::write(user.join("agents/two.md"), two).unwrap();
        let lib = Library::load(Some(&user), None);
        assert!(
            lib.why_not("two").contains("(and 1 more)"),
            "{}",
            lib.why_not("two")
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
        r.insert(
            "agents/reviewer.md".into(),
            fingerprint(DEFAULT_AGENTS[2].1),
        );
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
