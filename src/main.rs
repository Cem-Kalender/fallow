//! Fallow: a terminal editor with a live panel of the LLM processes on the machine.

use std::process::ExitCode;

fn main() -> ExitCode {
    // Exit codes (contracts/cli.md): 0 normal quit, 1 startup failure, 2 invalid arguments.
    ExitCode::SUCCESS
}
