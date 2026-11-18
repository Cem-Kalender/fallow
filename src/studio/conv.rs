//! A conversation with the lead in memory (007 research S-5 to S-7, FR-005 to FR-007a): its log
//! and state, its live reply, the engineer's document edits waiting for the next turn, and the
//! command waiting for the engineer's approval.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::Activity;
use crate::conversation::events::{Event, State};
use crate::run::engine::Reply;
use crate::run::events::Log;

/// A command the lead asked to run, waiting for the engineer (research S-7).
pub struct Pending {
    pub id: String,
    /// The runtime's own name for the tool, for the record.
    pub tool: String,
    /// The command asked about (012: from the contract's `PermissionRequest`).
    pub command: String,
    /// 012 (FR-043): a read put to the engineer (the contract's `read` kind), not a command.
    pub read: bool,
    /// The runtime's permission prompt as it came, for its answer.
    pub raw: Value,
    /// The runtime that asked, which shapes the answer (IX).
    pub runtime: String,
    /// 012 (FR-016; T037): what a command changing the engineer's folder must show.
    pub facts: Option<serde_json::Value>,
    pub reply: Sender<Reply>,
    pub deadline: Instant,
}

impl Pending {
    /// 012 (FR-043): a read put to the engineer, not a command.
    pub fn is_read(&self) -> bool {
        self.read
    }

    /// The answer to send, in the asking runtime's own form.
    pub fn answer(&self, allow: bool, message: &str) -> Value {
        crate::agent::registry::permission_reply(&self.runtime, allow, message, &self.raw)
    }
}

pub struct Conv {
    pub dir: PathBuf,
    pub log: Log<Event>,
    pub state: State,
    pub stop: Arc<AtomicBool>,
    /// Pages waiting for the next change (long polling).
    pub waiters: Vec<(Instant, Sender<Reply>)>,
    /// The reply as it is written, during a turn.
    pub live: String,
    /// The current (or last) turn's activity: files read, commands run.
    pub activity: Vec<Activity>,
    /// Changes since Fallow started that are not events (partial text, activity lines).
    pub ticks: u64,
    pub pending: Option<Pending>,
    /// Where the lead's working copy stood when its turn began.
    pub head_before: Option<String>,
    /// The token of the running turn's MCP bridge.
    pub token: Option<String>,
    /// The engineer's edited documents as the turn began, put back if it is withdrawn: they
    /// are not committed until a turn ends.
    pub kept: Vec<crate::conversation::events::KeptDoc>,
    /// 015 (research R-1, R-6): Manual or Auto, in memory: at load it is the default mode.
    pub mode: super::mode::ChatMode,
    /// 015 (R-6): said once after a restart, when the chat was in another mode before.
    pub mode_note: Option<String>,
    /// 015 (R-5): one-shot grants from "Allow this once", in memory only.
    pub grants: Vec<super::mode::Grant>,
    /// 015 (R-5): Fallow's messages to the lead, sent when its turn ends (or at once when idle).
    pub lead_notes: Vec<String>,
    /// 015 (R-3): the tool calls of the running turn that Fallow answered itself, so a runtime's
    /// own refusal is told from Fallow's.
    pub answered: Vec<String>,
    /// 015 (FR-010): the running turn's tool calls whose output Fallow hides (a read or search
    /// of a file that may hold secrets, as decided when the request came), shared with the
    /// turn's thread.
    pub hidden: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// 012 (R-9): after a restart, waiting messages wait for the engineer.
    pub queue_hold: bool,
    /// 012 (R-9): the engineer said something since the last failed turn: waiting messages go.
    pub queue_go: bool,
    /// 012 (FR-027): why the next message waits (no agent slot free).
    pub slot_wait: Option<String>,
    /// 012 (R-19): the session's working copy or branch is gone: it can only be closed.
    pub gone: Option<String>,
    /// 012 (R-19): set when the turn ends, so the thread recording its process group never
    /// leaves `turn-<n>.pid` behind it (fourth review, 6).
    pub turn_ended: Arc<AtomicBool>,
    /// 012 (R-12): the session's gates are running on a thread.
    pub gates_running: bool,
    /// 012 (R-12): why the last gate run could not run.
    pub gates_error: Option<String>,
    /// 012 (R-12): gate results at a base commit, the baseline of later runs, per commit.
    pub gate_baselines: std::collections::BTreeMap<String, Vec<crate::verify::gates::GateResult>>,
    /// 012 (R-4): the engineer opened the session's copy in an editor while a turn worked.
    pub editor_during_turn: bool,
    /// 012 (R-5): the starting branch's commit when the running turn began.
    pub start_before: Option<String>,
    /// 012 (R-10): the session's git command Fallow allowed, until its result.
    pub open_call: Option<super::refwatch::OpenCall>,
    /// The session was made within this very request: its working copy is its start commit,
    /// untouched, on its branch, so its first turn skips the git checks of an older session
    /// (issue #2: each git process costs about 0.1 s on Windows).
    pub fresh: Option<Fresh>,
    /// What a session's view would read from git, as its creation knew it (issue #2): its squad
    /// file and that nothing in its copy changed yet; for its first view only, while nothing
    /// happened in the session since and for a second at most.
    pub primed: Option<Primed>,
}

