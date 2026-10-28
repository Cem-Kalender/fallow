//! Merge conditions, readiness and publishing (004 US3, US6, FR-027, FR-028).
//!
//! Readiness is derived from the hub's state each time it is asked for; nothing an agent says
//! makes a condition hold except signals from the right role, tasks reported done and gate runs
//! Fallow observed itself (SC-003).

use std::collections::BTreeSet;

use super::burst::Burst;
use super::gates::Kind;
use super::squad::{Condition, Publish};
use super::{Hub, tasks};
use crate::projects::ProjectId;

/// How the work is published, resolved for a burst (FR-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DraftPr,
    LocalMerge,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::DraftPr => "draft-pr",
            Mode::LocalMerge => "local-merge",
        }
    }
}

pub fn mode(b: &Burst) -> Mode {
    match (b.squad.publish, b.has_remote) {
        (Publish::DraftPr, _) | (Publish::Auto, true) => Mode::DraftPr,
        _ => Mode::LocalMerge,
    }
}

/// One merge condition now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub condition: String,
    pub met: bool,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub checks: Vec<Check>,
    /// Every condition holds.
    pub ready: bool,
    /// The commit the final gates ran on, if any.
    pub commit: Option<String>,
    pub mode: Mode,
}

pub fn condition_label(c: &Condition) -> String {
    match c {
        Condition::Gate(g) => format!("gate:{g}"),
        Condition::AllDone(r) => format!("done:{r}"),
        Condition::Signal { event, role } => format!("signal:{event}@{role}"),
        Condition::WorkDone => "work:done".to_string(),
        Condition::ReviewApproved => "review:approved".to_string(),
    }
}

