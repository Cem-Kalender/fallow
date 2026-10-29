//! Runs: their event logs, their state, and the stage machine that decides what happens next
//! (006 research R-2, data-model.md).

pub mod engine;
pub mod events;
pub mod machine;
pub mod state;
pub mod time;
