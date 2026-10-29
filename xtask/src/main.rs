//! Benchmarks and verification tasks for Fallow. Run with `cargo xtask <task>`.
//!
//! The constitution VIII benchmarks and the retargeted egress test arrive with 006's polish phase
//! (tasks T074, T075); the terminal app's tasks were removed with it.

use std::process::ExitCode;

const TASKS: &[&str] = &[];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first() {
        None => {
            eprintln!("usage: cargo xtask <task> [args]\ntasks: (none yet)");
            ExitCode::from(2)
        }
        Some(t) => {
            eprintln!("xtask: unknown task {t:?}\ntasks: {}", TASKS.join(", "));
            ExitCode::from(2)
        }
    }
}
