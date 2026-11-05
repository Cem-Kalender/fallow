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
    /// 012 (R-6): "until I switch back", in memory only: a restart reads as "Ask".
    pub allow_until_switch: bool,
    /// 012 (R-9): after a restart, waiting messages wait for the engineer.
    pub queue_hold: bool,
    /// 012 (R-9): the engineer said something since the last failed turn: waiting messages go.
    pub queue_go: bool,
    /// 012 (FR-027): why the next message waits (no agent slot free).
    pub slot_wait: Option<String>,
    /// 012 (R-19): the session's working copy or branch is gone: it can only be closed.
    pub gone: Option<String>,
    /// 012 (FR-008): "until I switch back" was on when Fallow stopped; it is off now.
    pub mode_reset: bool,
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
            allow_until_switch: false,
            queue_hold: false,
            queue_go: false,
            slot_wait: None,
            gone: None,
            mode_reset: false,
            turn_ended: Arc::new(AtomicBool::new(false)),
            gates_running: false,
            gates_error: None,
            gate_baselines: std::collections::BTreeMap::new(),
            editor_during_turn: false,
            start_before: None,
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
                        "kind": if read { "read" } else { "run" } })
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
