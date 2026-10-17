//! Benchmarks and verification tasks for Fallow (research R-13). Run with `cargo xtask <task>`.

use std::process::ExitCode;

const TASKS: &[&str] = &[
    "bench-startup",
    "bench-memory",
    "detect-fixtures",
    "canary",
    "egress",
    "defender-check",
    "gen-fixture",
    "bench-search",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(task) = args.first() else {
        eprintln!(
            "usage: cargo xtask <task> [args]\ntasks: {}",
            TASKS.join(", ")
        );
        return ExitCode::from(2);
    };
    match task.as_str() {
        t if TASKS.contains(&t) => {
            eprintln!("xtask {t}: not implemented");
            ExitCode::from(2)
        }
        t => {
            eprintln!("xtask: unknown task {t:?}\ntasks: {}", TASKS.join(", "));
            ExitCode::from(2)
        }
    }
}
