//! Benchmarks and verification tasks for Fallow. Run with `cargo xtask <task>`.
//!
//! `bench` measures the constitution VIII budgets on Fallow's own process (007 T083).

mod bench;
mod spec_kit;

use std::process::ExitCode;

const TASKS: &[&str] = &["bench", "spec-kit"];

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
        Some("bench") => bench::run(&args[1..]),
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
