//! Fallow takes a coding task from a plan the engineer approved to a result it has verified
//! itself, running AI coding agents as non-interactive steps, and shows the run on a local page
//! (006, constitution v6.0.0).
//!
//! The binary (`src/main.rs`) is a thin command line; the modules live in this library so that
//! integration tests in `tests/` can reach them.

pub mod agent;
pub mod config;
pub mod conversation;
pub mod defs;
pub mod mcp;
pub mod platform;
pub mod project;
pub mod publish;
pub mod run;
pub mod service;
pub mod speckit;
pub mod studio;
pub mod verify;
