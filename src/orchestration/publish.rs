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
                Condition::Signal { event, role } => {
                    let signed = b.signed.get(event).cloned().unwrap_or_default();
                    // A session that exited without the signal can never give it: the merge
                    // no longer waits for it; one that gave it before exiting still counts.
                    let (gone, who): (BTreeSet<u32>, BTreeSet<u32>) = counting(b, role)
                        .into_iter()
                        .partition(|id| dropped(b, *id, event));
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
                            format!("no {role} session whose signals count{gone}"),
                        )
                    } else if missing.is_empty() {
                        (true, format!("signalled by every {role}{gone}"))
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
