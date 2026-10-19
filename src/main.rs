//! Fallow: a terminal editor with a live panel of the LLM processes on the machine.

use std::process::ExitCode;

use fallow::{app, cli};

fn main() -> ExitCode {
    // Exit codes (contracts/cli.md): 0 normal quit, 1 startup failure, 2 invalid arguments.
    let opts = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Action::Run(opts)) => opts,
        Ok(cli::Action::Help) => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(cli::Action::Version) => {
            println!("fallow {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("fallow: {e}\nTry 'fallow --help'.");
            return ExitCode::from(2);
        }
    };
    let (code, stats) = app::run(&opts);
    // Benchmark evidence (contracts/cli.md, research R-12): this process's PID and proof that
    // the event loop and procmon were running.
    if let Some(path) = &opts.bench_stats {
        let line = format!(
            "pid={} ticks={} cycles={} buffers={} searches={} copies={} sessions={}\n",
            std::process::id(),
            stats.ticks,
            stats.cycles,
            stats.buffers,
            stats.searches,
            stats.copies,
            stats.sessions
        );
        if let Err(e) = std::fs::write(path, line) {
            eprintln!("fallow: cannot write {}: {e}", path.display());
        }
    }
    code
}
