//! Signals (005 research R-4, FR-021, FR-022): builders' structured questions and proposed
//! changes to the lead, and the lead's answers that point to commits. Kept in memory only
//! (constitution VII); the decisions themselves live in the repository.

use std::time::Instant;

/// What a builder raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A question the lead decides.
    Question,
    /// A proposed change to the plan, the manifest or a contract, with its diff.
    Change,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "question" => Kind::Question,
            "change" => Kind::Change,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Question => "question",
            Kind::Change => "change",
        }
    }
}

/// The lead's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    /// The commit that records the decision; `None` when the answer changes nothing.
    pub commit: Option<String>,
    /// For a signal Fallow raised about a non-lead change: the lead accepted the work.
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub id: u64,
    /// The raising session; for a signal Fallow raised (R-19), the session whose commit it is
    /// about.
    pub from: u32,
    /// Raised by Fallow itself about a non-lead change to the manifest, docs or a contract.
    pub by_fallow: bool,
    pub project: crate::projects::ProjectId,
    pub kind: Kind,
    pub work: Option<String>,
    pub part: Option<String>,
    pub text: String,
    pub diff: Option<String>,
    /// The commit a Fallow signal is about.
    pub commit: Option<String>,
    pub raised: Instant,
    pub answer: Option<Answer>,
    /// The engineer was told it is unanswered (FR-035).
    pub told_engineer: bool,
}

impl Signal {
    pub fn open(&self) -> bool {
        self.answer.is_none()
    }
}

#[derive(Debug, Default)]
pub struct Signals {
    pub list: Vec<Signal>,
    next: u64,
}

impl Signals {
    pub fn add(&mut self, mut s: Signal) -> u64 {
        self.next += 1;
        s.id = self.next;
        self.list.push(s);
        self.next
    }

    pub fn get(&self, id: u64) -> Option<&Signal> {
        self.list.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Signal> {
        self.list.iter_mut().find(|s| s.id == id)
    }

    /// Whether `session` waits on an open signal it raised.
    pub fn open_from(&self, session: u32) -> bool {
        self.list
            .iter()
            .any(|s| !s.by_fallow && s.from == session && s.open())
    }

    /// Signals raised per session (for run records, R-15).
    pub fn count_by_session(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for s in self.list.iter().filter(|s| !s.by_fallow) {
            match out.iter_mut().find(|(id, _)| *id == s.from) {
                Some(e) => e.1 += 1,
                None => out.push((s.from, 1)),
            }
        }
        out
    }

    /// Open signals of a project.
    pub fn open_in(&self, project: crate::projects::ProjectId) -> impl Iterator<Item = &Signal> {
        self.list
            .iter()
            .filter(move |s| s.project == project && s.open())
    }
}

/// What a signal's text and diff may hold.
pub const TEXT_MAX: usize = 2 * 1024;
pub const DIFF_MAX: usize = 8 * 1024;

impl super::Hub {
    /// A session of a burst raises a signal to its lead (FR-021): a question, or a proposed
    /// change with its diff. Only the lead receives it.
    #[allow(clippy::too_many_arguments)]
    pub fn raise_signal(
        &mut self,
        caller: u32,
        kind: Kind,
        work: Option<String>,
        part: Option<String>,
        text: String,
        diff: Option<String>,
        now: Instant,
    ) -> Result<u64, super::Reply> {
        let Some(b) = self.burst_for(caller) else {
            return Err(super::Reply::error(
                "unavailable",
                "this project is not coordinating",
            ));
        };
        if b.lead == caller {
            return Err(super::Reply::error(
                "invalid",
                "you are the lead: record decisions in the plan, the manifest or a contract",
            ));
        }
        if kind == Kind::Change && diff.as_deref().is_none_or(|d| d.trim().is_empty()) {
            return Err(super::Reply::error(
                "invalid",
                "a change needs its diff (of the plan, the manifest or a contract)",
            ));
        }
        let (project, lead) = (b.project, b.lead);
        let role = b.roles.get(&caller).cloned().unwrap_or_default();
        let assigned = b.assigned.get(&caller).cloned();
        let work = work.or_else(|| assigned.as_ref().map(|a| a.0.clone()));
        let part = part.or_else(|| assigned.and_then(|a| a.1));
        let id = self.signals.add(Signal {
            id: 0,
            from: caller,
            by_fallow: false,
            project,
            kind,
            work: work.clone(),
            part: part.clone(),
            text: text.clone(),
            diff,
            commit: None,
            raised: now,
            answer: None,
            told_engineer: false,
        });
        let at = match (&work, &part) {
            (Some(w), Some(p)) => format!(" ({w}, part {p})"),
            (Some(w), None) => format!(" ({w})"),
            _ => String::new(),
        };
        self.tell_lead(
            project,
            format!(
                "Signal S{id} from #{caller} {role}{at}, a {}: {} Answer with signal_answer; when it changes a decision, commit it to the plan, the manifest or a contract and give the commit.",
                kind.label(),
                super::lead::one_line(&text)
            ),
        );
        let _ = lead;
        Ok(id)
    }

