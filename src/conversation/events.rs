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

/// A document as the engineer left it before a turn, kept to be put back if the turn is
/// withdrawn (008 T047).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptDoc {
    pub path: String,
    pub text: String,
}

/// What undoing a cancelled turn needs, recorded when the engineer cancels so that a restart
/// before the turn has ended still undoes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancel {
    pub head: Option<String>,
    pub kept: Vec<KeptDoc>,
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
        /// The lead agent the session talks to (009); none reads as `lead`.
        #[serde(default)]
        lead: Option<String>,
    },
    TurnStarted {
        n: u32,
        said: String,
        edits: Vec<DocEdit>,
        mode: Mode,
        /// The session given to the call: new, resumed, or forked from.
        session: String,
        /// What the lead's call ran on (009), and whether it began an agent session of its own
        /// (a new one or a fork, not a resume): the session's runtime and model are that turn's,
        /// once it has a session.
        #[serde(default)]
        runtime: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        new_session: bool,
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
        /// The engineer cancelled the turn while the lead answered (008 T047): its message is
        /// withdrawn and what the lead did for it is undone.
        #[serde(default)]
        withdrawn: bool,
    },
    /// The engineer cancelled turn `n` (008 T047): where the lead's working copy stood when the
    /// turn began, and the engineer's edited documents as they were then.
    CancelAsked {
        n: u32,
        head: Option<String>,
        kept: Vec<KeptDoc>,
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
        /// The edit since the lead last saw the document, for its next turn (kept so it
        /// survives a restart; Phase 3 review).
        #[serde(default)]
        diff: String,
    },
    /// The lead works in another place from now on (a plain folder that got git).
    WorkdirSet {
        workdir: PathBuf,
    },
    /// The engineer picked another lead before the session's first message (009).
    LeadSet {
        lead: String,
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
    /// Cancelled by the engineer: the message was withdrawn.
    pub withdrawn: bool,
    /// The cancel asked while the turn ran, until it ends.
    #[serde(skip)]
    pub cancel: Option<Cancel>,
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
    /// The engineer's edits since the lead's last turn, one per document.
    pub pending_edits: Vec<DocEdit>,
    /// The lead agent the session talks to (009); `None` is `lead`.
    pub lead: Option<String>,
    /// The runtime and model the current agent session began on (009).
    pub lead_runtime: Option<String>,
    pub lead_model: Option<String>,
    /// Those of a turn that began a session, until it ends with one.
    #[serde(skip)]
    pub lead_pending: Option<(Option<String>, Option<String>)>,
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
            lead,
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
            pending_edits: Vec::new(),
            lead: lead.clone(),
            lead_runtime: None,
            lead_model: None,
            lead_pending: None,
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
                runtime,
                model,
                new_session,
                ..
            } => {
                self.busy = true;
                self.pending_edits.clear();
                self.lead_pending = new_session.then(|| (runtime.clone(), model.clone()));
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
                    withdrawn: false,
                    cancel: None,
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
                withdrawn,
            } => {
                self.busy = false;
                if let Some(s) = session {
                    self.session = Some(s.clone());
                }
                // A session of its own began: it runs on what that turn ran on. A turn that
                // brought no session leaves the old session and what it ran on.
                if let Some((r, m)) = self.lead_pending.take() {
                    if session.is_some() {
                        self.lead_runtime = r;
                        self.lead_model = m;
                    }
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
                    t.withdrawn = *withdrawn;
                    t.cancel = None;
                    // The edits that went with a withdrawn message go with the next one.
                    if *withdrawn {
                        // A session that was never saved has no runtime or model either.
                        if self.session.is_none() {
                            self.lead_runtime = None;
                            self.lead_model = None;
                        }
                        for e in &t.edits {
                            if !self.pending_edits.iter().any(|p| p.path == e.path) {
                                self.pending_edits.push(e.clone());
                            }
                        }
                    }
                }
            }
            Event::CancelAsked { n, head, kept } => {
                if let Some(t) = self.turns.iter_mut().find(|t| t.n == *n) {
                    t.cancel = Some(Cancel {
                        head: head.clone(),
                        kept: kept.clone(),
                    });
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
            Event::DocEdited { path, diff, .. } => {
                if !diff.is_empty() {
                    self.pending_edits.retain(|e| &e.path != path);
                    self.pending_edits.push(DocEdit {
                        path: path.clone(),
                        diff: diff.clone(),
                    });
                }
            }
            Event::WorkdirSet { workdir } => self.workdir = workdir.clone(),
            Event::LeadSet { lead } => self.lead = Some(lead.clone()),
        }
    }

    /// The lead agent's name (009): `lead` for a session that never named one.
    pub fn lead_name(&self) -> &str {
        self.lead.as_deref().unwrap_or("lead")
    }

    /// Whether the session's lead can no longer change (FR-015): a message was sent and not
    /// withdrawn, whether it is still being answered, was answered, or failed.
    pub fn lead_fixed(&self) -> bool {
        self.turns.iter().any(|t| !t.withdrawn)
    }

    /// The running turn's cancel, if the engineer asked for one.
    pub fn cancelling(&self) -> Option<&Cancel> {
        if !self.busy {
            return None;
        }
        self.turns.last().and_then(|t| t.cancel.as_ref())
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
                    lead: None,
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
                    runtime: None,
                    model: None,
                    new_session: false,
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
                    withdrawn: false,
                },
            ),
            logged(
                6,
                Event::ModeSet {
                    mode: Mode::ReadOnly,
                },
            ),
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
            lead: None,
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

    fn started(seq: u64, n: u32, new_session: bool, model: &str) -> Logged<Event> {
        logged(
            seq,
            Event::TurnStarted {
                n,
                said: "x".into(),
                edits: vec![],
                mode: Mode::Ask,
                session: "s".into(),
                runtime: Some("claude".into()),
                model: Some(model.into()),
                new_session,
            },
        )
    }

    fn finished(seq: u64, n: u32, session: Option<&str>, withdrawn: bool) -> Logged<Event> {
        logged(
            seq,
            Event::TurnFinished {
                n,
                reply: String::new(),
                result: None,
                session: session.map(str::to_string),
                documents_changed: vec![],
                commit: None,
                discarded: vec![],
                cost_usd: None,
                failure: None,
                withdrawn,
            },
        )
    }

    #[test]
    fn a_session_keeps_its_lead_and_what_it_ran_on() {
        // A log from before 009: no lead field at all.
        let old: Event = serde_json::from_str(
            r#"{"type":"conversation_created","kind":"spec","project":"p","feature":null,"forked_from":null,"workdir":"w"}"#,
        )
        .unwrap();
        let s = State::fold("c", &[logged(1, old.clone())]).unwrap();
        assert_eq!(s.lead_name(), "lead");
        assert!(!s.lead_fixed());

        let mut ev = vec![
            logged(1, old),
            logged(
                2,
                Event::LeadSet {
                    lead: "careful".into(),
                },
            ),
        ];
        let s = State::fold("c", &ev).unwrap();
        assert_eq!(s.lead_name(), "careful");

        // A first turn cancelled before it saved a session: withdrawn, nothing kept.
        ev.push(started(3, 1, true, "opus"));
        let s = State::fold("c", &ev).unwrap();
        assert!(s.lead_fixed(), "fixed while the first message is answered");
        ev.push(finished(4, 1, None, true));
        let s = State::fold("c", &ev).unwrap();
        assert!(!s.lead_fixed());
        assert_eq!(s.lead_model, None);

        // A first turn that failed still fixes the lead.
        ev.push(started(5, 2, true, "opus"));
        ev.push(finished(6, 2, Some("s1"), false));
        let s = State::fold("c", &ev).unwrap();
        assert!(s.lead_fixed());
        assert_eq!(s.lead_model.as_deref(), Some("opus"));
        assert_eq!(s.lead_runtime.as_deref(), Some("claude"));

        // A resumed turn keeps what the session began on; a new session replaces it.
        ev.push(started(7, 3, false, "sonnet"));
        let s = State::fold("c", &ev).unwrap();
        assert_eq!(s.lead_model.as_deref(), Some("opus"));
        ev.push(finished(8, 3, Some("s1"), false));
        ev.push(started(9, 4, true, "sonnet"));
        let s = State::fold("c", &ev).unwrap();
        assert_eq!(
            s.lead_model.as_deref(),
            Some("opus"),
            "not before it has a session"
        );
        // A new session that failed without one: the old session and model stay.
        ev.push(finished(10, 4, None, false));
        let s = State::fold("c", &ev).unwrap();
        assert_eq!(s.lead_model.as_deref(), Some("opus"));
        ev.push(started(11, 5, true, "sonnet"));
        ev.push(finished(12, 5, Some("s2"), false));
        let s = State::fold("c", &ev).unwrap();
        assert_eq!(s.lead_model.as_deref(), Some("sonnet"));
    }
}
