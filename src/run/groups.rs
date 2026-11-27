//! 018: builders over groups of `[P]` tasks (research R-5, R-6, R-11). A group is a run of
//! consecutive `[P]` tasks in one phase of the locked `tasks.md`, at least two, in a pipeline
//! whose `[caps] builders` is more than 1; each of its tasks is built in its own working copy and
//! merged into the build branch in `tasks.md` order after the touched-files check (R-6). The lock
//! statement (R-11) says at the lock what the builders will do.

use std::path::{Path, PathBuf};

use super::engine::Run;
use super::events::{Event, PhaseSpec};
use super::state::{GroupState, LoopState};
use crate::speckit;
use crate::verify::git;

/// The groups of a phase (research R-5): each run of consecutive `[P]` tasks of at least two,
/// in `tasks.md` order. None with one builder: tasks then run one after the other as in 0.4.0.
pub fn groups(phase: &PhaseSpec, builders: u32) -> Vec<Vec<String>> {
    if builders <= 1 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut run: Vec<String> = Vec::new();
    for t in &phase.tasks {
        if t.parallel {
            run.push(t.id.clone());
            continue;
        }
        if run.len() >= 2 {
            out.push(std::mem::take(&mut run));
        }
        run.clear();
    }
    if run.len() >= 2 {
        out.push(run);
    }
    out
}

/// The files both a group task (`mine`) and the build branch (`theirs`) changed since the
/// task's base, sorted, without the feature's `tasks.md` (Fallow's own ticks; research R-6).
pub fn overlap(mine: &[String], theirs: &[String], tasks_md: &str) -> Vec<String> {
    let mut v: Vec<String> = mine
        .iter()
        .filter(|f| f.as_str() != tasks_md && theirs.contains(f))
        .cloned()
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The part a group's gate runs and repair are counted under: `group-<first>-<last>`.
pub fn part(tasks: &[String]) -> String {
    format!(
        "group-{}-{}",
        tasks.first().map_or("", String::as_str),
        tasks.last().map_or("", String::as_str)
    )
}

/// A group task's working copy (`worktrees/task-<id>`).
pub fn worktree(run: &Run, task: &str) -> PathBuf {
    run.worktrees().join(format!("task-{task}"))
}

/// A group task's branch (`<build_branch>-<id>`).
pub fn branch(l: &LoopState, task: &str) -> String {
    format!("{}-{task}", l.build_branch)
}

/// The group in progress holding `task`: started and not done.
pub fn active_of<'a>(l: &'a LoopState, task: &str) -> Option<&'a GroupState> {
    l.group_of(task).filter(|g| !g.done)
}

/// The group in progress, if any (one at a time: the task after a group waits for it).
pub fn active(l: &LoopState) -> Option<&GroupState> {
    l.groups.iter().rev().find(|g| !g.done)
}

/// The task of a group in progress a step of the loop works on or checks: its own task, or the
/// task of its point (`<agent>@<task>-start|end`).
pub fn task_of_step(l: &LoopState, task: Option<&str>, part: Option<&str>) -> Option<String> {
    if let Some(t) = task.filter(|t| active_of(l, t).is_some()) {
        return Some(t.to_string());
    }
    let rest = part?.split_once('@')?.1;
    let t = rest
        .strip_suffix("-start")
        .or_else(|| rest.strip_suffix("-end"))?;
    active_of(l, t).map(|_| t.to_string())
}

/// Makes sure a group task's working copy is there, on its branch: created at `base` the first
/// time, re-created on its branch when its folder went missing (a restart; research R-10).
pub fn ensure_worktree(
    run: &Run,
    l: &LoopState,
    task: &str,
    base: &str,
) -> Result<PathBuf, String> {
    let wt = worktree(run, task);
    if wt.join(".git").exists() {
        return Ok(wt);
    }
    let repo = run.state.repo.clone();
    let b = branch(l, task);
    git::worktree_prune(&repo);
    if git::rev_exists(&repo, &format!("refs/heads/{b}")) {
        git::worktree_add(&repo, &wt, Some(&b), base)?;
    } else {
        git::worktree_add_new(&repo, &wt, &b, base)?;
    }
    Ok(wt)
}

