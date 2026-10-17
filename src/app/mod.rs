//! Application state, commands, key bindings and the main event loop.

pub mod commands;
pub mod event_loop;
pub mod keymap;
pub mod state;

use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::cli::Options;
use crate::config::{self, Config, ConfigPaths};
use crate::dns;
use crate::procmon::json;
use crate::procmon::platform::{self, OsPlatform};
use crate::procmon::snapshot::{Note, NoteKind};
use crate::procmon::worker::{self, Monitor, WorkerConfig};
use crate::ui::terminal;

/// What a run produced, for `--bench-stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    pub ticks: u64,
    /// Procmon refresh cycles.
    pub cycles: u64,
}

/// `--all-users` takes effect only when Fallow already runs elevated (FR-031).
fn all_users(opts: &Options) -> (bool, Option<String>) {
    if !opts.all_users {
        return (false, None);
    }
    if platform::is_elevated() {
        (true, None)
    } else {
        let who = if cfg!(windows) {
            "Administrator"
        } else {
            "root"
        };
        (
            false,
            Some(format!(
                "--all-users needs Fallow to be started elevated ({who}); showing your processes only"
            )),
        )
    }
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
    if let Some(p) = &opts.path {
        if let Err(e) = std::fs::read_dir(p) {
            eprintln!("fallow: cannot open {}: {e}", p.display());
            return (ExitCode::FAILURE, stats);
        }
    }
    let mut cfg = config::load(&paths);
    if opts.no_dns {
        cfg.settings.dns.enabled = false;
    }
    let (all, all_note) = all_users(opts);
    let notes: Vec<Note> = all_note
        .iter()
        .map(|t| Note {
            kind: NoteKind::Degraded,
            text: t.as_str().into(),
        })
        .collect();
    if opts.snapshot_mode() {
        return (run_snapshot(opts, &cfg, all, notes), stats);
    }
    if !std::io::stdout().is_terminal() {
        eprintln!("fallow: needs a terminal (standard output is not a terminal)");
        return (ExitCode::FAILURE, stats);
    }

    let mut notices: Vec<String> = cfg.notices.iter().map(|n| n.to_string()).collect();
    notices.extend(all_note);
    let mut state = state::AppState::new(opts.path.clone(), notices);
    state.all_users = all;

    let mut term = match terminal::enter(cfg.settings.ui.mouse) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("fallow: cannot set up the terminal: {e}");
            return (ExitCode::FAILURE, stats);
        }
    };
    let cycles = AtomicU64::new(0);
    // Background threads start only after the first frame (R-6, FR-022c).
    let start = move || {
        let shared = dns::initial(&cfg.providers);
        if cfg.settings.dns.enabled {
            let _ = dns::spawn(
                cfg.providers.clone(),
                cfg.settings.dns.refresh_minutes,
                shared.clone(),
            );
        }
        let handle = worker::spawn(WorkerConfig {
            settings: cfg.settings.clone(),
            signatures: cfg.signatures.clone(),
            providers: shared,
            all_users: all,
            notes,
        })
        .ok()?;
        Some(handle)
    };
    let result = event_loop::run(&mut term, &mut state, start, &cycles);
    drop(term);
    terminal::restore();
    stats.ticks = state.ticks;
    stats.cycles = cycles.load(Ordering::Relaxed);
    match result {
        Ok(()) => (ExitCode::SUCCESS, stats),
        Err(e) => {
            eprintln!("fallow: terminal error: {e}");
            (ExitCode::FAILURE, stats)
        }
    }
}

/// The hidden test interface (contracts/cli.md, T048). Never starts the TUI.
fn run_snapshot(opts: &Options, cfg: &Config, all_users: bool, notes: Vec<Note>) -> ExitCode {
    let shared = dns::initial(&cfg.providers);
    if cfg.settings.dns.enabled {
        if let Ok(mut s) = shared.lock() {
            *s = Arc::new(dns::resolve(&cfg.providers));
        }
    }
    let mut sys = sysinfo::System::new();
    let mut monitor = Monitor::new(
        OsPlatform::default(),
        &cfg.signatures,
        &cfg.settings,
        shared,
        all_users,
        notes,
    );
    let interval = Duration::from_millis(cfg.settings.refresh_interval_ms);
    let mut out = std::io::stdout().lock();

    if let Some(secs) = opts.snapshot_stream {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut cycle = 0u64;
        while Instant::now() < until {
            let started = Instant::now();
            let (procs, total, avail) = worker::sample(&mut sys);
            let snap = monitor.cycle(&procs, total, avail);
            cycle += 1;
            if writeln!(out, "{}", json::stream_line(&snap, cycle))
                .and_then(|_| out.flush())
                .is_err()
            {
                return ExitCode::FAILURE;
            }
            if let Some(rest) = interval.checked_sub(started.elapsed()) {
                std::thread::sleep(rest.min(until.saturating_duration_since(Instant::now())));
            }
        }
        return ExitCode::SUCCESS;
    }

    // Two cycles, so CPU figures have a delta.
    let (procs, total, avail) = worker::sample(&mut sys);
    let _ = monitor.cycle(&procs, total, avail);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(500)));
    let (procs, total, avail) = worker::sample(&mut sys);
    let snap = monitor.cycle(&procs, total, avail);
    if writeln!(out, "{}", json::snapshot(&snap))
        .and_then(|_| out.flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    if let Some(secs) = opts.snapshot_hold {
        // Keep every piece of state alive for the memory dump (SC-007 canary).
        std::thread::sleep(Duration::from_secs(secs));
        drop((snap, monitor, sys));
    }
    ExitCode::SUCCESS
}