/// Whether every counting session in `roles` signalled `event` (004 FR-027, 005 FR-041): exited
/// sessions without it are no longer waited for, an objection that stands keeps waiting.
fn signal_check(b: &Burst, event: &str, roles: &[&str], what: &str) -> (bool, String) {
    let signed = b.signed.get(event).cloned().unwrap_or_default();
    let mut all = BTreeSet::new();
    for r in roles {
        all.extend(counting(b, r));
    }
    let (gone, who): (BTreeSet<u32>, BTreeSet<u32>) =
        all.into_iter().partition(|id| dropped(b, *id, event));
    let stands: Vec<String> = who
        .iter()
        .filter(|id| b.exited.contains(id) && objects(b, **id, event))
        .map(|id| format!("#{id}"))
        .collect();
    let gone = if gone.is_empty() {
        String::new()
    } else {
        format!(
            " ({} exited without it)",
            gone.iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let missing: Vec<String> = who
        .iter()
        .filter(|id| !signed.contains(id))
        .map(|id| format!("#{id}"))
        .collect();
    if who.is_empty() {
        (
            false,
            format!("no {what} session whose signals count{gone}"),
        )
    } else if missing.is_empty() {
        (true, format!("signalled by every {what}{gone}"))
    } else if !stands.is_empty() {
        (
            false,
            format!(
                "waiting for {}{gone}; {} exited asking for changes (the user clears review-changes-requested to drop it)",
                missing.join(", "),
                stands.join(", ")
            ),
        )
    } else {
        (false, format!("waiting for {}{gone}", missing.join(", ")))
    }
}

/// Sessions in `role` whose signals count (FR-013).
fn counting(b: &Burst, role: &str) -> BTreeSet<u32> {
    b.roles
        .iter()
        .filter(|(id, r)| *r == role && !b.tainted.contains_key(id))
        .map(|(id, _)| *id)
        .collect()
}

/// `id` asked for changes and that request stands: it is the opposite of `event`.
pub(crate) fn objects(b: &Burst, id: u32, event: &str) -> bool {
    event == "review-approved"
        && b.signed
            .get("review-changes-requested")
            .is_some_and(|s| s.contains(&id))
}

/// `id` exited without giving `event` and without an objection that stands: no longer waited
/// for (FR-041). An objection keeps the merge waiting until the engineer clears it.
pub(crate) fn dropped(b: &Burst, id: u32, event: &str) -> bool {
    b.exited.contains(&id)
        && !b.signed.get(event).is_some_and(|s| s.contains(&id))
        && !objects(b, id, event)
}

impl Hub {
    /// The merge readiness of a project's burst (data-model "MergeReadiness").
    pub fn readiness(&self, project: ProjectId) -> Option<Readiness> {
        let b = self.burst_of(project)?;
        let mut checks = Vec::new();
        for c in &b.squad.merge_requires {
            let (met, why) = match c {
                Condition::Gate(g) => {
                    match b
                        .gate_runs
                        .iter()
                        .rev()
                        .find(|r| &r.gate == g && r.kind == Kind::Final)
                    {
                        None => (false, "the final run has not happened yet".to_string()),
                        Some(r) => match r.class {
                            None => (false, r.state.label()),
                            Some(c) => (c.opens(), format!("{}: {}", r.state.label(), c.label())),
                        },
                    }
                }
                Condition::AllDone(role) => {
                    let members: BTreeSet<u32> = b
                        .roles
                        .iter()
                        .filter(|(_, r)| *r == role)
                        .map(|(id, _)| *id)
                        .collect();
                    let mut handed: Vec<&tasks::Task> = self
                        .tasks
                        .list
                        .iter()
                        .filter(|t| {
                            t.created_at >= b.started
                                && t.assigned.is_some_and(|a| members.contains(&a))
                        })
                        .collect();
                    // A withdrawn task is no longer work to finish, but the reason says so
                    // (FR-042, review).
                    let withdrawn = handed
                        .iter()
                        .filter(|t| matches!(t.state, tasks::State::Cancelled(_)))
                        .count();
                    handed.retain(|t| !matches!(t.state, tasks::State::Cancelled(_)));
                    let withdrawn = if withdrawn == 0 {
                        String::new()
                    } else {
                        format!(", {withdrawn} withdrawn")
                    };
                    let open: Vec<String> = handed
                        .iter()
                        .filter(|t| t.state != tasks::State::Done)
                        .map(|t| format!("T{} {}", t.id, t.state.label()))
                        .collect();
                    if handed.is_empty() {
                        (
                            false,
                            format!("no task was handed to a {role} yet{withdrawn}"),
                        )
                    } else if open.is_empty() {
                        (true, format!("{} task(s) done{withdrawn}", handed.len()))
                    } else {
                        (false, open.join(", "))
                    }
                }
                // A session that exited without the signal can never give it: the merge no
                // longer waits for it; one that gave it before exiting still counts.
                Condition::Signal { event, role } => signal_check(b, event, &[role], role),
                Condition::WorkDone => self.work_done(b),
                Condition::ReviewApproved => {
                    let reviewers = b.squad.reviewer_roles();
                    match b.manifest.as_ref().filter(|m| !m.reviews.is_empty()) {
                        None => signal_check(b, "review-approved", &reviewers, "reviewer"),
                        Some(m) => {
                            // One approval that counts per focus (005 FR-031).
                            let approved = b.signed.get("review-approved");
                            let missing: Vec<&str> = m
                                .reviews
                                .iter()
                                .filter(|r| {
                                    !b.focus.iter().any(|(id, f)| {
                                        *f == r.focus
                                            && !b.tainted.contains_key(id)
                                            && approved.is_some_and(|s| s.contains(id))
                                    })
                                })
                                .map(|r| r.focus.as_str())
                                .collect();
                            if missing.is_empty() {
                                (true, "approved for every focus".to_string())
                            } else {
                                (false, format!("no approval yet for {}", missing.join(", ")))
                            }
                        }
                    }
                }
            };
            checks.push(Check {
                condition: condition_label(c),
                met,
                why,
            });
        }
        let commit = b
            .gate_runs
            .iter()
            .rev()
            .find(|r| r.kind == Kind::Final)
            .and_then(|r| r.commit.clone());
        Some(Readiness {
            ready: checks.iter().all(|c| c.met),
            checks,
            commit,
            mode: mode(b),
        })
    }

    /// `work:done` (005 R-16): every piece of work in the manifest reported done with evidence,
    /// by the lead or its builders; without a manifest, the lead's task for the burst.
    fn work_done(&self, b: &Burst) -> (bool, String) {
        let of_burst = |t: &&tasks::Task| {
            t.created_at >= b.started && !matches!(t.state, tasks::State::Cancelled(_))
        };
        let Some(m) = &b.manifest else {
            return match self
                .tasks
                .list
                .iter()
                .filter(of_burst)
                .find(|t| t.work.as_deref() == Some(super::manifest::BURST_WORK))
            {
                Some(t) if t.state == tasks::State::Done => {
                    (true, "the lead reported the work done".to_string())
                }
                Some(t) => (
                    false,
                    format!("T{} {} (the lead's work)", t.id, t.state.label()),
                ),
                None => (false, "the lead has no task for the work yet".to_string()),
            };
        };
        if m.work.is_empty() {
            return (false, "the manifest lists no work".to_string());
        }
        let mut open = Vec::new();
        for w in &m.work {
            let mine: Vec<&tasks::Task> = self
                .tasks
                .list
                .iter()
                .filter(of_burst)
                .filter(|t| t.work.as_deref() == Some(w.id.as_str()))
                .collect();
            let parts: Vec<&str> = m.parts_of(&w.id).map(|p| p.id.as_str()).collect();
            // Work behind a change only the lead makes counts once accepted (R-19, FR-024).
            let waits = |t: &&tasks::Task| {
                t.evidence.as_ref().is_some_and(|e| {
                    self.signals.list.iter().any(|s| {
                        s.by_fallow && s.open() && s.commit.as_deref() == Some(e.commit.as_str())
                    })
                })
            };
            let undone: Vec<String> = mine
                .iter()
                .filter(|t| t.state != tasks::State::Done || waits(t))
                .map(|t| {
                    if waits(t) {
                        format!("T{} waits for the lead to accept a change", t.id)
                    } else {
                        format!("T{} {}", t.id, t.state.label())
                    }
                })
                .collect();
            let unbuilt: Vec<&str> = parts
                .iter()
                .copied()
                .filter(|p| {
                    !mine
                        .iter()
                        .any(|t| t.part.as_deref() == Some(*p) && t.state == tasks::State::Done)
                })
                .collect();
            if mine.is_empty() {
                open.push(format!("{}: not started", w.id));
            } else if !undone.is_empty() {
                open.push(format!("{}: {}", w.id, undone.join(", ")));
            } else if !unbuilt.is_empty() {
                open.push(format!("{}: no done task for {}", w.id, unbuilt.join(", ")));
            }
        }
        if open.is_empty() {
            (true, format!("{} piece(s) of work done", m.work.len()))
        } else {
            (false, open.join("; "))
        }
    }

    /// Records who signalled what during a burst, for `signal:` conditions. A reviewer's latest
    /// verdict replaces its earlier one (review-approved / review-changes-requested).
    pub(crate) fn record_signal(&mut self, session: u32, event: &str) {
        let Some(b) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&session))
        else {
            return;
        };
        let opposite = match event {
            "review-approved" => Some("review-changes-requested"),
            "review-changes-requested" => Some("review-approved"),
            _ => None,
        };
        if let Some(o) = opposite {
            if let Some(s) = b.signed.get_mut(o) {
                s.remove(&session);
            }
        }
        b.signed
            .entry(event.to_string())
            .or_default()
            .insert(session);
    }
}