/// What session start read anyway, for the session's first turn (issue #2).
pub struct Fresh {
    /// The squad file at the project's `HEAD`.
    pub squad: crate::config::Squad,
}

/// A session view's git values as its creation knew them (issue #2).
pub struct Primed {
    /// The session's log sequence it is valid for.
    pub seq: u64,
    pub at: std::time::Instant,
    /// The squad file declares gates.
    pub declared: bool,
}

impl Conv {
    pub fn new(dir: PathBuf, log: Log<Event>, state: State) -> Conv {
        Conv {
            dir,
            log,
            state,
            stop: Arc::new(AtomicBool::new(false)),
            waiters: Vec::new(),
            live: String::new(),
            activity: Vec::new(),
            ticks: 0,
            pending: None,
            head_before: None,
            token: None,
            kept: Vec::new(),
            mode: super::mode::ChatMode::Manual,
            mode_note: None,
            grants: Vec::new(),
            lead_notes: Vec::new(),
            answered: Vec::new(),
            hidden: Default::default(),
            queue_hold: false,
            queue_go: false,
            slot_wait: None,
            fresh: None,
            primed: None,
            gone: None,
            turn_ended: Arc::new(AtomicBool::new(false)),
            gates_running: false,
            gates_error: None,
            gate_baselines: std::collections::BTreeMap::new(),
            editor_during_turn: false,
            start_before: None,
            open_call: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.state.id
    }

    /// Writes an event, applies it, and wakes the pages waiting.
    pub fn append(&mut self, event: Event) -> Result<(), String> {
        let logged = self.log.append(event)?;
        self.state.apply(&logged);
        self.wake();
        Ok(())
    }

    /// One number that grows with every change, for the page's long poll.
    pub fn mark(&self) -> u64 {
        self.state.seq + self.ticks
    }

    /// Something changed that is not an event.
    pub fn tick(&mut self) {
        self.ticks += 1;
        self.wake();
    }

    pub fn wake(&mut self) {
        let m = json!({ "seq": self.mark() });
        for (_, w) in self.waiters.drain(..) {
            let _ = w.send(Reply::Ok(m.clone()));
        }
    }

    pub fn prune(&mut self) {
        self.waiters
            .retain(|(since, _)| since.elapsed() < Duration::from_secs(60));
    }

    pub fn turns_dir(&self) -> PathBuf {
        self.dir.join("turns")
    }

    /// The conversation as the page shows it.
    pub fn view(&self) -> Value {
        let mut v = serde_json::to_value(&self.state).unwrap_or(Value::Null);
        v["live"] = json!(self.live);
        // 015: the chat's mode and its refusals' states as the page shows them (data-model
        // "Views"); an allowed refusal whose grant is gone (a restart) is expired.
        v["mode"] = json!(self.mode.name());
        v["mode_note"] = json!(self.mode_note);
        if let Some(turns) = v["turns"].as_array_mut() {
            for t in turns {
                if let Some(rs) = t["refusals"].as_array_mut() {
                    for r in rs {
                        let id = r["id"].as_str().unwrap_or_default();
                        if r["state"] == "allowed" && !self.grants.iter().any(|g| g.refusal == id) {
                            r["state"] = json!("expired");
                        }
                    }
                }
            }
        }
        v["activity"] = json!(self.activity);
        v["mark"] = json!(self.mark());
        v["cancelling"] = json!(self.state.cancelling().is_some());
        v["pending_edits"] = json!(
            self.state
                .pending_edits
                .iter()
                .map(|e| &e.path)
                .collect::<Vec<_>>()
        );
        v["pending_approval"] = match &self.pending {
            Some(p) => {
                let command = if p.command.is_empty() {
                    &p.tool
                } else {
                    &p.command
                };
                // What "always in this project" could allow, or why it cannot (008 T040). A
                // read (012, FR-043) is allowed once at a time.
                let read = p.is_read();
                let (always, why) = if read {
                    (None, Some("a read is allowed once at a time"))
                } else {
                    match crate::project::commands::always_prefix(command) {
                        Ok(pre) => (Some(pre), None),
                        Err(why) => (None, Some(why)),
                    }
                };
                json!({ "id": p.id, "command": command, "always": always, "always_not": why,
                        "kind": if read { "read" } else { "run" }, "facts": p.facts })
            }
            None => Value::Null,
        };
        v
    }
}

/// A short diff of `old` to `new`: the changed lines, with what is the same before and after
/// left out.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let mut end_a = a.len();
    let mut end_b = b.len();
    while end_a > start && end_b > start && a[end_a - 1] == b[end_b - 1] {
        end_a -= 1;
        end_b -= 1;
    }
    let mut out = format!("@@ line {}\n", start + 1);
    for l in &a[start..end_a] {
        out.push('-');
        out.push_str(l);
        out.push('\n');
    }
    for l in &b[start..end_b] {
        out.push('+');
        out.push_str(l);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_shows_only_the_changed_lines() {
        let d = diff("a\nb\nc\nd\n", "a\nB\nc\nd\n");
        assert_eq!(d, "@@ line 2\n-b\n+B\n");
        let d = diff("", "x\n");
        assert_eq!(d, "@@ line 1\n+x\n");
    }
}
