//! Boards and messages (003 US1, data-model.md "Board and BoardMessage").
//!
//! One board per project plus one shared by all projects. Each keeps its newest messages; the
//! text of all boards together is capped too, oldest message first out.

use std::collections::{HashMap, VecDeque};
use std::time::SystemTime;

use super::Party;
use crate::projects::ProjectId;

/// Longest message text (FR-008).
pub const TEXT_MAX: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardId {
    Shared,
    Project(ProjectId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Increasing across all boards.
    pub id: u64,
    pub from: Party,
    pub at: SystemTime,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Boards {
    boards: HashMap<BoardId, VecDeque<Message>>,
    next_id: u64,
    bytes: usize,
    /// Messages per board (FR-008).
    pub per_board: usize,
}

impl Boards {
    pub fn new(per_board: usize) -> Boards {
        Boards {
            boards: HashMap::new(),
            next_id: 1,
            bytes: 0,
            per_board: per_board.max(1),
        }
    }

    /// Bytes of text stored on all boards.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Posts `text` (validated and cleaned by `clean_text`). `budget` is how many bytes the
    /// boards may hold in total; the oldest messages of any board go first.
    pub fn post(
        &mut self,
        board: BoardId,
        from: Party,
        text: String,
        at: SystemTime,
        budget: usize,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        while self.bytes + text.len() > budget && self.drop_oldest() {}
        let q = self.boards.entry(board).or_default();
        while q.len() >= self.per_board {
            if let Some(m) = q.pop_front() {
                self.bytes -= m.text.len();
            }
        }
        self.bytes += text.len();
        q.push_back(Message { id, from, at, text });
        id
    }

    fn drop_oldest(&mut self) -> bool {
        let Some(board) = self
            .boards
            .iter()
            .filter_map(|(b, q)| q.front().map(|m| (m.id, *b)))
            .min_by_key(|(id, _)| *id)
            .map(|(_, b)| b)
        else {
            return false;
        };
        if let Some(m) = self.boards.get_mut(&board).and_then(VecDeque::pop_front) {
            self.bytes -= m.text.len();
        }
        true
    }

    /// Messages newer than `since`, oldest first, at most `limit`; and the newest id on the
    /// board (0 when empty).
    pub fn read(&self, board: BoardId, since: u64, limit: usize) -> (Vec<&Message>, u64) {
        let Some(q) = self.boards.get(&board) else {
            return (Vec::new(), 0);
        };
        let newest = q.back().map_or(0, |m| m.id);
        let out = q.iter().filter(|m| m.id > since).take(limit).collect();
        (out, newest)
    }

    /// All messages of a board, oldest first.
    pub fn messages(&self, board: BoardId) -> impl Iterator<Item = &Message> {
        self.boards.get(&board).into_iter().flatten()
    }

    pub fn len(&self, board: BoardId) -> usize {
        self.boards.get(&board).map_or(0, VecDeque::len)
    }
}

/// Checks a message text: 1..=8 KiB of UTF-8 after cleaning. Control characters other than
/// newline and tab become U+FFFD, so a message cannot carry terminal escapes.
pub fn clean_text(text: &str, max: usize) -> Result<String, (&'static str, String)> {
    if text.trim().is_empty() {
        return Err(("invalid", "the text is empty".into()));
    }
    if text.len() > max {
        return Err(("too_large", format!("{} bytes; at most {max}", text.len())));
    }
    Ok(text
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect())
}
