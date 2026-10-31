//! Unlocking and locking again (007 research S-17, FR-009): the loop pauses, and after the new
//! lock the user stories the change affects are reopened and verified again.

use std::collections::BTreeSet;
use std::path::Path;

use super::events::PhaseSpec;
use crate::speckit;
use crate::verify::git;

/// The stories a change of the locked documents affects, and why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Affected {
    pub stories: Vec<String>,
    pub why: Vec<String>,
}

/// A document of the feature's folder at the two locks (`None` where it does not exist).
pub struct Doc {
    /// The path inside the feature's folder, such as `spec.md` or `contracts/api.md`.
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// The other side of a comparison: a lock commit, or the working copy before the new lock.
pub enum Side<'a> {
    Commit(&'a str),
    WorkingCopy(&'a Path),
}

/// The feature's documents at `old` (a lock commit) and at `new`, without the handovers and
/// checklists, which are not the spec.
pub fn docs(repo: &Path, feature_dir: &str, old: &str, new: Side) -> Result<Vec<Doc>, String> {
    let prefix = format!("{}/", feature_dir.trim_end_matches('/'));
    let keep = |f: &str| {
        f.strip_prefix(&prefix)
            .filter(|n| !n.starts_with("handover/") && !n.starts_with("checklists/"))
            .map(str::to_string)
    };
    let mut names: BTreeSet<String> = git::files_at(repo, old)?
        .iter()
        .filter_map(|f| keep(f))
        .collect();
    match &new {
        Side::Commit(c) => names.extend(git::files_at(repo, c)?.iter().filter_map(|f| keep(f))),
        Side::WorkingCopy(wd) => {
            let mut stack = vec![wd.join(feature_dir)];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let Ok(rel) = p.strip_prefix(wd) {
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        if let Some(n) = keep(&rel) {
                            names.insert(n);
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for name in names {
        let path = format!("{prefix}{name}");
        let old_text = git::show(repo, old, &path)?;
        let new_text = match &new {
            Side::Commit(c) => git::show(repo, c, &path)?,
            Side::WorkingCopy(wd) => std::fs::read_to_string(wd.join(&path)).ok(),
        };
        let norm = |t: Option<String>| t.map(|t| t.replace("\r\n", "\n"));
        out.push(Doc {
            name,
            old: norm(old_text),
            new: norm(new_text),
        });
    }
    Ok(out)
}

/// Works out the affected stories (research S-17), mechanically so the engineer can predict it:
/// - a story whose section of `spec.md` changed;
/// - a story whose tasks changed in `tasks.md`;
/// - for a change elsewhere (`plan.md`, a contract, the rest of `spec.md`), every story whose
///   tasks name a file the changed lines mention, or every story when none is named.
pub fn affected(docs: &[Doc]) -> Affected {
    let get = |name: &str| docs.iter().find(|d| d.name == name);
    let phases = |text: Option<&String>| -> Vec<PhaseSpec> {
        text.and_then(|t| speckit::tasks::read("tasks.md", t).ok())
            .unwrap_or_default()
    };
    let tasks_doc = get("tasks.md");
    let old_phases = phases(tasks_doc.and_then(|d| d.old.as_ref()));
    let new_phases = phases(tasks_doc.and_then(|d| d.new.as_ref()));
    let mut all: Vec<String> = Vec::new();
    for p in new_phases.iter().chain(&old_phases) {
        for s in p
            .story
            .iter()
            .chain(p.tasks.iter().filter_map(|t| t.story.as_ref()))
        {
            if !all.contains(s) {
                all.push(s.clone());
            }
        }
    }
    let mut out = Affected::default();
    let add = |out: &mut Affected, s: &str, why: String| {
        if !out.stories.iter().any(|x| x == s) {
            out.stories.push(s.to_string());
        }
        out.why.push(why);
    };
    // The text a story's tasks give, for each story.
    let story_tasks = |ps: &[PhaseSpec], story: &str| -> Vec<String> {
        ps.iter()
            .flat_map(|p| {
                p.tasks
                    .iter()
                    .filter(move |t| {
                        t.story.as_deref() == Some(story)
                            || (t.story.is_none() && p.story.as_deref() == Some(story))
                    })
                    .map(|t| format!("{} {}", t.id, t.text))
            })
            .collect()
    };
    let mut elsewhere: Vec<(String, Vec<String>)> = Vec::new();
    for d in docs {
        if d.old == d.new {
            continue;
        }
        match d.name.as_str() {
            "spec.md" => {
                let old = sections(d.old.as_deref().unwrap_or_default());
                let new = sections(d.new.as_deref().unwrap_or_default());
                let ids: BTreeSet<&String> = old.0.keys().chain(new.0.keys()).collect();
                for id in ids {
                    if old.0.get(id) != new.0.get(id) {
                        add(
                            &mut out,
                            id,
                            format!("{id}: its section of spec.md changed"),
                        );
                    }
                }
                if old.1 != new.1 {
                    elsewhere.push(("spec.md".into(), changed_lines(&old.1, &new.1)));
                }
            }
            "tasks.md" => {
                for s in &all {
                    if story_tasks(&old_phases, s) != story_tasks(&new_phases, s) {
                        add(&mut out, s, format!("{s}: its tasks changed in tasks.md"));
                    }
                }
                // Tasks outside the stories, such as the setup's.
                let rest = |ps: &[PhaseSpec]| -> Vec<String> {
                    ps.iter()
                        .filter(|p| p.story.is_none())
                        .flat_map(|p| p.tasks.iter().filter(|t| t.story.is_none()))
                        .map(|t| format!("{} {}", t.id, t.text))
                        .collect()
                };
                let (a, b) = (rest(&old_phases), rest(&new_phases));
                if a != b {
                    elsewhere.push((
                        "tasks.md".into(),
                        changed_lines(&a.join("\n"), &b.join("\n")),
                    ));
                }
            }
            name => elsewhere.push((
                name.to_string(),
                changed_lines(
                    d.old.as_deref().unwrap_or_default(),
                    d.new.as_deref().unwrap_or_default(),
                ),
            )),
        }
    }
    for (name, lines) in elsewhere {
        let named: BTreeSet<String> = lines.iter().flat_map(|l| paths_in(l)).collect();
        let hit: Vec<&String> = all
            .iter()
            .filter(|s| {
                let text = story_tasks(&new_phases, s).join("\n");
                named.iter().any(|p| text.contains(p.as_str()))
            })
            .collect();
        if named.is_empty() || hit.is_empty() {
            for s in &all {
                add(
                    &mut out,
                    s,
                    format!("{s}: {name} changed and names no file of a story's tasks"),
                );
            }
        } else {
            for s in hit {
                add(
                    &mut out,
                    s,
                    format!("{s}: {name} changed a file its tasks name"),
                );
            }
        }
    }
    // In the order of the stories.
    out.stories
        .sort_by_key(|s| all.iter().position(|x| x == s).unwrap_or(usize::MAX));
    out
}

/// `spec.md` cut into its stories' sections (`USn` to the text) and the rest.
fn sections(text: &str) -> (std::collections::BTreeMap<String, String>, String) {
    let mut stories = std::collections::BTreeMap::new();
    let mut rest = String::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("### User Story") {
            current = line
                .trim_start_matches("### User Story")
                .split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
                .map(|n| format!("US{n}"));
        } else if line.starts_with("## ") || (line.starts_with("### ") && current.is_some()) {
            current = None;
        }
        match &current {
            Some(id) => {
                let s: &mut String = stories.entry(id.clone()).or_default();
                s.push_str(line.trim_end());
                s.push('\n');
            }
            None => {
                rest.push_str(line.trim_end());
                rest.push('\n');
            }
        }
    }
    (stories, rest)
}

/// The lines in one text and not the other.
fn changed_lines(old: &str, new: &str) -> Vec<String> {
    let a: Vec<&str> = old.lines().map(str::trim_end).collect();
    let b: Vec<&str> = new.lines().map(str::trim_end).collect();
    a.iter()
        .filter(|l| !b.contains(l))
        .chain(b.iter().filter(|l| !a.contains(l)))
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// The file paths a line mentions: words with a `/` or a file extension.
fn paths_in(line: &str) -> Vec<String> {
    line.split(|c: char| c.is_whitespace() || "`'\"(),;:[]<>*".contains(c))
        .map(|w| w.trim_end_matches('.'))
        .filter(|w| {
            let ext = w.rsplit_once('.').map(|(a, e)| {
                !a.is_empty()
                    && (1..=5).contains(&e.len())
                    && e.chars().all(|c| c.is_ascii_alphanumeric())
            });
            (w.contains('/') && w.len() > 2) || ext == Some(true)
        })
        .filter(|w| !w.starts_with("http"))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "# Spec\n\n## User Scenarios\n\n### User Story 1 - Add (Priority: P1)\n\nAdd two numbers.\n\n### User Story 2 - Subtract (Priority: P2)\n\nSubtract.\n\n## Requirements\n\n- FR-001: ints only.\n";
    const TASKS: &str = "# Tasks\n\n## Phase 1: Setup\n\n- [ ] T001 Create calc/__init__.py\n\n## Phase 2: User Story 1 - Add\n\n- [ ] T002 [US1] add() in calc/__init__.py\n\n## Phase 3: User Story 2 - Subtract\n\n- [ ] T003 [US2] sub() in calc/sub.py\n";

    fn doc(name: &str, old: &str, new: &str) -> Doc {
        Doc {
            name: name.into(),
            old: Some(old.into()),
            new: Some(new.into()),
        }
    }

    #[test]
    fn a_story_section_change_affects_that_story_only() {
        let a = affected(&[
            doc(
                "spec.md",
                SPEC,
                &SPEC.replace("Subtract.", "Subtract, ints only."),
            ),
            doc("tasks.md", TASKS, TASKS),
        ]);
        assert_eq!(a.stories, vec!["US2"]);
    }

    #[test]
    fn a_task_change_affects_its_story() {
        let a = affected(&[doc(
            "tasks.md",
            TASKS,
            &TASKS.replace("add() in", "add() with checks in"),
        )]);
        assert_eq!(a.stories, vec!["US1"]);
    }

    #[test]
    fn a_plan_change_affects_the_stories_whose_tasks_name_its_files() {
        let a = affected(&[
            doc("tasks.md", TASKS, TASKS),
            doc(
                "plan.md",
                "# Plan\n",
                "# Plan\n\n`calc/sub.py` takes floats too.\n",
            ),
        ]);
        assert_eq!(a.stories, vec!["US2"]);
        let a = affected(&[
            doc("tasks.md", TASKS, TASKS),
            doc("plan.md", "# Plan\n", "# Plan\n\nUse Python 3.12.\n"),
        ]);
        assert_eq!(a.stories, vec!["US1", "US2"], "nothing named: every story");
    }

    #[test]
    fn nothing_changed_affects_nothing() {
        assert_eq!(
            affected(&[doc("spec.md", SPEC, SPEC), doc("tasks.md", TASKS, TASKS)]),
            Affected::default()
        );
    }
}
