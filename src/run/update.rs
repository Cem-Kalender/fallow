//! 011: bringing a loop up to date with its target (research R-2, R-3): the merge computed by git
//! without a working copy and committed by Fallow; a conflict resolved by the builder on a working
//! commit kept on a branch of the loop's own; nothing ever merged in a working copy, and the
//! target branch and the engineer's folder never touched.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use serde::Serialize;

use super::engine::{Busy, Done, Msg, Run};
use super::events::*;
use super::state::{LoopState, UpdateState};
use crate::verify::git;

/// What an update's computation found (on a thread, research R-2).
#[derive(Debug, Clone)]
pub enum Computed {
    /// The target changed the feature's locked documents, or a protected file conflicts: nothing
    /// merged, the engineer decides (FR-005).
    DocsChanged(Vec<String>),
    /// No conflict: Fallow's two-parent merge commit, ready to move the loop's branch to.
    Clean(String),
    /// Conflicts: the working commit holding them, on its branch, and the paths.
    Conflicted {
        working: String,
        branch: String,
        conflicts: Vec<UpdateConflict>,
    },
}

/// What the target watch read for a loop (research R-1; contracts/web-api.md `target`).
#[derive(Debug, Clone, Serialize)]
pub struct Moved {
    pub moved: bool,
    pub target_commit: String,
    /// The target's first-parent commits since the verified commit, each with the feature whose
    /// merge it is, or `None` (outside Fallow).
    pub commits: Vec<MovedCommit>,
    pub conflicts: Vec<String>,
    pub docs_changed: Vec<String>,
    pub gone: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MovedCommit {
    pub sha: String,
    pub subject: String,
    pub from: Option<String>,
}

/// The paths of the feature's locked documents (its spec directory, without `handover/`).
fn doc_dir(feature: &str) -> String {
    format!("specs/{feature}/")
}

fn is_doc(feature: &str, path: &str) -> bool {
    let dir = doc_dir(feature);
    path.starts_with(&dir) && !path.starts_with(&format!("{dir}handover/"))
}

/// Protected files that are not this feature's documents: changed on both sides so that they
/// conflict, they go to the engineer too (a builder may not edit them).
fn is_other_protected(path: &str) -> bool {
    path == ".specify/memory/constitution.md"
}

/// The feature's documents the target changed since `since` whose content differs from the
/// loop's (research R-2).
fn docs_changed(
    repo: &Path,
    feature: &str,
    since: &str,
    target: &str,
    loop_head: &str,
) -> Result<Vec<String>, String> {
    let changed = git::files_between(repo, since, target)?;
    let mut out = Vec::new();
    for f in changed.into_iter().filter(|f| is_doc(feature, f)) {
        let theirs = git::show(repo, target, &f)?;
        let ours = git::show(repo, loop_head, &f)?;
        if theirs != ours {
            out.push(f);
        }
    }
    Ok(out)
}

/// Computes update `n` (research R-2): the docs check first; then the merge, with the loop's
/// own version of its documents kept; committed when clean, else a working commit on a new
/// branch of the loop's own.
#[allow(clippy::too_many_arguments)]
pub fn compute(
    repo: &Path,
    feature: &str,
    n: u32,
    loop_head: &str,
    target_commit: &str,
    target: &str,
    reconciled: Option<&str>,
    index_file: &Path,
) -> Result<Computed, String> {
    if let Some(why) = git::version_refusal(git::git_version(repo)) {
        return Err(why);
    }
    // Since when the target's changes count: the last reconcile (a "Lock again") when it is on
    // the target's line, else where the two sides last met.
    let since = match reconciled.filter(|r| git::is_ancestor(repo, r, target_commit)) {
        Some(r) => r.to_string(),
        None => git::merge_base(repo, loop_head, target_commit)?,
    };
    let docs = docs_changed(repo, feature, &since, target_commit, loop_head)?;
    if !docs.is_empty() {
        return Ok(Computed::DocsChanged(docs));
    }
    let m = git::merge_tree(repo, loop_head, target_commit)?;
    let protected: Vec<String> = m
        .conflicts
        .iter()
        .filter(|c| is_other_protected(&c.path))
        .map(|c| c.path.clone())
        .collect();
    if !protected.is_empty() {
        return Ok(Computed::DocsChanged(protected));
    }
    // The loop's own version of its documents (the lock and the answers merged since).
    let mut docs_paths: Vec<String> = git::files_at(repo, loop_head)?
        .into_iter()
        .filter(|f| is_doc(feature, f))
        .collect();
    for f in git::files_at(repo, &m.tree)? {
        if is_doc(feature, &f) && !docs_paths.contains(&f) {
            docs_paths.push(f);
        }
    }
    let tree = if docs_paths.is_empty() {
        m.tree.clone()
    } else {
        git::tree_with_paths_from(repo, &m.tree, loop_head, &docs_paths, index_file)?
    };
    let conflicts: Vec<UpdateConflict> = m
        .conflicts
        .iter()
        .filter(|c| !is_doc(feature, &c.path))
        .map(|c| UpdateConflict {
            path: c.path.clone(),
            kind: match c.kind {
                git::ConflictKind::Text => ConflictKindRec::Text,
                git::ConflictKind::NonText => ConflictKindRec::NonText,
            },
        })
        .collect();
    let short = &target_commit[..target_commit.len().min(7)];
    if conflicts.is_empty() {
        let c = git::commit_tree(
            repo,
            &tree,
            &[loop_head, target_commit],
            &format!("Fallow: {feature} brought up to date with {target} at {short}"),
        )?;
        return Ok(Computed::Clean(c));
    }
    let working = git::commit_tree(
        repo,
        &tree,
        &[loop_head],
        &format!("Fallow: update {n} of {feature}, conflicts with {target} at {short} to resolve"),
    )?;
    let branch =
        git::create_numbered_branch(repo, &format!("fallow/{feature}/update-{n}"), &working)?;
    Ok(Computed::Conflicted {
        working,
        branch,
        conflicts,
    })
}

/// The part of update `n`'s steps and gate runs.
pub fn part(n: u32) -> String {
    format!("update-{n}")
}

/// Starts an update of the loop to `target_commit` (research R-2): records it, takes a ready
/// loop back to building (FR-003a), and computes the merge on a thread; its end comes back as
/// `Done::Update`.
pub fn start(
    run: &mut Run,
    tx: Sender<Msg>,
    target_commit: &str,
    remote: bool,
) -> Result<(), String> {
    let l = run
        .state
        .feature_loop
        .as_ref()
        .ok_or("only a feature's loop is brought up to date")?;
    if l.update.is_some() {
        return Err("this loop is being brought up to date already".into());
    }
    if l.unlocked {
        return Err("the feature is unlocked: lock it again first".into());
    }
    if run.busy.is_some() || !run.running.is_empty() {
        return Err("a step, a merge or a publication is running".into());
    }
    let local_ready = run.state.stage == Stage::Ready;
    let published = run.state.stage == Stage::Publishing;
    if !local_ready && !(remote && published) {
        return Err("only a loop ready to merge, or a published one, is brought up to date".into());
    }
    let repo = run.state.repo.clone();
    let loop_head = git::rev(&repo, &format!("refs/heads/{}", l.build_branch))?;
    let n = l.updates + 1;
    let feature = l.feature.clone();
    let target = run.state.target.clone();
    let reconciled = l.reconciled_target.clone();
    run.append(Event::Updated {
        n,
        target: target.clone(),
        target_commit: target_commit.to_string(),
        loop_head: loop_head.clone(),
        remote,
    })?;
    if local_ready {
        run.append(Event::Stage {
            to: Stage::Building,
            why: format!("bringing {feature} up to date with {target}"),
        })?;
    }
    computing(
        run,
        tx,
        n,
        &feature,
        &loop_head,
        target_commit,
        &target,
        reconciled,
    )
}

/// Computes update `n` on a thread.
#[allow(clippy::too_many_arguments)]
fn computing(
    run: &mut Run,
    tx: Sender<Msg>,
    n: u32,
    feature: &str,
    loop_head: &str,
    target_commit: &str,
    target: &str,
    reconciled: Option<String>,
) -> Result<(), String> {
    run.busy = Some(Busy::Update);
    let id = run.id();
    let repo = run.state.repo.clone();
    let index: PathBuf = run.worktrees().join(format!("update-{n}.index"));
    let (feature, loop_head, target_commit, target) = (
        feature.to_string(),
        loop_head.to_string(),
        target_commit.to_string(),
        target.to_string(),
    );
    std::thread::Builder::new()
        .name(format!("update-{id}"))
        .spawn(move || {
            if let Some(d) = index.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let result = compute(
                &repo,
                &feature,
                n,
                &loop_head,
                &target_commit,
                &target,
                reconciled.as_deref(),
                &index,
            );
            let _ = tx.send(Msg::Done {
                run: id,
                done: Done::Update { n, result },
            });
        })
        .map(|_| ())
        .map_err(|e| format!("cannot start a thread: {e}"))
}

/// The end of an update's computation (machine::on_done).
pub fn computed(run: &mut Run, n: u32, result: Result<Computed, String>) -> Result<(), String> {
    let Some(u) = run
        .state
        .feature_loop
        .as_ref()
        .and_then(|l| l.update.clone())
        .filter(|u| u.n == n && !u.merged)
    else {
        return Ok(());
    };
    match result {
        Err(why) => {
            run.append(Event::UpdateDropped {
                n,
                why: why.clone(),
            })?;
            run.append(Event::NeedsEngineer {
                why: format!("The loop could not be brought up to date: {why}"),
                options: vec!["bring up to date".into(), "stop".into()],
            })
        }
        Ok(Computed::DocsChanged(paths)) => {
            run.append(Event::UpdateDocsChanged {
                n,
                paths: paths.clone(),
            })?;
            run.append(Event::NeedsEngineer {
                why: format!(
                    "{} changed this feature's documents or a protected file ({}): nothing was merged. Lock again with the target's version, or stop.",
                    u.target,
                    paths.join(", ")
                ),
                options: vec!["lock again".into(), "stop".into()],
            })
        }
        Ok(Computed::Clean(commit)) => merged(run, &u, &commit),
        Ok(Computed::Conflicted {
            working,
            branch,
            conflicts,
        }) => run.append(Event::UpdateConflicted {
            n,
            working,
            branch,
            conflicts,
        }),
    }
}

/// The loop's branch head when it already is a merge of `u`'s two sides (a restart after the
/// branch moved but before `UpdateMerged` was written, research R-9).
fn already_merged(run: &Run, u: &UpdateState) -> Option<String> {
    let l = run.state.feature_loop.as_ref()?;
    let head = git::rev(&run.state.repo, &format!("refs/heads/{}", l.build_branch)).ok()?;
    let parents = git::parents(&run.state.repo, &head).ok()?;
    (parents == [u.loop_head.clone(), u.target_commit.clone()]).then_some(head)
}

/// Moves the loop's branch to Fallow's merge commit and records it; a merge of the same two
/// sides already there is taken as it is.
fn merged(run: &mut Run, u: &UpdateState, commit: &str) -> Result<(), String> {
    let commit = already_merged(run, u).unwrap_or_else(|| commit.to_string());
    let commit = commit.as_str();
    let l = run.state.feature_loop.as_ref().ok_or("no loop")?;
    let wt = run.worktrees().join("build");
    if wt.join(".git").exists() {
        git::checkout_forward(&wt, &l.build_branch, commit)?;
    } else {
        git::set_branch(&run.state.repo, &l.build_branch, commit)?;
    }
    if let Some(b) = &u.branch {
        let _ = git::delete_branch(&run.state.repo, b);
    }
    run.append(Event::UpdateMerged {
        n: u.n,
        commit: commit.to_string(),
    })
}

/// The repair step of update `u`'s conflicts was accepted at `commit`: Fallow checks what the
/// builder left (no marker, no `~side` file, every non-text conflict accounted for) and makes
/// the two-parent merge commit from the resolved tree; or returns what is still wrong.
pub fn resolved(
    run: &mut Run,
    u: &UpdateState,
    commit: &str,
    result: &serde_json::Value,
) -> Result<Vec<String>, String> {
    let repo = run.state.repo.clone();
    let text: Vec<String> = u
        .conflicts
        .iter()
        .filter(|c| c.kind == ConflictKindRec::Text)
        .map(|c| c.path.clone())
        .collect();
    let all: Vec<String> = u.conflicts.iter().map(|c| c.path.clone()).collect();
    let mut wrong = Vec::new();
    for (p, line) in git::marker_lines(&repo, commit, &text) {
        wrong.push(format!("{p} still holds a conflict marker: {line}"));
    }
    for f in git::side_files(&repo, commit, &all) {
        wrong.push(format!(
            "{f} is a file git added beside a conflict: remove it"
        ));
    }
    let told: Vec<String> = result["resolutions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["path"].as_str().map(|p| p.replace('\\', "/")))
        .collect();
    for c in u
        .conflicts
        .iter()
        .filter(|c| c.kind == ConflictKindRec::NonText)
    {
        if !told.contains(&c.path) {
            wrong.push(format!(
                "{} had a conflict without markers: say how you resolved it in `resolutions`",
                c.path
            ));
        }
    }
    if !wrong.is_empty() {
        return Ok(wrong);
    }
    let tree = git::tree_of(&repo, commit)?;
    let l = run.state.feature_loop.as_ref().ok_or("no loop")?;
    let short = &u.target_commit[..u.target_commit.len().min(7)];
    let merge = git::commit_tree(
        &repo,
        &tree,
        &[&u.loop_head, &u.target_commit],
        &format!(
            "Fallow: {} brought up to date with {} at {short}",
            l.feature, u.target
        ),
    )?;
    merged(run, u, &merge)?;
    Ok(Vec::new())
}

/// The update in progress and not merged yet, if any.
pub fn unmerged(l: &LoopState) -> Option<&UpdateState> {
    l.update.as_ref().filter(|u| !u.merged)
}

/// What the target watch reads for a loop (research R-1): whether the target moved past the
/// verified commit (by ancestry), its new first-parent commits and whose merge each is, whether
/// merging would conflict, and the feature's documents it changed. Touches no working copy,
/// index or ref of the engineer's.
#[allow(clippy::too_many_arguments)]
pub fn watch(
    repo: &Path,
    feature: &str,
    target: &str,
    integrated: &str,
    reconciled: Option<&str>,
    merges: &[(String, String)],
    scratch: &Path,
) -> Result<Moved, String> {
    let Ok(tip) = git::rev(repo, &format!("refs/heads/{target}")) else {
        return Ok(Moved {
            moved: false,
            target_commit: String::new(),
            commits: Vec::new(),
            conflicts: Vec::new(),
            docs_changed: Vec::new(),
            gone: true,
        });
    };
    if git::is_ancestor(repo, &tip, integrated) {
        return Ok(Moved {
            moved: false,
            target_commit: tip,
            commits: Vec::new(),
            conflicts: Vec::new(),
            docs_changed: Vec::new(),
            gone: false,
        });
    }
    let base = git::merge_base(repo, integrated, &tip)?;
    let commits = git::first_parent_commits(repo, &base, &tip)
        .into_iter()
        .map(|sha| {
            let subject = git::subject(repo, &sha).unwrap_or_default();
            let from = merges
                .iter()
                .find(|(c, _)| *c == sha)
                .map(|(_, f)| f.clone());
            MovedCommit { sha, subject, from }
        })
        .collect();
    let since = match reconciled.filter(|r| git::is_ancestor(repo, r, &tip)) {
        Some(r) => r.to_string(),
        None => base,
    };
    let docs = docs_changed(repo, feature, &since, &tip, integrated)?;
    let conflicts = git::would_conflict(repo, integrated, &tip, scratch)?
        .into_iter()
        .filter(|p| !is_doc(feature, p))
        .collect();
    Ok(Moved {
        moved: true,
        target_commit: tip,
        commits,
        conflicts,
        docs_changed: docs,
        gone: false,
    })
}

/// After a restart (research R-9): an update whose merge was being computed is computed again,
/// or recorded when the loop's branch already holds it; a leftover working branch of an update
/// that ended is removed.
pub fn resume(run: &mut Run, tx: Sender<Msg>) -> Result<(), String> {
    let Some(l) = run.state.feature_loop.clone() else {
        return Ok(());
    };
    let Some(u) = l.update.clone() else {
        return Ok(());
    };
    if u.merged {
        // The loop's branch is checked out again in its worktree when a crash left it detached.
        let wt = run.worktrees().join("build");
        if wt.join(".git").exists() && git::branch(&wt).is_none() {
            let head = git::rev(&run.state.repo, &format!("refs/heads/{}", l.build_branch))?;
            git::checkout_forward(&wt, &l.build_branch, &head)?;
        }
        return Ok(());
    }
    if u.working.is_some() || !u.docs_changed.is_empty() {
        return Ok(());
    }
    if let Some(head) = already_merged(run, &u) {
        return run.append(Event::UpdateMerged {
            n: u.n,
            commit: head,
        });
    }
    let reconciled = l.reconciled_target.clone();
    computing(
        run,
        tx,
        u.n,
        &l.feature,
        &u.loop_head,
        &u.target_commit,
        &u.target,
        reconciled,
    )
}

/// Drops update `n` (a pull request merged or closed, the loop stopped, locked again): recorded,
/// its working branch removed, the build worktree back on the loop's branch.
pub fn drop_update(run: &mut Run, why: &str) -> Result<(), String> {
    let Some(l) = run.state.feature_loop.clone() else {
        return Ok(());
    };
    let Some(u) = l.update.clone() else {
        return Ok(());
    };
    let wt = run.worktrees().join("build");
    if wt.join(".git").exists() && git::branch(&wt).is_none() {
        let _ = git::discard_changes(&wt);
        let head = git::rev(&run.state.repo, &format!("refs/heads/{}", l.build_branch))?;
        let _ = git::checkout_forward(&wt, &l.build_branch, &head);
    }
    if let Some(b) = &u.branch {
        let _ = git::delete_branch(&run.state.repo, b);
    }
    run.append(Event::UpdateDropped {
        n: u.n,
        why: why.to_string(),
    })
}
