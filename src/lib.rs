//! Fallow: a terminal editor with a live panel of the LLM processes on the machine.
//!
//! The binary (`src/main.rs`) is a thin wrapper; the modules live in this library so that
//! integration tests in `tests/` can reach them.

pub mod app;
pub mod cli;
pub mod config;
pub mod dns;
pub mod procmon;
pub mod ui;
