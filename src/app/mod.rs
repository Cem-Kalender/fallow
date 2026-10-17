//! Application state and the main event loop.

pub mod event_loop;
pub mod state;

use std::io::IsTerminal;
use std::process::ExitCode;

use crate::cli::Options;
use crate::config::{self, ConfigPaths};
use crate::ui::terminal;

/// What a run produced, for `--bench-stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    pub ticks: u64,
    /// Procmon refresh cycles; 0 until the worker exists (T034).
    pub cycles: u64,
}

/// Runs Fallow with parsed options. Exit codes per contracts/cli.md.
pub fn run(opts: &Options) -> (ExitCode, RunStats) {
    let mut stats = RunStats {
        ticks: 0,
        cycles: 0,
    };
    let paths = ConfigPaths::resolve(opts.config_dir.as_deref());
    if opts.print_config_paths {
        print!("{}", config::describe_paths(&paths));
        return (ExitCode::SUCCESS, stats);
    }
    if opts.snapshot_mode() {
        eprintln!("fallow: the snapshot options are not implemented yet");
        return (ExitCode::FAILURE, stats);
    }
    if let Some(p) = &opts.path {
        if let Err(e) = std::fs::read_dir(p) {
            eprintln!("fallow: cannot open {}: {e}", p.display());
            return (ExitCode::FAILURE, stats);
        }
    }
    if !std::io::stdout().is_terminal() {
        eprintln!("fallow: needs a terminal (standard output is not a terminal)");
        return (ExitCode::FAILURE, stats);
    }

    let mut cfg = config::load(&paths);
    if opts.no_dns {
        cfg.settings.dns.enabled = false;
    }
    let notices = cfg.notices.iter().map(|n| n.to_string()).collect();
    let mut state = state::AppState::new(opts.path.clone(), notices);

    let mut term = match terminal::enter(cfg.settings.ui.mouse) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("fallow: cannot set up the terminal: {e}");
            return (ExitCode::FAILURE, stats);
        }
    };
    let result = event_loop::run(&mut term, &mut state);
    drop(term);
    terminal::restore();
    stats.ticks = state.ticks;
    match result {
        Ok(()) => (ExitCode::SUCCESS, stats),
        Err(e) => {
            eprintln!("fallow: terminal error: {e}");
            (ExitCode::FAILURE, stats)
        }
    }
}
