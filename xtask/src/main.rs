//! Benchmarks and verification tasks for Fallow. Run with `cargo xtask <task>`.
//!
//! `bench` measures the constitution VIII budgets on Fallow's own process (007 T083).

mod bench;
#[cfg(target_os = "macos")]
mod mac_procs;
#[cfg(target_os = "macos")]
mod no_listener;
#[cfg(target_os = "macos")]
mod screens;
mod spec_kit;

use std::process::ExitCode;

const TASKS: &[&str] = &[
    "bench",
    "spec-kit",
    "no-listener (macOS)",
    "screens (macOS)",
];

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
        // 016 (SC-007, T039): the release app listens on no port.
        #[cfg(target_os = "macos")]
        Some("no-listener") => no_listener::run(&args[1..]),
        // 016 (SC-005, T047): the screens in the Mac window, light and dark.
        #[cfg(target_os = "macos")]
        Some("screens") => screens::run(&args[1..]),
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
