//! A conversation's events (007 contracts/storage.md, data-model.md Conversation): the project
//! conversation or a spec session, kept as an append-only log like a run's, and its fold.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::settings::ChatMode;
use crate::run::events::{Log, Logged};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Spec,
    /// 012: a solo session with an assistant.
    Solo,
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
        /// 012: the assistant a solo session talks to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        /// 012 (FR-034): the solo session a feature's spec session was started from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
    TurnStarted {
        n: u32,
        said: String,
        edits: Vec<DocEdit>,
        /// The chat's mode when the turn began: `manual` or `auto` (015); older logs hold the
        /// modes of 007 and 012 (`ask`, `run_anything`, …), kept as written.
        #[serde(default)]
        mode: String,
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
        /// 012 (research R-7): the project's instruction files handed with this turn, each
        /// with its fingerprint, so a change is seen at the next turn. Empty when none were
        /// handed (they had not changed).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        instructions: Vec<InstructionRec>,
        /// 012: the images sent with the message.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<super::solo::AttachmentRec>,
        /// 012 (FR-027): how long the message waited for a free agent slot.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot_waited_ms: Option<u64>,
        /// 012: the session's head as the turn began, so a restart knows the turn's commits.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head: Option<String>,
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
        /// 012 (R-3): every commit the session's turn added, the assistant's own included.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        commits: Vec<String>,
        /// 012: why Fallow's commit at the end of the turn failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit_failed: Option<String>,
        /// 012 (R-4): the paths the turn left, with their content, when its commit failed.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        changed: Vec<super::solo::ChangedRec>,
        /// 012: the turn was cut short (stopped, or Fallow restarted).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cut: bool,
        /// 012: this turn's commit also holds turn `folded_into`'s changes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folded_into: Option<u32>,
        /// The runtime's running total for `session` after the turn (`Outcome.session_total_usd`),
        /// handed back when the session is resumed or forked; none for a runtime that reports
        /// each call's own cost.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_total_usd: Option<f64>,
        /// `cost_usd` is the turn's own cost. False in turns recorded before 2026-11-25, whose
        /// `cost_usd` holds Claude Code's running total as it came: see `State::turn_cost`.
        #[serde(default)]
        own_cost: bool,
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
        /// 012 (R-6): why the judge asks the engineer, per command.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reasons: Vec<String>,
        /// 012 (FR-016): what a merge would bring, and the engineer's folder.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merge: Option<Value>,
    },
    ApprovalAnswered {
        id: String,
        answer: ApprovalAnswer,
        prefix: Option<String>,
        /// 012: allowed by the judge under "until I switch back", not by the engineer.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto: bool,
        /// 012: why it was answered without the engineer ("Fallow restarted").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    /// 007's mode of a lead's chat (`read_only`, `approved`, `ask`, `run_anything`): read and
    /// ignored since 015, which starts every chat in the default mode (research R-6).
    ModeSet {
        #[serde(default)]
        mode: String,
    },
    /// 015: the engineer changed the chat's mode; for the chat's notes, never to restore it.
    ChatMode { mode: ChatMode },
    /// 015 (research R-4): Fallow refused an agent's request at a fixed limit, in either mode.
    Refused {
        id: String,
        /// `edit`, `run` or `tool`.
        kind: String,
        /// The path, command or tool name.
        what: String,
        reason: String,
        /// `edit`, `target`, `tool` or `control` (never allowed once).
        limit: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facts: Option<Value>,
    },
    /// 015 (R-5): the engineer allowed a refusal once.
    RefusalAllowed { id: String },
    /// 015 (R-5): the grant of an allowed refusal was used by the agent's request.
    RefusalUsed { id: String },
    /// 015: the engineer dismissed a refusal.
    RefusalDismissed { id: String },
    /// 015 (review of US2, 2): an allowed refusal whose agent could not be told: refused again,
    /// with why.
    RefusalReopened { id: String, why: String },
    /// The Spec Kit step a spec session is at (FR-006).
    StepSet { step: String },
    DocEdited {
        path: String,
        version: u64,
        /// The edit since the lead last saw the document, for its next turn (kept so it
        /// survives a restart; Phase 3 review).
        #[serde(default)]
        diff: String,
    },
    /// The lead works in another place from now on (a plain folder that got git).
    WorkdirSet { workdir: PathBuf },
    /// The engineer picked another lead before the session's first message (009).
    LeadSet { lead: String },
    /// 012: a solo session's own events (conversation/solo.rs), each with its own `type`.
    #[serde(untagged)]
    Solo(super::solo::SoloEvent),
}