impl Hub {
    /// A reviewer's verdict: kept with its notes; reviewers that disagree are shown to the
    /// engineer, never resolved silently (FR-027).
    pub(crate) fn verdict(&mut self, session: u32, approve: bool, notes: &str) {
        let Some(b) = self
            .bursts
            .iter_mut()
            .find(|b| b.roles.contains_key(&session))
        else {
            return;
        };
        let v = if approve {
            "approved"
        } else {
            "changes-requested"
        };
        b.verdicts
            .insert(session, (v.to_string(), notes.to_string()));
        b.lead_updates.push(super::lead::one_line(&format!(
            "#{session} reviewer {}: {notes}",
            if approve {
                "approved"
            } else {
                "asks for changes"
            }
        )));
        let counting: Vec<(u32, String, String)> = b
            .verdicts
            .iter()
            .filter(|(id, _)| !b.tainted.contains_key(id))
            .map(|(id, (v, n))| (*id, v.clone(), n.clone()))
            .collect();
        let yes = counting.iter().any(|(_, v, _)| v == "approved");
        let no = counting.iter().any(|(_, v, _)| v == "changes-requested");
        if yes && no {
            let mut parts: Vec<String> = counting
                .iter()
                .map(|(id, v, n)| format!("#{id} {v}: {n}"))
                .collect();
            parts.sort();
            let text = format!("Reviewers disagree: {}", parts.join(" | "));
            let short: Vec<String> = counting
                .iter()
                .map(|(id, v, _)| format!("#{id} {v}"))
                .collect();
            let lead_line = format!(
                "Reviewers disagree: {} (their notes are in their verdict lines); rule on it or bring it to the user.",
                short.join(", ")
            );
            let project = b.project;
            self.boards.post(
                super::board::BoardId::Project(project),
                super::Party::Fallow,
                text.clone(),
                std::time::SystemTime::now(),
                super::TEXT_BUDGET,
            );
            self.tell_lead(project, lead_line);
            self.alerts.push(text.chars().take(1500).collect());
        }
    }

