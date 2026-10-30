//! A conversation's events (007 contracts/storage.md, data-model.md Conversation): the project
//! conversation or a spec session, kept as an append-only log like a run's, and its fold.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::rights::Mode;
use crate::run::events::{Log, Logged};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Spec,
}

/// A document the engineer edited on the page since the lead's last turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocEdit {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAnswer {
    Once,
    Always,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ConversationCreated {
        kind: Kind,
        project: String,
        feature: Option<String>,
        /// The conversation whose session a spec session forks (FR-034).
        forked_from: Option<String>,
        /// Where the lead works: Fallow's worktree, or the folder itself.
        workdir: PathBuf,
    },
    TurnStarted {
        n: u32,
        said: String,
        edits: Vec<DocEdit>,
        mode: Mode,
        /// The session given to the call: new, resumed, or forked from.
        session: String,
    },
    TurnFinished {
        n: u32,
        reply: String,
        /// The lead's whole structured result (contracts/step-results.md, `reply`).
        result: Option<Value>,
        /// The session the turn ran in (a fork's new id).
        session: Option<String>,
        documents_changed: Vec<String>,
        commit: Option<String>,
        discarded: Vec<String>,
        cost_usd: Option<f64>,
        failure: Option<String>,
    },
    ApprovalAsked {
        id: String,
        command: String,
    },
    ApprovalAnswered {
        id: String,
        answer: ApprovalAnswer,
        prefix: Option<String>,
    },
    ModeSet {
        mode: Mode,
    },
    /// The Spec Kit step a spec session is at (FR-006).
    StepSet {
        step: String,
    },
    DocEdited {
        path: String,
        version: u64,
    },
}

/// One turn of a conversation, folded.
#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub n: u32,
    pub said: String,
    pub edits: Vec<DocEdit>,
    pub mode: Mode,
    pub started: String,
    pub reply: Option<String>,
    pub result: Option<Value>,
    pub documents_changed: Vec<String>,
    pub commit: Option<String>,
    pub discarded: Vec<String>,
    pub cost_usd: Option<f64>,
    pub failure: Option<String>,
    pub approvals: Vec<Approval>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Approval {
    pub id: String,
    pub command: String,
    pub answer: Option<ApprovalAnswer>,
    pub prefix: Option<String>,
}

/// A conversation, folded from its events.
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub id: String,
    pub kind: Kind,
    pub project: String,
    pub feature: Option<String>,
    pub forked_from: Option<String>,
    pub workdir: PathBuf,
    /// The session the next turn resumes; none before the first turn.
    pub session: Option<String>,
    pub mode: Mode,
    pub step: Option<String>,
    pub turns: Vec<Turn>,
    /// A turn is running (started, not finished).
    pub busy: bool,
    pub cost_usd: f64,
    pub seq: u64,
}

impl State {
    pub fn fold(id: &str, events: &[Logged<Event>]) -> Result<State, String> {
        let Some(first) = events.first() else {
            return Err(format!("conversation {id}: no events"));
        };
        let Event::ConversationCreated {
            kind,
            project,
            feature,
            forked_from,
            workdir,
        } = &first.event
        else {
            return Err(format!(
                "conversation {id}: the first event is not conversation_created"
            ));
        };
        let mut s = State {
            id: id.to_string(),
            kind: *kind,
            project: project.clone(),
            feature: feature.clone(),
            forked_from: forked_from.clone(),
            workdir: workdir.clone(),
            session: None,
            mode: Mode::Ask,
            step: None,
            turns: Vec::new(),
            busy: false,
            cost_usd: 0.0,
            seq: first.seq,
        };
        for l in &events[1..] {
            s.apply(l);
        }
        Ok(s)
    }