/// Starts a group (research R-5): `GroupStarted` with the build head as its base, and each
/// task's working copy on its own branch there.
pub fn start_group(
    run: &mut Run,
    l: &LoopState,
    phase: u32,
    tasks: &[String],
    build_head: &str,
) -> Result<(), String> {
    for t in tasks {
        // A branch left by an earlier start (a new lock) starts again at the new base.
        let wt = worktree(run, t);
        let _ = git::worktree_remove(&run.state.repo, &wt);
        let _ = std::fs::remove_dir_all(&wt);
        git::delete_branch(&run.state.repo, &branch(l, t))?;
        ensure_worktree(run, l, t, build_head)?;
    }
    run.append(Event::GroupStarted {
        phase,
        tasks: tasks.to_vec(),
        base: build_head.to_string(),
    })
}

/// What `try_merge` did with an accepted group task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    /// Merged into the build branch and ticked (`TaskMerged` appended).
    Merged,
    /// Changed files the build branch changed too: held back (`TaskOverlap` appended); its
    /// builder repairs it on the working commit.
    Overlap,
}

/// The commit a group task's changes are compared from: the group's base, or the build head
/// its working commit merged in after its latest overlap (that commit's second parent).
pub fn task_base(run: &Run, l: &LoopState, task: &str) -> Option<String> {
    let g = l.group_of(task)?;
    let gt = g.tasks.iter().find(|t| t.id == task)?;
    match &gt.working {
        Some(w) => git::parents(&run.state.repo, w)
            .ok()
            .and_then(|p| p.get(1).cloned()),
        None => Some(g.base.clone()),
    }
}

/// Merges an accepted group task into the build branch (research R-6), or holds it back when it
/// changed a file the build branch changed since its base. `head` is the task's accepted commit.
/// Idempotent: a task already in the build branch (merged just before a crash) is only recorded.
pub fn try_merge(
    run: &mut Run,
    l: &LoopState,
    build_wt: &Path,
    task: &str,
    head: &str,
) -> Result<Merge, String> {
    let repo = run.state.repo.clone();
    let tasks_md = format!("{}/tasks.md", l.dir());
    let build_head = git::head(build_wt)?;
    if !git::is_ancestor(&repo, head, &build_head) {
        let base = task_base(run, l, task).ok_or_else(|| format!("{task} is not in a group"))?;
        let mine = git::files_between(&repo, &base, head)?;
        let theirs = git::files_between(&repo, &base, &build_head)?;
        let files = overlap(&mine, &theirs, &tasks_md);
        if !files.is_empty() {
            // The tasks of the group merged before it that changed those files: each merge's
            // own side, from where it met the build branch.
            let merges: Vec<(String, Vec<String>)> =
                git::first_parent_commits(&repo, &base, &build_head)
                    .into_iter()
                    .filter_map(|c| {
                        let s = git::subject(&repo, &c).ok()?;
                        let t = s
                            .strip_prefix("Fallow: merge ")?
                            .strip_suffix(" (built alongside)")?
                            .to_string();
                        Some((t, git::parents(&repo, &c).ok()?))
                    })
                    .collect();
            let mut with = Vec::new();
            for (t, parents) in merges {
                let [p0, p1, ..] = parents.as_slice() else {
                    continue;
                };
                let met = git::merge_base(&repo, p0, p1).unwrap_or_else(|_| base.clone());
                let changed = git::files_between(&repo, &met, p1).unwrap_or_default();
                if files.iter().any(|f| changed.contains(f)) && !with.contains(&t) {
                    with.push(t);
                }
            }
            // The working commit (as 011's update): both sides merged, with markers where they
            // conflict; the task's branch and working copy move there.
            let mt = git::merge_tree(&repo, head, &build_head)?;
            let working = git::commit_tree(
                &repo,
                &mt.tree,
                &[head, &build_head],
                &format!(
                    "Fallow: {task} on top of the build branch (both changed {})",
                    files.join(", ")
                ),
            )?;
            let wt = ensure_worktree(run, l, task, head)?;
            git::reset_hard_nohooks(&wt, &working)?;
            run.append(Event::TaskOverlap {
                task: task.to_string(),
                files,
                with,
                working,
            })?;
            return Ok(Merge::Overlap);
        }
        git::merge_into(
            build_wt,
            head,
            &format!("Fallow: merge {task} (built alongside)"),
        )
        .map_err(|e| format!("cannot merge {task}: {}", e.join(", ")))?;
    }
    // Fallow ticks the task in tasks.md and commits only that file (research R-6).
    let file = build_wt.join(&tasks_md);
    if let Ok(text) = std::fs::read_to_string(&file) {
        if let Some(ticked) = speckit::tasks::tick(&text, task) {
            std::fs::write(&file, ticked).map_err(|e| format!("{}: {e}", file.display()))?;
            git::commit_all(
                build_wt,
                &format!(
                    "Fallow: tick {task} (built alongside)\n\nFallow loop r{}",
                    run.id()
                ),
                Some(&[tasks_md.as_str()]),
            )?;
        }
    }
    let commit = git::head(build_wt)?;
    run.append(Event::TaskMerged {
        task: task.to_string(),
        commit,
    })?;
    Ok(Merge::Merged)
}