/// One turn of a conversation, folded.
#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub n: u32,
    pub said: String,
    pub edits: Vec<DocEdit>,
    pub mode: String,
    pub started: String,
    pub reply: Option<String>,
    pub result: Option<Value>,
    pub documents_changed: Vec<String>,
    pub commit: Option<String>,
    pub discarded: Vec<String>,
    pub cost_usd: Option<f64>,
    pub failure: Option<String>,
    pub approvals: Vec<Approval>,
    /// 015: the requests Fallow refused at a fixed limit during the turn.
    pub refusals: Vec<Refusal>,
    /// 012: the instruction files handed with this turn (none when unchanged).
    pub instructions: Vec<InstructionRec>,
    /// 012: the images sent with the message.
    pub attachments: Vec<super::solo::AttachmentRec>,
    /// 012: the turn was cut short (stopped, or Fallow restarted).
    pub cut: bool,
    /// 012: the session's head as the turn began.
    pub head: Option<String>,
    /// 012: why Fallow's commit at the end of the turn failed.
    pub commit_failed: Option<String>,
    /// Cancelled by the engineer: the message was withdrawn.
    pub withdrawn: bool,
    /// The cancel asked while the turn ran, until it ends.
    #[serde(skip)]
    pub cancel: Option<Cancel>,
}

/// 012: an instruction file as handed to an agent: its path, its fingerprint, whether it was
/// cut, and why it was not given, if it was not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionRec {
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Approval {
    pub id: String,
    pub command: String,
    pub answer: Option<ApprovalAnswer>,
    pub prefix: Option<String>,
    /// 012: why the judge asks the engineer, per command.
    pub reasons: Vec<String>,
}

/// 015 (FR-006): the engineer changed the chat's mode after turn `after`, for the chat's notes.
#[derive(Debug, Clone, Serialize)]
pub struct ModeChange {
    pub after: u32,
    pub mode: ChatMode,
    pub at: String,
}

/// 015 (data-model "Refusal states").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalState {
    Refused,
    /// Allowed once; the grant waits for the agent's request (in memory: a restart expires it).
    Allowed,
    Used,
    Dismissed,
}

/// 015 (R-4): a request Fallow refused at a fixed limit.
#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub id: String,
    pub kind: String,
    pub what: String,
    pub reason: String,
    pub limit: String,
    pub facts: Option<Value>,
    pub state: RefusalState,
    /// Why an allowed refusal is refused again (its agent could not be told).
    pub note: Option<String>,
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
    /// 015: the engineer's mode changes, in order.
    pub mode_changes: Vec<ModeChange>,
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
    /// 012: the assistant of a solo session.
    pub agent: Option<String>,
    /// 012 (FR-034): the solo session a spec session was started from.
    pub origin: Option<String>,
    /// 012: a solo session's own state.
    pub solo: Option<super::solo::Solo>,
    /// The runtime's last running total per agent session, for `Call.session_total_usd`.
    #[serde(skip)]
    pub session_totals: std::collections::HashMap<String, f64>,
    /// The running turn resumes its session (not a new one or a fork).
    #[serde(skip)]
    pub resuming: bool,
}

