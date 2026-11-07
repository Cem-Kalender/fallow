//! 012 (data-model.md, "Solo session"): what only a solo session records, and its fold.
//! A session is a conversation of kind `solo`; its turns are the conversation's turns, and
//! everything else about it is here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An image attached to a message, stored by Fallow (research R-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRec {
    pub sha: String,
    pub name: String,
    pub media_type: String,
    pub size: u64,
}

/// A path as a turn left it, recorded when its commit failed (R-4): the content's blob, or
/// `None` when the turn deleted it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedRec {
    pub path: String,
    pub blob: Option<String>,
}

/// Why the base moved (R-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseWhy {
    UpToDate,
    Squash,
}

/// The engineer's folder after a command that touched it (R-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderState {
    Changed,
    Conflicted,
    StagedOnly,
    Unchanged,
}

/// How a branch moved between two observations (R-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    Created,
    Deleted,
    Commits,
    Rewound,
    Rewritten,
}

/// How a session was closed (R-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
    Merged,
    Kept,
    KeptWithCopy,
    Discarded,
}

/// The engineer's choice at a close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseChoice {
    Merged,
    Keep,
    Discard,
}

/// The session's base (R-5): what Changes diff against, and what the gates' baseline runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Base {
    pub mark: String,
    pub gates: String,
}

/// A message typed while a turn works (R-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Waiting {
    pub id: String,
    pub text: String,
    pub attachments: Vec<AttachmentRec>,
}

/// A change this session's command made to a branch (R-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub refname: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub how: How,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub commits: Vec<String>,
    pub turn: u32,
}

/// One gate's outcome in a session's gate run (R-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateOutcome {
    pub gate: String,
    pub outcome: String,
    pub class: String,
    #[serde(default)]
    pub output: String,
}

/// The session's events (data-model.md). They sit in the conversation's log beside its turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SoloEvent {
    SessionCreating {
        branch: String,
        start_commit: String,
    },
    SessionCreated {
        branch: String,
        start_branch: Option<String>,
        start_commit: String,
        worktree: std::path::PathBuf,
        dirty_at_start: bool,
        web: bool,
        #[serde(default)]
        lfs: bool,
    },
    SessionRenamed {
        name: String,
        /// `first_message` when named after it; none when the engineer renamed it.
        #[serde(default)]
        by: Option<String>,
    },
    BranchRenamed {
        from: String,
        to: String,
    },
    TurnCommitted {
        n: u32,
        commit: String,
    },
    FoldNext {
        n: u32,
    },
    UncommittedDiscarded {
        n: u32,
        files: Vec<String>,
    },
    EngineerEdits {
        /// The turn they came before (`turn <n>`), or `undo`, `close`, `gates`.
        before: String,
        commit: String,
        files: Vec<String>,
    },
    BaseMoved {
        turn: u32,
        mark: String,
        gates: String,
        why: BaseWhy,
    },
    UndoStarted {
        from: u32,
        head_before: String,
        head_after: String,
    },
    TurnsUndone {
        from: u32,
        to: u32,
        head_before: String,
        head_after: String,
        keep_ref: String,
    },
    MessageQueued {
        id: String,
        text: String,
        #[serde(default)]
        attachments: Vec<AttachmentRec>,
    },
    QueueEdited {
        id: String,
        text: String,
    },
    QueueRemoved {
        id: String,
    },
    QueueSent {
        id: String,
        n: u32,
    },
    WebSet {
        on: bool,
    },
    GitObserved(Observed),
    FolderStateSeen {
        state: FolderState,
        turn: u32,
    },
    Squashed {
        branch: String,
        commit: String,
        upto_turn: u32,
    },
    GatesRun {
        commit: String,
        base: String,
        results: Vec<GateOutcome>,
    },
    FeatureSuggested {
        n: u32,
        title: String,
        brief: String,
    },
    FeatureStarting {
        title: String,
        brief: String,
        lead: Option<String>,
    },
    FeatureStarted {
        feature: String,
        title: String,
        conversation: String,
    },
    /// 012 (R-15): a handover that made nothing (a title taken, no lead): the dialog is free.
    FeatureDropped {
        why: String,
    },
    CloseStarted {
        choice: CloseChoice,
        /// The fingerprint of what the engineer was shown (Phase 10 review): a restart finishes
        /// the choice only while the session still holds exactly that.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        digest: Option<String>,
    },
    SessionClosed {
        how: Closed,
        branch: Option<String>,
        worktree: Option<std::path::PathBuf>,
    },
    CopyRemoved {
        files: Vec<String>,
    },
}