    pub fn apply(&mut self, l: &Logged<Event>) {
        self.seq = l.seq;
        match &l.event {
            Event::ConversationCreated { .. } => {}
            Event::TurnStarted {
                n,
                said,
                edits,
                mode,
                ..
            } => {
                self.busy = true;
                self.turns.push(Turn {
                    n: *n,
                    said: said.clone(),
                    edits: edits.clone(),
                    mode: *mode,
                    started: l.at.clone(),
                    reply: None,
                    result: None,
                    documents_changed: Vec::new(),
                    commit: None,
                    discarded: Vec::new(),
                    cost_usd: None,
                    failure: None,
                    approvals: Vec::new(),
                });
            }
            Event::TurnFinished {
                n,
                reply,
                result,
                session,
                documents_changed,
                commit,
                discarded,
                cost_usd,
                failure,
            } => {
                self.busy = false;
                if let Some(s) = session {
                    self.session = Some(s.clone());
                }
                self.cost_usd += cost_usd.unwrap_or(0.0);
                if let Some(t) = self.turns.iter_mut().find(|t| t.n == *n) {
                    t.reply = Some(reply.clone());
                    t.result = result.clone();
                    t.documents_changed = documents_changed.clone();
                    t.commit = commit.clone();
                    t.discarded = discarded.clone();
                    t.cost_usd = *cost_usd;
                    t.failure = failure.clone();
                }
            }
            Event::ApprovalAsked { id, command } => {
                if let Some(t) = self.turns.last_mut() {
                    t.approvals.push(Approval {
                        id: id.clone(),
                        command: command.clone(),
                        answer: None,
                        prefix: None,
                    });
                }
            }
            Event::ApprovalAnswered { id, answer, prefix } => {
                if let Some(a) = self
                    .turns
                    .iter_mut()
                    .flat_map(|t| t.approvals.iter_mut())
                    .find(|a| &a.id == id)
                {
                    a.answer = Some(*answer);
                    a.prefix = prefix.clone();
                }
            }
            Event::ModeSet { mode } => self.mode = *mode,
            Event::StepSet { step } => self.step = Some(step.clone()),
            Event::DocEdited { .. } => {}
        }
    }

    /// The approval the lead waits for, if any.
    pub fn pending_approval(&self) -> Option<&Approval> {
        self.turns
            .last()
            .and_then(|t| t.approvals.iter().find(|a| a.answer.is_none()))
    }
}

/// Opens (creating) a conversation's log in `dir`.
pub type ConversationLog = Log<Event>;

#[cfg(test)]
mod tests {
    use super::*;

    fn logged(seq: u64, event: Event) -> Logged<Event> {
        Logged {
            seq,
            at: format!("2026-10-30T10:00:{seq:02}Z"),
            event,
        }
    }

    #[test]
    fn a_conversation_folds_its_turns_mode_and_approvals() {
        let events = vec![
            logged(
                1,
                Event::ConversationCreated {
                    kind: Kind::Spec,
                    project: "shop".into(),
                    feature: Some("008-checkout".into()),
                    forked_from: Some("c1".into()),
                    workdir: "w".into(),
                },
            ),
            logged(
                2,
                Event::TurnStarted {
                    n: 1,
                    said: "checkout".into(),
                    edits: vec![],
                    mode: Mode::Ask,
                    session: "s0".into(),
                },
            ),
            logged(
                3,
                Event::ApprovalAsked {
                    id: "a1".into(),
                    command: "pip list".into(),
                },
            ),
            logged(
                4,
                Event::ApprovalAnswered {
                    id: "a1".into(),
                    answer: ApprovalAnswer::Always,
                    prefix: Some("pip list".into()),
                },
            ),
            logged(
                5,
                Event::TurnFinished {
                    n: 1,
                    reply: "Drafted the spec.".into(),
                    result: None,
                    session: Some("s1".into()),
                    documents_changed: vec!["specs/008-checkout/spec.md".into()],
                    commit: Some("abc".into()),
                    discarded: vec!["shop/cart.py".into()],
                    cost_usd: Some(0.2),
                    failure: None,
                },
            ),
            logged(6, Event::ModeSet { mode: Mode::ReadOnly }),
            logged(
                7,
                Event::StepSet {
                    step: "clarify".into(),
                },
            ),
        ];
        let s = State::fold("c2", &events).unwrap();
        assert_eq!(s.kind, Kind::Spec);
        assert_eq!(s.session.as_deref(), Some("s1"));
        assert_eq!(s.mode, Mode::ReadOnly);
        assert_eq!(s.step.as_deref(), Some("clarify"));
        assert!(!s.busy);
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].discarded, ["shop/cart.py"]);
        assert_eq!(s.turns[0].approvals[0].answer, Some(ApprovalAnswer::Always));
        assert!(s.pending_approval().is_none());
        assert_eq!(s.seq, 7);
    }

    #[test]
    fn the_log_round_trips_conversation_events() {
        let d = std::env::temp_dir().join(format!("fallow-conv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (mut log, events, _) = ConversationLog::open(&d).unwrap();
        assert!(events.is_empty());
        log.append(Event::ConversationCreated {
            kind: Kind::Project,
            project: "shop".into(),
            feature: None,
            forked_from: None,
            workdir: "w".into(),
        })
        .unwrap();
        log.append(Event::ModeSet {
            mode: Mode::RunAnything,
        })
        .unwrap();
        drop(log);
        let (_, events, notes) = ConversationLog::open(&d).unwrap();
        assert!(notes.is_empty());
        let s = State::fold("c1", &events).unwrap();
        assert_eq!(s.mode, Mode::RunAnything);
        let _ = std::fs::remove_dir_all(&d);
    }
}
