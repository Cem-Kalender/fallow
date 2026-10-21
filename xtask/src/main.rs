//! Benchmarks and verification tasks for Fallow (research R-13). Run with `cargo xtask <task>`.

mod bench_memory;
mod bench_new_process;
mod bench_refresh;
mod bench_search;
mod bench_sessions;
mod bench_startup;
mod canary;
mod canary_sessions;
mod diag;
mod egress;
mod fixtures;
mod gen_fixture;
mod mcp_agent;
mod projects;
mod pty;
mod record;
mod rss;
mod unprivileged;

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
    "bench-refresh",
    "bench-new-process",
    "unprivileged",
    "bench-sessions",
    "record-transcript",
    "diag-sessions",
];

fn main() -> ExitCode {
    // Fixture copies of this binary (`ollama`, `claude`) play their part and never return.
    fixtures::persona();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(task) = args.first() else {
        eprintln!(
            "usage: cargo xtask <task> [args]\ntasks: {}",
            TASKS.join(", ")
        );
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    match task.as_str() {
        "bench-startup" => ExitCode::from(bench_startup::run(rest) as u8),
        "bench-memory" => ExitCode::from(bench_memory::run(rest) as u8),
        "bench-refresh" => ExitCode::from(bench_refresh::run(rest) as u8),
        "bench-new-process" => ExitCode::from(bench_new_process::run(rest) as u8),
        "unprivileged" => ExitCode::from(unprivileged::run(rest) as u8),
        "canary" => ExitCode::from(canary::run(rest) as u8),
        "egress" => ExitCode::from(egress::run(rest) as u8),
        "gen-fixture" => ExitCode::from(gen_fixture::run(rest) as u8),
        "bench-search" => ExitCode::from(bench_search::run(rest) as u8),
        "bench-sessions" => ExitCode::from(bench_sessions::run(rest) as u8),
        "record-transcript" => ExitCode::from(record::run(rest) as u8),
        "diag-sessions" => ExitCode::from(diag::run(rest) as u8),
        t if fixtures::subcommand(t, rest).is_some() => ExitCode::from(3),
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