/// A solo session, folded (data-model.md, "State").
#[derive(Debug, Clone, Default, Serialize)]
pub struct Solo {
    pub name: Option<String>,
    pub creating: Option<(String, String)>,
    pub branch: String,
    pub start_branch: Option<String>,
    pub start_commit: String,
    pub worktree: std::path::PathBuf,
    pub dirty_at_start: bool,
    pub lfs: bool,
    pub web: bool,
    /// The turns' commits, by turn: what Fallow committed (`commit`) and every commit it added.
    pub commits: std::collections::BTreeMap<u32, TurnCommits>,
    /// Turns undone (each stays visible, marked).
    pub undone: std::collections::BTreeSet<u32>,
    /// The turn whose commit failed and has not been committed, folded or discarded since.
    pub pending_commit: Option<u32>,
    /// `fold_next` asked for the pending turn.
    pub fold_next: bool,
    pub engineer_edits: Vec<(String, String, Vec<String>)>,
    pub base_moves: Vec<(u32, Base, BaseWhy)>,
    pub undoing: Option<(u32, String, String)>,
    pub queue: Vec<Waiting>,
    pub observed: Vec<Observed>,
    pub folder: Vec<(u32, FolderState)>,
    pub squashes: Vec<(String, String, u32)>,
    pub gates: Option<(String, String, Vec<GateOutcome>)>,
    pub suggestions: Vec<(u32, String, String)>,
    pub starting: Option<(String, String, Option<String>)>,
    pub features: Vec<(String, String, String)>,
    pub closing: Option<CloseChoice>,
    pub closing_digest: Option<String>,
    pub closed: Option<(Closed, Option<String>, Option<std::path::PathBuf>)>,
    pub copy_removed: bool,
}

/// A turn's commits (R-3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TurnCommits {
    /// The branch's head after the turn (or after its retried commit).
    pub commit: Option<String>,
    /// Every commit the turn added, the assistant's own included.
    pub commits: Vec<String>,
    pub failed: Option<String>,
    pub changed: Vec<ChangedRec>,
    pub cut: bool,
    /// The turn whose commit holds this one's changes too ("fix it in the next turn").
    pub folded_into: Option<u32>,
}