    /// Fallow raises a signal to the lead about a non-lead session's commit that changed the
    /// manifest, the plan documents or a contract (R-19): that work counts once accepted.
    pub fn raise_change_by_other(
        &mut self,
        session: u32,
        commit: &str,
        files: &[String],
        now: Instant,
    ) {
        let Some(b) = self.burst_for(session) else {
            return;
        };
        let (project, role) = (
            b.project,
            b.roles.get(&session).cloned().unwrap_or_default(),
        );
        let known = self
            .signals
            .list
            .iter()
            .any(|s| s.by_fallow && s.from == session && s.commit.as_deref() == Some(commit));
        if known {
            return;
        }
        let text = format!(
            "#{session} {role}'s commit {} changes {}, which only the lead changes",
            &commit[..commit.len().min(10)],
            files.join(", ")
        );
        let id = self.signals.add(Signal {
            id: 0,
            from: session,
            by_fallow: true,
            project,
            kind: Kind::Change,
            work: None,
            part: None,
            text: text.clone(),
            diff: None,
            commit: Some(commit.to_string()),
            raised: now,
            answer: None,
            told_engineer: false,
        });
        self.alerts
            .push(format!("{text} (signal S{id} to the lead)"));
        self.tell_lead(
            project,
            format!(
                "Signal S{id} from Fallow: {text}. Its work does not count until you accept it with signal_answer and accept = true, or supersede it with a commit of your own."
            ),
        );
    }

    /// The lead answers a signal (FR-022): a commit that records the decision, or a reply that
    /// changes nothing. `accept` accepts the work behind a Fallow signal (R-19).
    #[allow(clippy::too_many_arguments)]
    pub fn answer_signal(
        &mut self,
        sessions: &dyn super::Sessions,
        caller: u32,
        id: u64,
        text: String,
        commit: Option<String>,
        accept: bool,
        now: Instant,
    ) -> Result<serde_json::Value, super::Reply> {
        let Some(sig) = self.signals.get(id).cloned() else {
            return Err(super::Reply::error("not_found", format!("no signal S{id}")));
        };
        let Some(b) = self.burst_of(sig.project) else {
            return Err(super::Reply::error(
                "unavailable",
                "that coordination has ended",
            ));
        };
        if b.lead != caller {
            return Err(super::Reply::error(
                "not_allowed",
                "only the lead answers signals",
            ));
        }
        if !sig.open() {
            return Err(super::Reply::error(
                "invalid",
                format!("S{id} is already answered"),
            ));
        }
        if accept && !sig.by_fallow {
            return Err(super::Reply::error(
                "invalid",
                "accept is for signals Fallow raised about a change by another session",
            ));
        }
        // A commit must be the lead's and change a decision file (FR-022).
        let mut checked = None;
        if let Some(c) = &commit {
            let decision: Vec<String> = {
                let mut v = vec![super::manifest::WORK_FILE.to_string()];
                if let Some(m) = &b.manifest {
                    v.extend(m.docs.iter().cloned());
                    v.extend(m.contracts.iter().map(|x| x.path.clone()));
                }
                v
            };
            if let Some((dir, branch)) = sessions.workdir(caller) {
                let at = branch.as_deref().unwrap_or("HEAD");
                if !super::git::commit_exists(&dir, c) || !super::git::on_branch(&dir, c, at) {
                    return Err(super::Reply::error(
                        "invalid",
                        format!(
                            "commit {c} is not on your branch {at}: commit the decision there first"
                        ),
                    ));
                }
                let parent = format!("{c}^");
                let files = super::git::files_between(&dir, &parent, c).unwrap_or_default();
                let decided: Vec<&String> = files.iter().filter(|f| decision.contains(f)).collect();
                if decided.is_empty() {
                    return Err(super::Reply::error(
                        "invalid",
                        format!(
                            "commit {c} changes no decision file: the plan documents, .fallow/work.toml or a contract"
                        ),
                    ));
                }
                checked = Some(
                    decided
                        .iter()
                        .map(|f| (*f).clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
        }
        if let Some(s) = self.signals.get_mut(id) {
            s.answer = Some(Answer {
                text: text.clone(),
                commit: commit.clone(),
                accepted: accept,
            });
        }
        if !sig.by_fallow {
            let take = commit.as_ref().map_or(String::new(), |c| {
                format!(
                    " The decision is in commit {c}: merge it into your branch before you report."
                )
            });
            let reply = format!(
                "Answer to your signal S{id} from the lead: {}{take} Carry on with the work it does not block.",
                super::lead::one_line(&text)
            );
            let _ = self.request_delivery(
                sessions,
                super::Party::Fallow,
                sig.from,
                reply,
                super::delivery::Purpose::Message,
                true,
                now,
            );
        }
        Ok(serde_json::json!({ "answered": id, "commit": commit, "decision_files": checked }))
    }

    /// Open signals unanswered past the squad's limit are told to the engineer and the lead
    /// once (FR-035).
    pub(crate) fn tick_signals(&mut self, now: Instant) -> bool {
        let mut late = Vec::new();
        for s in self
            .signals
            .list
            .iter_mut()
            .filter(|s| s.open() && !s.told_engineer)
        {
            let Some(b) = self.bursts.iter().find(|b| b.project == s.project) else {
                continue;
            };
            let limit =
                std::time::Duration::from_secs(u64::from(b.squad.signal_unanswered_min) * 60);
            if now.saturating_duration_since(s.raised) >= limit {
                s.told_engineer = true;
                late.push((
                    s.project,
                    s.id,
                    b.project_name.clone(),
                    b.squad.signal_unanswered_min,
                ));
            }
        }
        let any = !late.is_empty();
        for (project, id, name, min) in late {
            self.alerts.push(format!(
                "{name}: signal S{id} to the lead has waited {min} min without an answer (the squad waits on the lead)"
            ));
            self.tell_lead(
                project,
                format!("Signal S{id} has waited {min} min for your answer; the session that raised it may be blocked."),
            );
        }
        any
    }
}