/// Removes a done group's working copies and branches: every task is merged.
pub fn clean_up(run: &Run, l: &LoopState, tasks: &[String]) {
    let repo = run.state.repo.clone();
    for t in tasks {
        let _ = git::worktree_remove(&repo, &worktree(run, t));
        let _ = git::delete_branch(&repo, &branch(l, t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::events::TaskSpec;

    fn phase(tasks: &[(&str, bool)]) -> PhaseSpec {
        PhaseSpec {
            n: 2,
            label: "2".into(),
            title: "User Story 1".into(),
            story: Some("US1".into()),
            tasks: tasks
                .iter()
                .map(|(id, p)| TaskSpec {
                    id: id.to_string(),
                    story: Some("US1".into()),
                    parallel: *p,
                    text: format!("task {id}"),
                })
                .collect(),
        }
    }

    fn ids(g: &[Vec<String>]) -> Vec<Vec<&str>> {
        g.iter()
            .map(|x| x.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn groups_are_consecutive_parallel_runs_of_at_least_two() {
        let p = phase(&[
            ("T004", true),
            ("T005", true),
            ("T006", true),
            ("T007", false),
            ("T008", true),
            ("T009", false),
            ("T010", true),
            ("T011", true),
        ]);
        assert_eq!(
            ids(&groups(&p, 3)),
            vec![vec!["T004", "T005", "T006"], vec!["T010", "T011"]]
        );
        // A single [P] task is no group; a plain task between two splits them.
        let p = phase(&[("T004", true), ("T005", false), ("T006", true)]);
        assert!(groups(&p, 3).is_empty());
        // More tasks than builders: still one group (the builders take turns).
        let p = phase(&[
            ("T004", true),
            ("T005", true),
            ("T006", true),
            ("T007", true),
        ]);
        assert_eq!(
            ids(&groups(&p, 2)),
            vec![vec!["T004", "T005", "T006", "T007"]]
        );
    }

    #[test]
    fn one_builder_or_no_parallel_task_makes_no_group() {
        let p = phase(&[("T004", true), ("T005", true), ("T006", true)]);
        assert!(groups(&p, 1).is_empty());
        assert!(groups(&p, 0).is_empty());
        let p = phase(&[("T004", false), ("T005", false)]);
        assert!(groups(&p, 3).is_empty());
    }

    #[test]
    fn groups_never_cross_phases() {
        // Each phase is grouped on its own: a run at the end of one phase and the start of the
        // next are two runs of one task each, so no group.
        let a = phase(&[("T004", false), ("T005", true)]);
        let mut b = phase(&[("T006", true), ("T007", false)]);
        b.n = 3;
        assert!(groups(&a, 3).is_empty());
        assert!(groups(&b, 3).is_empty());
    }

    #[test]
    fn overlap_ignores_tasks_md_and_is_sorted() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let tasks = "specs/001-calc/tasks.md";
        assert_eq!(
            overlap(
                &s(&["calc/z.py", tasks, "calc/a.py", "calc/own.py"]),
                &s(&[tasks, "calc/a.py", "calc/z.py", "calc/other.py"]),
                tasks
            ),
            s(&["calc/a.py", "calc/z.py"])
        );
        assert!(overlap(&s(&[tasks]), &s(&[tasks]), tasks).is_empty());
        assert!(overlap(&s(&["a"]), &s(&["b"]), tasks).is_empty());
    }

    #[test]
    fn a_group_part_names_its_first_and_last_task() {
        let t = vec!["T004".to_string(), "T005".into(), "T007".into()];
        assert_eq!(part(&t), "group-T004-T007");
    }
}