impl Solo {
    pub fn apply(&mut self, e: &SoloEvent) {
        match e {
            SoloEvent::SessionCreating {
                branch,
                start_commit,
            } => self.creating = Some((branch.clone(), start_commit.clone())),
            SoloEvent::SessionCreated {
                branch,
                start_branch,
                start_commit,
                worktree,
                dirty_at_start,
                web,
                lfs,
            } => {
                self.creating = None;
                self.branch = branch.clone();
                self.start_branch = start_branch.clone();
                self.start_commit = start_commit.clone();
                self.worktree = worktree.clone();
                self.dirty_at_start = *dirty_at_start;
                self.web = *web;
                self.lfs = *lfs;
            }
            SoloEvent::SessionRenamed { name, .. } => self.name = Some(name.clone()),
            SoloEvent::BranchRenamed { to, .. } => self.branch = to.clone(),
            SoloEvent::TurnCommitted { n, commit } => {
                let t = self.commits.entry(*n).or_default();
                t.commit = Some(commit.clone());
                t.commits.push(commit.clone());
                t.failed = None;
                if self.pending_commit == Some(*n) {
                    self.pending_commit = None;
                    self.fold_next = false;
                }
            }
            SoloEvent::FoldNext { n } => {
                if self.pending_commit == Some(*n) {
                    self.fold_next = true;
                }
            }
            SoloEvent::UncommittedDiscarded { n, .. } => {
                if self.pending_commit == Some(*n) {
                    self.pending_commit = None;
                    self.fold_next = false;
                }
            }
            SoloEvent::EngineerEdits {
                before,
                commit,
                files,
            } => self
                .engineer_edits
                .push((before.clone(), commit.clone(), files.clone())),
            SoloEvent::BaseMoved {
                turn,
                mark,
                gates,
                why,
            } => self.base_moves.push((
                *turn,
                Base {
                    mark: mark.clone(),
                    gates: gates.clone(),
                },
                *why,
            )),
            SoloEvent::UndoStarted {
                from,
                head_before,
                head_after,
            } => self.undoing = Some((*from, head_before.clone(), head_after.clone())),
            SoloEvent::TurnsUndone { from, to, .. } => {
                self.undoing = None;
                for n in *from..=*to {
                    self.undone.insert(n);
                }
            }
            SoloEvent::MessageQueued {
                id,
                text,
                attachments,
            } => self.queue.push(Waiting {
                id: id.clone(),
                text: text.clone(),
                attachments: attachments.clone(),
            }),
            SoloEvent::QueueEdited { id, text } => {
                if let Some(w) = self.queue.iter_mut().find(|w| &w.id == id) {
                    w.text = text.clone();
                }
            }
            SoloEvent::QueueRemoved { id } | SoloEvent::QueueSent { id, .. } => {
                self.queue.retain(|w| &w.id != id)
            }
            SoloEvent::WebSet { on } => self.web = *on,
            SoloEvent::GitObserved(o) => self.observed.push(o.clone()),
            SoloEvent::FolderStateSeen { state, turn } => self.folder.push((*turn, *state)),
            SoloEvent::Squashed {
                branch,
                commit,
                upto_turn,
            } => self
                .squashes
                .push((branch.clone(), commit.clone(), *upto_turn)),
            SoloEvent::GatesRun {
                commit,
                base,
                results,
            } => self.gates = Some((commit.clone(), base.clone(), results.clone())),
            SoloEvent::FeatureSuggested { n, title, brief } => {
                self.suggestions.push((*n, title.clone(), brief.clone()))
            }
            SoloEvent::FeatureStarting { title, brief, lead } => {
                self.starting = Some((title.clone(), brief.clone(), lead.clone()))
            }
            SoloEvent::FeatureStarted {
                feature,
                title,
                conversation,
            } => {
                self.starting = None;
                self.features
                    .push((feature.clone(), title.clone(), conversation.clone()))
            }
            SoloEvent::FeatureDropped { .. } => self.starting = None,
            SoloEvent::CloseStarted { choice, digest } => {
                self.closing = Some(*choice);
                self.closing_digest = digest.clone();
            }
            SoloEvent::SessionClosed {
                how,
                branch,
                worktree,
            } => {
                self.closing = None;
                self.closed = Some((*how, branch.clone(), worktree.clone()));
            }
            SoloEvent::CopyRemoved { .. } => self.copy_removed = true,
        }
    }

    /// A turn ended (`TurnFinished`): its commits as recorded.
    pub fn turn_finished(&mut self, n: u32, t: TurnCommits) {
        // The pending turn's changes went into this one's commit (review M4: first, so a
        // folded commit that fails too stays pending).
        if let Some(into) = t.folded_into {
            if self.pending_commit == Some(into) {
                self.pending_commit = None;
                self.fold_next = false;
            }
        }
        if t.failed.is_some() {
            self.pending_commit = Some(n);
        }
        self.commits.insert(n, t);
    }

    /// The base (R-5): the last `base_moved` of a turn that is not undone, else the start.
    pub fn base(&self) -> Base {
        self.base_moves
            .iter()
            .rev()
            .find(|(turn, _, _)| !self.undone.contains(turn))
            .map(|(_, b, _)| b.clone())
            .unwrap_or_else(|| Base {
                mark: self.start_commit.clone(),
                gates: self.start_commit.clone(),
            })
    }

    /// The gates' label (FR-022): whether they ran on `head`, and how they ended.
    pub fn gates_label(&self, head: &str, declared: bool) -> &'static str {
        if !declared {
            return "no_gates";
        }
        match &self.gates {
            None => "not_run",
            Some((commit, _, _)) if commit != head => "stale",
            Some((_, _, results)) if results.iter().all(|r| r.class == "passed" || r.class == "pass" || r.class == "skipped") => "passed",
            Some(_) => "failed",
        }
    }
}

