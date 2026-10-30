//! What Fallow reads and writes in Spec Kit files (007 contracts/spec-kit-files.md, research
//! S-8): phases and tasks from `tasks.md`, user stories and acceptance scenarios (`USn/k`) from
//! `spec.md`, commands from `plan.md`, the lock checks, and ticking accepted tasks.

pub mod lock;
pub mod plan;
pub mod spec;
pub mod tasks;

use std::fmt;

/// Something Fallow cannot read, at a file's line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Problem {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl Problem {
    pub fn new(file: &str, line: usize, message: impl Into<String>) -> Problem {
        Problem {
            file: file.to_string(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}
