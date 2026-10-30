//! Reading `tasks.md` (007 contracts/spec-kit-files.md): phases, tasks, and which phases are a
//! user story's; and ticking an accepted task.

use crate::run::events::{PhaseSpec, TaskSpec};

use super::Problem;

/// Reads `text` (the file `file`). Problems name the line.
pub fn read(file: &str, text: &str) -> Result<Vec<PhaseSpec>, Vec<Problem>> {
    let mut phases: Vec<(usize, PhaseSpec)> = Vec::new();
    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut in_task = false;
    for (i, line) in text.lines().enumerate() {
        let at = i + 1;
        if let Some(rest) = line.strip_prefix("## Phase ") {
            in_task = false;
            let Some((n, title)) = rest.split_once(':') else {
                problems.push(Problem::new(
                    file,
                    at,
                    "a phase heading is `## Phase <n>: <title>`",
                ));
                continue;
            };
            let label = n.trim();
            if !label.starts_with(|c: char| c.is_ascii_digit()) {
                problems.push(Problem::new(
                    file,
                    at,
                    format!("phase number {label:?} is not a number"),
                ));
                continue;
            }
            let n = phases.iter().filter(|(_, p)| p.n != 0).count() as u32 + 1;
            phases.push((
                at,
                PhaseSpec {
                    n,
                    label: label.to_string(),
                    title: title.trim().to_string(),
                    story: None,
                    tasks: Vec::new(),
                },
            ));
            continue;
        }
        if line.starts_with("## ") || line.starts_with("# ") {
            // Another section (dependencies, strategy): no longer inside a phase.
            if !phases.is_empty() {
                phases.push((
                    at,
                    PhaseSpec {
                        n: 0,
                        label: String::new(),
                        title: String::new(),
                        story: None,
                        tasks: Vec::new(),
                    },
                ));
            }
            in_task = false;
            continue;
        }
        if let Some(task) = task_line(line) {
            let (id, parallel, story, text) = task;
            let Some((_, phase)) = phases.last_mut() else {
                problems.push(Problem::new(
                    file,
                    at,
                    format!("task {id} is not under a `## Phase` heading"),
                ));
                continue;
            };
            if phase.n == 0 {
                // A checkbox in a later section (dependencies, notes) is not a task.
                continue;
            }
            if seen.contains(&id) {
                problems.push(Problem::new(
                    file,
                    at,
                    format!("task id {id} is used twice"),
                ));
            }
            seen.push(id.clone());
            phase.tasks.push(TaskSpec {
                id,
                story,
                parallel,
                text,
            });
            in_task = true;
            continue;
        }
        // A task's continuation lines (indented sub-items) belong to its text.
        if in_task && (line.starts_with("  ") || line.starts_with('\t')) && !line.trim().is_empty()
        {
            if let Some((_, p)) = phases.last_mut() {
                if let Some(t) = p.tasks.last_mut() {
                    t.text.push('\n');
                    t.text.push_str(line.trim_end());
                }
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        in_task = false;
    }
    let mut out = Vec::new();
    for (at, mut p) in phases {
        if p.n == 0 {
            continue;
        }
        if p.tasks.is_empty() {
            problems.push(Problem::new(
                file,
                at,
                format!("phase {} has no tasks", p.n),
            ));
            continue;
        }
        p.story = story_of(&p);
        out.push(p);
    }
    if out.is_empty() && problems.is_empty() {
        problems.push(Problem::new(
            file,
            1,
            "no `## Phase <n>: <title>` with tasks",
        ));
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// `- [ ] T014 [P] [US1] text` (or `[X]`, `[x]`): the id, the parallel mark, the story, the text.
fn task_line(line: &str) -> Option<(String, bool, Option<String>, String)> {
    let rest = line
        .strip_prefix("- [ ] ")
        .or_else(|| line.strip_prefix("- [X] "))
        .or_else(|| line.strip_prefix("- [x] "))?;
    let (id, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
    if !(id.len() >= 2 && id.starts_with('T') && id[1..].chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let mut parallel = false;
    let mut story = None;
    loop {
        let r = rest.trim_start();
        if let Some(after) = r.strip_prefix("[P]") {
            parallel = true;
            rest = after;
        } else if let Some(after) = r.strip_prefix("[US") {
            let Some((n, after)) = after.split_once(']') else {
                break;
            };
            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                story = Some(format!("US{n}"));
                rest = after;
            } else {
                break;
            }
        } else {
            rest = r;
            break;
        }
    }
    Some((id.to_string(), parallel, story, rest.trim().to_string()))
}

/// A phase is a user story's when its tasks carry the same `[USn]`, or (with no labels) its
/// title names one `User Story n`. A phase whose tasks carry different stories is no single
/// story's (Phase 2 review); the loop places each story's checkpoint after its last task.
fn story_of(p: &PhaseSpec) -> Option<String> {
    let labels: Vec<&String> = p.tasks.iter().filter_map(|t| t.story.as_ref()).collect();
    if let Some(first) = labels.first() {
        return labels.iter().all(|l| l == first).then(|| (*first).clone());
    }
    if p.title.to_lowercase().matches("user story ").count() != 1 {
        return None;
    }
    let lower = p.title.to_lowercase();
    let i = lower.find("user story ")?;
    let n: String = lower[i + "user story ".len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!n.is_empty()).then(|| format!("US{n}"))
}

/// `text` with task `id` ticked (`- [ ]` becomes `- [X]`); `None` when the task is not there or
/// already ticked.
pub fn tick(text: &str, id: &str) -> Option<String> {
    let mut done = false;
    let out: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| {
            if !done {
                if let Some(rest) = line.strip_prefix("- [ ] ") {
                    let first = rest.split_whitespace().next().unwrap_or_default();
                    if first == id {
                        done = true;
                        return format!("- [X] {rest}");
                    }
                }
            }
            line.to_string()
        })
        .collect();
    done.then(|| out.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKS: &str = "\
# Tasks: Cart

## Phase 1: Setup

- [ ] T001 Create shop/cart.py
- [X] T002 [P] Add tests/test_cart.py
  - with a fixture
  - and a must-fail case

## Phase 3: User Story 1 - Add to cart (Priority: P1)

- [ ] T010 [US1] Cart model in shop/cart.py
- [ ] T011 [P] [US1] Tests for add

**Checkpoint**: US1 works on its own.

## Phase 4: Polish

- [ ] T020 README

## Dependencies

- [ ] T999 not a task: outside the phases
";

    #[test]
    fn phases_tasks_and_story_phases_are_read() {
        let p = read("tasks.md", TASKS).unwrap();
        assert_eq!(p.len(), 3, "{p:#?}");
        assert_eq!((p[0].n, p[0].title.as_str()), (1, "Setup"));
        assert_eq!(p[0].story, None);
        assert_eq!(p[0].tasks[1].id, "T002");
        assert!(p[0].tasks[1].parallel);
        assert!(
            p[0].tasks[1].text.contains("must-fail case"),
            "continuation lines kept"
        );
        assert_eq!(p[1].story.as_deref(), Some("US1"));
        assert_eq!(p[1].tasks[1].story.as_deref(), Some("US1"));
        assert_eq!(p[1].tasks[1].text, "Tests for add");
        assert_eq!(p[2].tasks.len(), 1, "T999 is outside the phases");
    }

    #[test]
    fn broken_files_are_refused_with_their_lines() {
        let e = read("tasks.md", "- [ ] T001 before any phase\n## Phase 1: A\n").unwrap_err();
        let s: Vec<String> = e.iter().map(|p| p.to_string()).collect();
        assert!(
            s.iter()
                .any(|x| x.starts_with("tasks.md:1:") && x.contains("not under")),
            "{s:?}"
        );
        assert!(
            s.iter()
                .any(|x| x.starts_with("tasks.md:2:") && x.contains("no tasks")),
            "{s:?}"
        );
        let e = read("tasks.md", "## Phase 1: A\n- [ ] T001 a\n- [ ] T001 b\n").unwrap_err();
        assert!(e[0].to_string().contains("used twice"));
        let e = read("tasks.md", "## Phase x: A\n- [ ] T001 a\n").unwrap_err();
        assert!(e[0].to_string().contains("not a number"));
    }

    #[test]
    fn lettered_phases_and_mixed_stories() {
        let p = read(
            "t",
            "## Phase 3a: A\n- [ ] T001 [US1] a\n## Phase 3b: B\n- [ ] T002 [US1] b\n- [ ] T003 [US3] c\n",
        )
        .unwrap();
        assert_eq!((p[0].n, p[0].label.as_str()), (1, "3a"));
        assert_eq!((p[1].n, p[1].label.as_str()), (2, "3b"));
        assert_eq!(p[0].story.as_deref(), Some("US1"));
        assert_eq!(p[1].story, None, "two stories: no single story");
        for f in [
            "001-llm-aware-tui-editor",
            "002-agent-sessions",
            "005-adaptive-delegation",
        ] {
            let path = format!("{}/specs/{f}/tasks.md", env!("CARGO_MANIFEST_DIR"));
            if let Ok(text) = std::fs::read_to_string(&path) {
                assert!(
                    read("tasks.md", &text).is_ok(),
                    "{f}: {:?}",
                    read("tasks.md", &text).err()
                );
            }
        }
    }

    #[test]
    fn a_title_naming_a_user_story_makes_a_story_phase() {
        let p = read(
            "t",
            "## Phase 2: User Story 2 - Receipt\n- [ ] T005 Receipt\n",
        )
        .unwrap();
        assert_eq!(p[0].story.as_deref(), Some("US2"));
    }

    #[test]
    fn this_repositorys_own_tasks_file_reads() {
        let p = read(
            "tasks.md",
            include_str!("../../specs/007-fallow-studio/tasks.md"),
        )
        .unwrap();
        assert!(p.len() >= 10);
        assert!(p.iter().any(|x| x.story.as_deref() == Some("US1")));
    }

    #[test]
    fn ticking_marks_one_task() {
        let t = tick(TASKS, "T010").unwrap();
        assert!(t.contains("- [X] T010 [US1] Cart model"));
        assert!(t.contains("- [ ] T011"));
        assert!(tick(&t, "T010").is_none(), "already ticked");
        assert!(tick(TASKS, "T404").is_none());
    }
}
