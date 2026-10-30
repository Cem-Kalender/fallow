//! The page's global scope (007 research S-4, contracts/web-api.md `GET /api/events`): a short
//! history of what changed across projects and loops, and the pages waiting for the next change.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::run::engine::Reply;

/// How many recent events are kept for pages that come back.
const KEEP: usize = 200;

#[derive(Default)]
pub struct Bus {
    pub seq: u64,
    events: VecDeque<(u64, Value)>,
    waiters: Vec<(Instant, u64, Sender<Reply>)>,
}

impl Bus {
    /// Records an event (`{"type": …}`) and answers the pages waiting for it.
    pub fn publish(&mut self, mut event: Value) {
        self.seq += 1;
        event["seq"] = json!(self.seq);
        self.events.push_back((self.seq, event));
        while self.events.len() > KEEP {
            self.events.pop_front();
        }
        let waiting = std::mem::take(&mut self.waiters);
        for (since, after, tx) in waiting {
            if after < self.seq {
                let _ = tx.send(Reply::Ok(self.answer(after)));
            } else {
                self.waiters.push((since, after, tx));
            }
        }
    }

    /// The events after `after`, and where the page should continue from. A page that knows a
    /// later number than this service (Fallow restarted) is told to start again (`reset`).
    fn answer(&self, after: u64) -> Value {
        if after > self.seq {
            return json!({ "seq": self.seq, "events": [], "reset": true });
        }
        let events: Vec<&Value> = self
            .events
            .iter()
            .filter(|(s, _)| *s > after)
            .map(|(_, v)| v)
            .collect();
        json!({ "seq": self.seq, "events": events })
    }

    /// Answers at once when something is newer than `after`, or keeps the page waiting.
    pub fn wait(&mut self, after: u64, reply: Sender<Reply>) {
        if after != self.seq {
            let _ = reply.send(Reply::Ok(self.answer(after)));
        } else {
            self.waiters.push((Instant::now(), after, reply));
        }
    }

    /// Lets go of pages that stopped waiting (a page gives up after 25 s).
    pub fn prune(&mut self) {
        self.waiters
            .retain(|(since, ..)| since.elapsed() < Duration::from_secs(60));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_page_gets_the_next_event_and_a_late_page_the_history() {
        let mut b = Bus::default();
        let (tx, rx) = std::sync::mpsc::channel();
        b.wait(0, tx);
        assert!(rx.try_recv().is_err(), "nothing yet: it waits");
        b.publish(json!({"type": "project", "id": "shop"}));
        let Reply::Ok(v) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(v["seq"], 1);
        assert_eq!(v["events"][0]["type"], "project");
        b.publish(json!({"type": "loop_progress", "run": 3}));
        let (tx, rx) = std::sync::mpsc::channel();
        b.wait(0, tx);
        let Reply::Ok(v) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(v["events"].as_array().unwrap().len(), 2);
        let (tx, rx) = std::sync::mpsc::channel();
        b.wait(9, tx);
        let Reply::Ok(v) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(
            v["reset"], true,
            "a page from before a restart starts again"
        );
    }
}