    /// The lead reports the draft pull request it opened (draft-PR mode).
    pub(crate) fn published(&mut self, project: ProjectId, url: &str, title: &str) {
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        b.published = Some(url.to_string());
        let text = format!("Draft pull request opened by the lead: {title} {url}. You merge it.");
        self.boards.post(
            super::board::BoardId::Project(project),
            super::Party::Fallow,
            text.clone(),
            std::time::SystemTime::now(),
            super::TEXT_BUDGET,
        );
        self.notices.push(text);
    }

    /// Fallow merged the gated commit on the engineer's press (or automatically, as the squad
    /// says).
    pub fn merged(&mut self, project: ProjectId, gated: &str, merge: &str) {
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        b.merged.push((gated.to_string(), merge.to_string()));
        let target = b
            .target_branch
            .clone()
            .unwrap_or_else(|| "the target branch".into());
        let text = format!(
            "Merged {} into {target} ({})",
            &gated[..gated.len().min(10)],
            &merge[..merge.len().min(10)]
        );
        self.boards.post(
            super::board::BoardId::Project(project),
            super::Party::Fallow,
            text.clone(),
            std::time::SystemTime::now(),
            super::TEXT_BUDGET,
        );
        self.notices.push(text);
    }

    /// Commits on the target branch that Fallow did not merge (FR-028a): each is reported once.
    pub fn foreign_commits(&mut self, project: ProjectId, commits: Vec<String>) {
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        let new: Vec<String> = commits
            .into_iter()
            .filter(|c| !b.foreign_seen.contains(c))
            .collect();
        if new.is_empty() {
            return;
        }
        b.foreign_seen.extend(new.iter().cloned());
        let target = b.target_branch.clone().unwrap_or_default();
        let shown: Vec<&str> = new.iter().take(5).map(|c| &c[..c.len().min(10)]).collect();
        self.notices.push(format!(
            "{} new commit(s) on {target} during coordination, not merged by Fallow: {}",
            new.len(),
            shown.join(", ")
        ));
    }
}

fn git(dir: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The engineer's local merge (FR-028): `git merge --no-ff <gated>` in the project folder, which
/// must be on the target branch and clean. Returns the new commit of the target branch.
pub fn local_merge(dir: &std::path::Path, target: &str, gated: &str) -> Result<String, String> {
    let on = super::git::branch(dir).unwrap_or_default();
    if on != target {
        return Err(format!(
            "the project folder is on {}, not on the target branch {target}",
            if on.is_empty() {
                "a detached HEAD"
            } else {
                &on
            }
        ));
    }
    if !super::git::changes(dir)?.is_empty() {
        return Err("the project folder has uncommitted changes".into());
    }
    if !super::git::commit_exists(dir, gated) {
        return Err(format!("commit {gated} is not in the project's repository"));
    }
    let msg = format!(
        "Merge {} (coordination, merged by Fallow)",
        &gated[..gated.len().min(10)]
    );
    if let Err(e) = git(dir, &["merge", "--no-ff", "-m", &msg, gated]) {
        let _ = git(dir, &["merge", "--abort"]);
        return Err(format!("git merge failed: {e}"));
    }
    git(dir, &["rev-parse", "HEAD"])
}

/// Commits on `target` since `start` that neither came from a merged gated commit nor are
/// Fallow's merge commits (FR-028a).
pub fn foreign(
    dir: &std::path::Path,
    start: &str,
    target: &str,
    merged: &[(String, String)],
) -> Result<Vec<String>, String> {
    let mut args = vec!["rev-list".to_string(), format!("{start}..{target}")];
    for (g, _) in merged {
        args.push(format!("^{g}"));
    }
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(git(dir, &a)?
        .lines()
        .map(str::to_string)
        .filter(|c| !merged.iter().any(|(_, m)| m == c))
        .collect())
}
