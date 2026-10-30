//! Benchmarks and verification tasks for Fallow. Run with `cargo xtask <task>`.
//!
//! The constitution VIII benchmarks and the retargeted egress test arrive with 006's polish phase
//! (tasks T074, T075); the terminal app's tasks were removed with it.

mod spec_kit;

use std::process::ExitCode;

const TASKS: &[&str] = &["spec-kit"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => {
            eprintln!(
                "usage: cargo xtask <task> [args]\ntasks: {}",
                TASKS.join(", ")
            );
            return ExitCode::from(2);
        }
        Some("spec-kit") => spec_kit::run(&args[1..]),
        Some(t) => {
            eprintln!("xtask: unknown task {t:?}\ntasks: {}", TASKS.join(", "));
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
