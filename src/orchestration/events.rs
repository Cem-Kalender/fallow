//! Events, waits and interests (003 US2, data-model.md "Event", research R-4).
//!
//! - **Signals are sticky**: once an event is signalled, a later wait ends at once. Signalling
//!   again replaces the sender, time and message.
//! - **Waits are parked replies**: a wait answers on the signal, at the agent's own time limit
//!   ("timed_out"), or at the block limit ("still_waiting", the agent calls again). The start time
//!   survives such repeats, for the overview.
//! - **Interests**: with `wake_when_idle`, a session asks to be woken if the event arrives while
//!   it is not waiting (US2 scenario 5).

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::Party;
use super::board::BoardId;
use super::channel::ConnId;

/// Longest signal message (data-model.md "Limits").
pub const MESSAGE_MAX: usize = 2 * 1024;
/// Events per scope.
pub const PER_SCOPE: usize = 500;
/// Interests per session.
pub const INTERESTS_MAX: usize = 16;
/// Progress notifications while a wait is parked (research R-4).
pub const PROGRESS_EVERY: Duration = Duration::from_secs(15);

/// An event's key: its scope (a project or shared, as boards) and name.
pub type Key = (BoardId, String);

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub from: Party,
    pub at: SystemTime,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub signal: Option<Signal>,
}

/// A wait in progress: a parked tool call.
#[derive(Debug, Clone)]
pub struct Wait {
    pub session: u32,
    pub key: Key,
    /// When the session started waiting for this event (kept across repeats).
    pub since: Instant,
    /// The agent's own time limit.
    pub deadline: Option<Instant>,
    /// When this call answers "still_waiting".
    pub block_until: Instant,
    pub conn: Option<ConnId>,
    pub id: Value,
    pub progress: Option<Value>,
    pub last_progress: Instant,
}

/// What ended a wait.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitEnd {
    Signalled(Signal),
    TimedOut,
    StillWaiting(Duration),
}

#[derive(Debug, Default)]
pub struct Events {
    pub map: HashMap<Key, Event>,
    pub waits: Vec<Wait>,
    /// The last wait per session, for the overview between repeats: (key, since, last answer).
    pub recent: HashMap<u32, (Key, Instant, Instant)>,
    /// Sessions that asked to be woken: session → keys.
    pub interests: HashMap<u32, Vec<Key>>,
}

/// Checks an event name: `[A-Za-z0-9._:/#-]{1,64}` plus `_`.
pub fn valid_name(n: &str) -> bool {
    (1..=64).contains(&n.len())
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/#-_".contains(&b))
}

impl Events {
    /// Creates the event if needed. `Err` when the scope already holds `PER_SCOPE` events and
    /// none of them is signalled (the oldest signalled one makes room otherwise).
    pub fn ensure(&mut self, key: &Key) -> Result<&mut Event, String> {
        if !self.map.contains_key(key) {
            let in_scope = self.map.keys().filter(|(s, _)| *s == key.0).count();
            if in_scope >= PER_SCOPE {
                let waited: Vec<&Key> = self.waits.iter().map(|w| &w.key).collect();
                let oldest = self
                    .map
                    .iter()
                    .filter(|(k, e)| k.0 == key.0 && e.signal.is_some() && !waited.contains(k))
                    .min_by_key(|(_, e)| e.signal.as_ref().map(|s| s.at))
                    .map(|(k, _)| k.clone());
                match oldest {
                    Some(k) => {
                        self.map.remove(&k);
                    }
                    None => {
                        return Err(format!(
                            "{PER_SCOPE} events exist in this scope and none is signalled"
                        ));
                    }
                }
            }
        }
        Ok(self.map.entry(key.clone()).or_default())
    }

    /// Signals `key`. Returns the waits it ends (removed) and the sessions with an interest that
    /// are not waiting for it (wake candidates; their interest is consumed).
    pub fn signal(&mut self, key: &Key, s: Signal) -> Result<(Vec<Wait>, Vec<u32>), String> {
        self.ensure(key)?.signal = Some(s);
        let (ended, rest): (Vec<Wait>, Vec<Wait>) =
            self.waits.drain(..).partition(|w| &w.key == key);
        self.waits = rest;
        for w in &ended {
            self.recent.remove(&w.session);
        }
        let mut wake = Vec::new();
        for (session, keys) in &mut self.interests {
            if let Some(i) = keys.iter().position(|k| k == key) {
                keys.remove(i);
                if !ended.iter().any(|w| w.session == *session) {
                    wake.push(*session);
                }
            }
        }
        wake.sort_unstable();
        Ok((ended, wake))
    }

    pub fn interest(&mut self, session: u32, key: &Key) {
        let v = self.interests.entry(session).or_default();
        if !v.contains(key) {
            if v.len() >= INTERESTS_MAX {
                v.remove(0);
            }
            v.push(key.clone());
        }
    }

    /// The event a session waits for now (or answered "still_waiting" for in the last 5 s), and
    /// since when.
    pub fn waiting(&self, session: u32, now: Instant) -> Option<(&str, Instant)> {
        if let Some(w) = self.waits.iter().find(|w| w.session == session) {
            return Some((w.key.1.as_str(), w.since));
        }
        self.recent.get(&session).and_then(|(k, since, last)| {
            (now.saturating_duration_since(*last) < Duration::from_secs(5))
                .then_some((k.1.as_str(), *since))
        })
    }

    /// Waits whose deadline or block limit passed, removed. `still_waiting` ones remember their
    /// start for the repeat.
    pub fn due(&mut self, now: Instant) -> Vec<(Wait, WaitEnd)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.waits.len() {
            let w = &self.waits[i];
            let end = if w.deadline.is_some_and(|d| now >= d) {
                Some(WaitEnd::TimedOut)
            } else if now >= w.block_until {
                Some(WaitEnd::StillWaiting(
                    now.saturating_duration_since(w.since),
                ))
            } else {
                None
            };
            match end {
                Some(e) => {
                    let w = self.waits.remove(i);
                    if matches!(e, WaitEnd::StillWaiting(_)) {
                        self.recent.insert(w.session, (w.key.clone(), w.since, now));
                    } else {
                        self.recent.remove(&w.session);
                    }
                    out.push((w, e));
                }
                None => i += 1,
            }
        }
        out
    }

    /// Removes the waits of a connection's request (cancelled) or of a session (ended).
    pub fn drop_where(&mut self, f: impl Fn(&Wait) -> bool) -> usize {
        let before = self.waits.len();
        self.waits.retain(|w| !f(w));
        before - self.waits.len()
    }

    /// The start time a repeat of a wait keeps.
    pub fn since_for(&self, session: u32, key: &Key, now: Instant) -> Instant {
        match self.recent.get(&session) {
            Some((k, since, last))
                if k == key && now.saturating_duration_since(*last) < Duration::from_secs(60) =>
            {
                *since
            }
            _ => now,
        }
    }

    /// Waiting sessions per event (for the Queue tab).
    pub fn waiters(&self, key: &Key) -> Vec<u32> {
        self.waits
            .iter()
            .filter(|w| &w.key == key)
            .map(|w| w.session)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["pr-123-merged", "deploy.ok", "ci/main#42", "a:b_c"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "has space", "ümlaut", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