/// The structured result of an assistant's turn (contracts/runtime.md, `assistant_reply`).
pub fn suggestion_of(result: &Value) -> Option<(String, String)> {
    let f = result.get("proposed_feature")?;
    let title = f["title"].as_str()?.trim().to_string();
    let brief = f["brief"].as_str()?.trim().to_string();
    (!title.is_empty() || !brief.is_empty()).then_some((title, brief))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created() -> Solo {
        let mut s = Solo::default();
        s.apply(&SoloEvent::SessionCreated {
            branch: "fallow/solo/fix".into(),
            start_branch: Some("main".into()),
            start_commit: "s0".into(),
            worktree: "wt".into(),
            dirty_at_start: true,
            web: true,
            lfs: false,
        });
        s
    }

    #[test]
    fn the_base_is_the_last_move_of_a_turn_not_undone() {
        let mut s = created();
        assert_eq!(s.base(), Base { mark: "s0".into(), gates: "s0".into() });
        s.apply(&SoloEvent::BaseMoved { turn: 2, mark: "m2".into(), gates: "g2".into(), why: BaseWhy::UpToDate });
        s.apply(&SoloEvent::BaseMoved { turn: 4, mark: "t3".into(), gates: "sq".into(), why: BaseWhy::Squash });
        assert_eq!(s.base().gates, "sq");
        s.apply(&SoloEvent::TurnsUndone { from: 4, to: 4, head_before: "h".into(), head_after: "a".into(), keep_ref: "r".into() });
        assert_eq!(s.base().gates, "g2", "undoing the squash's turn moves the base back");
        s.apply(&SoloEvent::TurnsUndone { from: 2, to: 3, head_before: "h".into(), head_after: "a".into(), keep_ref: "r".into() });
        assert_eq!(s.base().gates, "s0");
    }

    #[test]
    fn a_failed_commit_is_pending_until_committed_folded_or_discarded() {
        let mut s = created();
        s.turn_finished(1, TurnCommits { failed: Some("index.lock".into()), ..Default::default() });
        assert_eq!(s.pending_commit, Some(1));
        s.apply(&SoloEvent::FoldNext { n: 1 });
        assert!(s.fold_next);
        s.turn_finished(2, TurnCommits { commit: Some("c2".into()), folded_into: Some(1), ..Default::default() });
        assert_eq!(s.pending_commit, None);
        // A folded commit that fails too stays pending, as the later turn.
        s.turn_finished(5, TurnCommits { failed: Some("a".into()), ..Default::default() });
        s.turn_finished(6, TurnCommits { failed: Some("b".into()), folded_into: Some(5), ..Default::default() });
        assert_eq!(s.pending_commit, Some(6));
        s.apply(&SoloEvent::TurnCommitted { n: 6, commit: "c6".into() });
        s.turn_finished(3, TurnCommits { failed: Some("x".into()), ..Default::default() });
        s.apply(&SoloEvent::TurnCommitted { n: 3, commit: "c3".into() });
        assert_eq!(s.pending_commit, None);
        assert_eq!(s.commits[&3].commit.as_deref(), Some("c3"));
    }

    #[test]
    fn the_queue_keeps_its_order_and_edits() {
        let mut s = created();
        for (id, t) in [("q1", "one"), ("q2", "two"), ("q3", "three")] {
            s.apply(&SoloEvent::MessageQueued { id: id.into(), text: t.into(), attachments: vec![] });
        }
        s.apply(&SoloEvent::QueueEdited { id: "q2".into(), text: "TWO".into() });
        s.apply(&SoloEvent::QueueRemoved { id: "q3".into() });
        s.apply(&SoloEvent::QueueSent { id: "q1".into(), n: 2 });
        assert_eq!(s.queue.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["TWO"]);
    }

    #[test]
    fn the_gates_label_follows_the_head() {
        let mut s = created();
        assert_eq!(s.gates_label("h1", false), "no_gates");
        assert_eq!(s.gates_label("h1", true), "not_run");
        s.apply(&SoloEvent::GatesRun { commit: "h1".into(), base: "s0".into(), results: vec![GateOutcome { gate: "tests".into(), outcome: "passed".into(), class: "passed".into(), output: String::new() }] });
        assert_eq!(s.gates_label("h1", true), "passed");
        assert_eq!(s.gates_label("h2", true), "stale");
    }

    #[test]
    fn events_round_trip_as_tagged_json() {
        let e = SoloEvent::GitObserved(Observed { refname: "refs/heads/main".into(), before: Some("a".into()), after: Some("b".into()), how: How::Commits, n: Some(1), commits: vec!["b".into()], turn: 3 });
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["type"], "git_observed");
        assert_eq!(serde_json::from_value::<SoloEvent>(j).unwrap(), e);
    }
}