impl State {
    /// What to hand a call as `Call.session_total_usd` for `session`.
    pub fn session_total(&self, session: &crate::agent::Session) -> Option<f64> {
        match session {
            crate::agent::Session::New(_) => None,
            s => self.session_totals.get(s.id()).copied(),
        }
    }

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
            agent,
            origin,
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
            mode_changes: Vec::new(),
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
            agent: agent.clone(),
            origin: origin.clone(),
            solo: (*kind == Kind::Solo).then(super::solo::Solo::default),
            session_totals: Default::default(),
            resuming: false,
        };
        for l in &events[1..] {
            s.apply(l);
        }
        Ok(s)
    }

    /// The turn's own cost, and the session's running total kept for the next call.
    ///
    /// A turn recorded with `own_cost` already holds its own cost. One recorded before holds
    /// what the runtime reported, which for Claude Code was the
    /// session's running total. It is corrected only where it is clearly a running total: the
    /// turn resumed its session (not a new session or a fork), an earlier turn of that same
    /// session reported a total, and this one is at least that total. Then the turn cost the
    /// difference. Anything else (a first turn, a fork, a total that went down) is taken as
    /// the turn's own cost, as it was shown before, so no record is ever made smaller than a
    /// difference of two totals the runtime itself reported.
    fn turn_cost(
        &mut self,
        session: Option<&str>,
        cost: Option<f64>,
        total: Option<f64>,
        own: bool,
    ) -> Option<f64> {
        let Some(s) = session else {
            return cost;
        };
        if own {
            if let Some(t) = total {
                self.session_totals.insert(s.to_string(), t);
            }
            return cost;
        }
        let c = cost?;
        let before = self
            .resuming
            .then(|| self.session_totals.get(s).copied())
            .flatten();
        self.session_totals.insert(s.to_string(), c);
        Some(crate::agent::turn_cost(c, before))
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
                instructions,
                attachments,
                head,
                ..
            } => {
                self.busy = true;
                self.resuming = !*new_session;
                self.pending_edits.clear();
                self.lead_pending = new_session.then(|| (runtime.clone(), model.clone()));
                self.turns.push(Turn {
                    n: *n,
                    said: said.clone(),
                    edits: edits.clone(),
                    mode: mode.clone(),
                    started: l.at.clone(),
                    reply: None,
                    result: None,
                    documents_changed: Vec::new(),
                    commit: None,
                    discarded: Vec::new(),
                    cost_usd: None,
                    failure: None,
                    approvals: Vec::new(),
                    refusals: Vec::new(),
                    instructions: instructions.clone(),
                    attachments: attachments.clone(),
                    cut: false,
                    head: head.clone(),
                    commit_failed: None,
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
                commits,
                commit_failed,
                changed,
                cut,
                folded_into,
                session_total_usd,
                own_cost,
            } => {
                self.busy = false;
                let cost_usd =
                    &self.turn_cost(session.as_deref(), *cost_usd, *session_total_usd, *own_cost);
                if let Some(solo) = self.solo.as_mut() {
                    solo.turn_finished(
                        *n,
                        super::solo::TurnCommits {
                            commit: commit.clone(),
                            commits: commits.clone(),
                            failed: commit_failed.clone(),
                            changed: changed.clone(),
                            cut: *cut,
                            folded_into: *folded_into,
                        },
                    );
                }
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
                    t.cut = *cut;
                    t.commit_failed = commit_failed.clone();
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
            Event::ApprovalAsked {
                id,
                command,
                reasons,
                ..
            } => {
                if let Some(t) = self.turns.last_mut() {
                    t.approvals.push(Approval {
                        id: id.clone(),
                        command: command.clone(),
                        answer: None,
                        prefix: None,
                        reasons: reasons.clone(),
                    });
                }
            }
            Event::ApprovalAnswered {
                id, answer, prefix, ..
            } => {
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
            Event::ModeSet { .. } => {}
            Event::ChatMode { mode } => self.mode_changes.push(ModeChange {
                after: self.turns.len() as u32,
                mode: *mode,
                at: l.at.clone(),
            }),
            Event::Refused {
                id,
                kind,
                what,
                reason,
                limit,
                facts,
            } => {
                if let Some(t) = self.turns.last_mut() {
                    t.refusals.push(Refusal {
                        id: id.clone(),
                        kind: kind.clone(),
                        what: what.clone(),
                        reason: reason.clone(),
                        limit: limit.clone(),
                        facts: facts.clone(),
                        state: RefusalState::Refused,
                        note: None,
                    });
                }
            }
            Event::RefusalAllowed { id } => self.refusal_to(id, RefusalState::Allowed),
            Event::RefusalUsed { id } => self.refusal_to(id, RefusalState::Used),
            Event::RefusalDismissed { id } => self.refusal_to(id, RefusalState::Dismissed),
            Event::RefusalReopened { id, why } => {
                self.refusal_to(id, RefusalState::Refused);
                if let Some(r) = self
                    .turns
                    .iter_mut()
                    .flat_map(|t| t.refusals.iter_mut())
                    .find(|r| &r.id == id)
                {
                    r.note = Some(why.clone());
                }
            }
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
            Event::Solo(e) => {
                if let Some(solo) = self.solo.as_mut() {
                    solo.apply(e);
                }
                // A suggestion is also the turn's (FR-030).
            }
        }
    }

    fn refusal_to(&mut self, id: &str, to: RefusalState) {
        if let Some(r) = self
            .turns
            .iter_mut()
            .flat_map(|t| t.refusals.iter_mut())
            .find(|r| r.id == id)
        {
            r.state = to;
        }
    }

    /// 015: a refusal by its id.
    pub fn refusal(&self, id: &str) -> Option<&Refusal> {
        self.turns
            .iter()
            .flat_map(|t| t.refusals.iter())
            .find(|r| r.id == id)
    }

    /// 015: the turn a refusal belongs to.
    pub fn refusal_turn(&self, id: &str) -> Option<&Turn> {
        self.turns
            .iter()
            .find(|t| t.refusals.iter().any(|r| r.id == id))
    }

    /// 015: the next refusal's id (`r1`, `r2`, …).
    pub fn next_refusal_id(&self) -> String {
        let n: usize = self.turns.iter().map(|t| t.refusals.len()).sum();
        format!("r{}", n + 1)
    }

    /// The lead agent's name (009): `lead` for a session that never named one.
    pub fn lead_name(&self) -> &str {
        self.lead.as_deref().unwrap_or("lead")
    }

    /// 012: the instruction files as last handed in this conversation (path → fingerprint).
    pub fn last_instructions(&self) -> Vec<(String, String)> {
        self.turns
            .iter()
            .rev()
            // A withdrawn turn's agent never kept what it was handed (review L2).
            .find(|t| !t.instructions.is_empty() && !t.withdrawn)
            .map(|t| {
                t.instructions
                    .iter()
                    .map(|i| (i.path.clone(), i.sha.clone()))
                    .collect()
            })
            .unwrap_or_default()
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

    /// A conversation's log as JSON lines: a new session, its resumes, then a new session.
    fn costs_log(turns: &[(bool, &str, f64)], own: bool) -> Vec<Logged<Event>> {
        let mut lines = vec![serde_json::json!({"seq": 1, "at": "2026-11-24T17:26:32Z",
            "type": "conversation_created", "kind": "solo", "project": "shop", "feature": null,
            "forked_from": null, "workdir": "w", "lead": null, "agent": "assistant"})];
        for (i, (new, session, cost)) in turns.iter().enumerate() {
            let n = i + 1;
            lines.push(
                serde_json::json!({"seq": 2 * n, "at": "2026-11-24T17:27:00Z",
                "type": "turn_started", "n": n, "said": "hi", "edits": [], "mode": "auto",
                "session": session, "runtime": "claude", "model": "opus", "new_session": new}),
            );
            let mut f = serde_json::json!({"seq": 2 * n + 1, "at": "2026-11-24T17:28:00Z",
                "type": "turn_finished", "n": n, "reply": "ok", "result": null,
                "session": session, "documents_changed": [], "commit": null, "discarded": [],
                "cost_usd": cost, "failure": null});
            if own {
                f["own_cost"] = true.into();
            }
            lines.push(f);
        }
        lines
            .into_iter()
            .map(|l| serde_json::from_value(l).unwrap())
            .collect()
    }

    /// Turns recorded before 2026-11-25 hold Claude Code's running total: on load, a resumed
    /// turn's total at least the session's last is corrected to the difference.
    #[test]
    fn an_old_log_of_running_totals_is_corrected_on_load() {
        let reported = crate::agent::cost_tests::REPORTED;
        let mut turns: Vec<(bool, &str, f64)> = reported
            .iter()
            .enumerate()
            .map(|(i, c)| (i == 0, "s1", *c))
            .collect();
        let s = State::fold("c6571178e", &costs_log(&turns, false)).unwrap();
        assert!((s.cost_usd - 3.0137).abs() < 1e-9, "{}", s.cost_usd);
        assert!((s.turns[1].cost_usd.unwrap() - 2.0577).abs() < 1e-9);
        assert_eq!(s.turns[0].cost_usd, Some(0.5675));
        assert_eq!(s.session_totals.get("s1"), Some(&3.0137));

        // A new session starts from 0, whatever the old one had cost; and a total lower than
        // the session's last is the turn's own cost.
        turns.push((true, "s2", 0.75));
        turns.push((false, "s2", 1.0));
        turns.push((false, "s2", 0.125));
        let s = State::fold("c", &costs_log(&turns, false)).unwrap();
        let t: Vec<f64> = s.turns.iter().map(|t| t.cost_usd.unwrap()).collect();
        assert_eq!(&t[13..], &[0.75, 0.25, 0.125]);
        assert!((s.cost_usd - (3.0137 + 1.125)).abs() < 1e-9);

        // A turn recorded since holds its own cost, as written.
        let s = State::fold("c", &costs_log(&turns, true)).unwrap();
        assert!((s.cost_usd - (34.2825 + 1.875)).abs() < 1e-9);
    }

    #[test]
    fn a_solo_sessions_events_sit_in_the_conversation_log() {
        use crate::conversation::solo::SoloEvent;
        let e = Event::Solo(SoloEvent::SessionCreated {
            branch: "fallow/solo/fix".into(),
            start_branch: Some("main".into()),
            start_commit: "s0".into(),
            worktree: "wt".into(),
            dirty_at_start: false,
            web: true,
            lfs: false,
        });
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["type"], "session_created");
        assert_eq!(serde_json::from_value::<Event>(j).unwrap(), e);
        // A conversation event still reads as itself.
        let m: Event = serde_json::from_str(r#"{"type":"mode_set","mode":"ask"}"#).unwrap();
        assert!(matches!(m, Event::ModeSet { .. }));
        let created = logged(
            1,
            Event::ConversationCreated {
                kind: Kind::Solo,
                project: "shop".into(),
                feature: None,
                forked_from: None,
                workdir: "w".into(),
                lead: None,
                agent: Some("assistant".into()),
                origin: None,
            },
        );
        let s = State::fold("s1", &[created, logged(2, e)]).unwrap();
        assert_eq!(s.solo.as_ref().unwrap().branch, "fallow/solo/fix");
        assert_eq!(s.agent.as_deref(), Some("assistant"));
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
                    agent: None,
                    origin: None,
                },
            ),
            logged(
                2,
                Event::TurnStarted {
                    n: 1,
                    said: "checkout".into(),
                    edits: vec![],
                    mode: "ask".into(),
                    session: "s0".into(),
                    runtime: None,
                    model: None,
                    new_session: false,
                    instructions: Vec::new(),
                    attachments: Vec::new(),
                    slot_waited_ms: None,
                    head: None,
                },
            ),
            logged(
                3,
                Event::ApprovalAsked {
                    id: "a1".into(),
                    command: "pip list".into(),
                    reasons: Vec::new(),
                    merge: None,
                },
            ),
            logged(
                4,
                Event::ApprovalAnswered {
                    id: "a1".into(),
                    answer: ApprovalAnswer::Always,
                    prefix: Some("pip list".into()),
                    auto: false,
                    why: None,
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
                    commits: Vec::new(),
                    commit_failed: None,
                    changed: Vec::new(),
                    cut: false,
                    folded_into: None,
                    session_total_usd: None,
                    own_cost: true,
                },
            ),
            logged(
                6,
                Event::ModeSet {
                    mode: "read_only".into(),
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
            agent: None,
            origin: None,
        })
        .unwrap();
        log.append(Event::ModeSet {
            mode: "run_anything".into(),
        })
        .unwrap();
        drop(log);
        let (_, events, notes) = ConversationLog::open(&d).unwrap();
        assert!(notes.is_empty());
        // 015: an old chat's mode is read and ignored (research R-6).
        let s = State::fold("c1", &events).unwrap();
        assert_eq!(s.turns.len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn started(seq: u64, n: u32, new_session: bool, model: &str) -> Logged<Event> {
        logged(
            seq,
            Event::TurnStarted {
                n,
                said: "x".into(),
                edits: vec![],
                mode: "ask".into(),
                session: "s".into(),
                runtime: Some("claude".into()),
                model: Some(model.into()),
                new_session,
                instructions: Vec::new(),
                attachments: Vec::new(),
                slot_waited_ms: None,
                head: None,
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
                commits: Vec::new(),
                commit_failed: None,
                changed: Vec::new(),
                cut: false,
                folded_into: None,
                session_total_usd: None,
                own_cost: true,
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
