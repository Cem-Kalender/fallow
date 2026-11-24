//! The command line (006 contracts/cli.md): it starts or reaches the one service and opens
//! pages; the service does the work.

use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::{Value, json};

use fallow::mcp::bridge;
use fallow::service;

const USAGE: &str = "\
Fallow works out features with you following Spec Kit, then builds them in a loop it verifies
itself, on a local dashboard.

Usage:
  fallow                       open Fallow's window (starts the service if needed)
  fallow quit [--installer]    quit Fallow, asking first when work is running
  fallow status                list the loops
  fallow stop                  stop the service (runs resume at the next start)
  fallow serve [--config-dir <dir>] [--data-dir <dir>]
                               run the service in the foreground
  fallow --version
";

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("fallow: {msg}");
    ExitCode::from(1)
}

fn usage(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("fallow: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

#[cfg(any(
    not(any(windows, target_os = "macos")),
    all(target_os = "macos", feature = "automation")
))]
fn open(url: &str) {
    println!("{}", url.split("?k=").next().unwrap_or(url));
    if let Err(e) = fallow::platform::open_browser(url) {
        eprintln!("fallow: {e}; open the address above while signed in from `fallow`");
    }
}

fn answer(v: Result<Value, String>) -> Result<Value, String> {
    let v = v?;
    if v["ok"] == true {
        Ok(v)
    } else {
        Err(v["error"]
            .as_str()
            .unwrap_or("the service refused")
            .to_string())
    }
}

/// 016 (research M-3, contracts/cli-macos.md): whether LaunchServices started this program as
/// `Fallow.app` (Finder, the Dock, Spotlight, `open`, or `fallow` asking it): the binary inside
/// the bundle, no arguments but an old-style `-psn_…`, and LaunchServices' own mark of the
/// bundle in the environment. Not "the parent is launchd" alone, which a launchd job or an
/// orphaned shell also has.
#[cfg(target_os = "macos")]
fn launched_as_app(args: &[OsString]) -> bool {
    args.iter()
        .all(|a| a.to_str().is_some_and(|a| a.starts_with("-psn_")))
        && std::env::var_os("__CFBundleIdentifier").is_some_and(|v| v == "dev.fallow.Fallow")
        && fallow::platform::bundle().is_some()
}

/// 016: started as the app: becomes the service with its window, its output in the service's
/// log; a Fallow already running is asked for its window instead.
#[cfg(target_os = "macos")]
fn run_as_app() -> ExitCode {
    // Not handed down: what the service starts (an agent running `fallow`) is not the app.
    // SAFETY: the process has one thread yet.
    unsafe { std::env::remove_var("__CFBundleIdentifier") };
    // Found on the first release: opened from Finder or the Dock, the app had macOS's minimal
    // search path and found no `claude`. It takes the login shell's, as a terminal has it, for
    // itself and all it starts (the service, agents, git).
    let path = fallow::platform::app_search_path();
    // SAFETY: as above; the shell asked for it has ended, and its reader thread with it.
    unsafe { std::env::set_var("PATH", path) };
    // A Fallow already runs (`fallow serve` in a terminal): its window, asked directly, not
    // through LaunchServices again (review). Of another version (T034): it quits first, with its
    // own confirmation, and this one starts.
    if let Ok(status) = service::request(json!({ "cmd": "status" })) {
        if service::other_version(&status).is_none() {
            return match answer(service::request(json!({ "cmd": "window" }))) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    fallow::window::mac::alert::cannot_start(&e);
                    ExitCode::from(1)
                }
            };
        }
        if let Err(e) = service::replace_other_version() {
            fallow::window::mac::alert::cannot_start(&e);
            return ExitCode::from(1);
        }
    }
    // A Fallow from before 016 on the old data folder (T034): quits on the engineer's word.
    let confirm = |names: &[String]| {
        objc2::MainThreadMarker::new().is_some_and(|mtm| {
            fallow::window::mac::alert::ask(
                mtm,
                objc2_app_kit::NSAlertStyle::Warning,
                "An older Fallow is running",
                &format!(
                    "It quits so this version can start; this work pauses and resumes here:\n\n{}",
                    fallow::window::quit::text(names)
                ),
                &["Quit It", "Cancel"],
            ) == Some(0)
        })
    };
    match service::quit_older(confirm) {
        Ok(true) => {}
        Ok(false) => return ExitCode::from(1),
        Err(e) => {
            fallow::window::mac::alert::cannot_start(&e);
            return ExitCode::from(1);
        }
    }
    let log = service::data_dir().and_then(|d| service::service_log(&d));
    match log {
        Ok(log) => {
            use std::os::fd::AsRawFd;
            // SAFETY: duplicating a live file descriptor onto standard output and error.
            unsafe {
                libc::dup2(log.as_raw_fd(), 1);
                libc::dup2(log.as_raw_fd(), 2);
            }
        }
        Err(e) => {
            fallow::window::mac::alert::cannot_start(&e);
            return ExitCode::from(1);
        }
    }
    fallow::window::mac::app::open_at_start();
    match service::serve(None, None) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fallow: {e}");
            fallow::window::mac::alert::cannot_start(&e);
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    #[cfg(target_os = "macos")]
    if launched_as_app(&args) {
        return run_as_app();
    }
    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
    match first {
        // 014 (contracts/cli.md): Fallow's window on Windows, never a browser; 016
        // (contracts/cli-macos.md): on macOS too.
        // 016 (US5 AS3, FR-018): a Mac test build asked for its page (`FALLOW_AUTOMATION=1`)
        // opens the Studio in the browser as before 016, for working on Fallow itself.
        #[cfg(all(target_os = "macos", feature = "automation"))]
        "" if std::env::var_os("FALLOW_AUTOMATION").is_some_and(|v| v == "1") => {
            if let Err(e) = service::ensure_running() {
                return fail(e);
            }
            match answer(service::request(json!({ "cmd": "url", "path": "/" }))) {
                Ok(v) => {
                    open(v["url"].as_str().unwrap_or_default());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(format!(
                    "{e} (a service without its page runs: `fallow stop`, then this again)"
                )),
            }
        }
        #[cfg(any(windows, target_os = "macos"))]
        "" => {
            if !fallow::platform::display::interactive() {
                return fail(if cfg!(target_os = "macos") {
                    "the Studio needs Fallow's window and the Mac's desktop (this session has no display, as over SSH); `fallow serve` runs the service without one"
                } else {
                    "the Studio needs Fallow's window and a desktop; `fallow serve` runs the service without one"
                });
            }
            match service::open_window(None) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        "" => {
            if let Err(e) = service::ensure_running() {
                return fail(e);
            }
            match answer(service::request(json!({ "cmd": "url", "path": "/" }))) {
                Ok(v) => {
                    open(v["url"].as_str().unwrap_or_default());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        "-V" | "--version" => {
            println!("fallow {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "serve" => {
            let mut config_dir = None;
            let mut data_dir = None;
            let mut rest = args
                .iter()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned());
            while let Some(a) = rest.next() {
                match (a.as_str(), rest.next()) {
                    // Absolute: git runs in other folders (worktrees under the data folder).
                    ("--config-dir", Some(d)) => config_dir = Some(absolute(&d)),
                    ("--data-dir", Some(d)) => data_dir = Some(absolute(&d)),
                    _ => return usage(format!("serve: unknown option {a}")),
                }
            }
            match service::serve(config_dir.as_deref(), data_dir) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }
        "mcp-bridge" => match bridge::parse(args.into_iter().skip(1)) {
            Ok(a) => bridge::run(&a),
            Err(e) => {
                eprintln!("fallow mcp-bridge: {e}\n\n{}", bridge::USAGE);
                ExitCode::from(2)
            }
        },
        "status" => match answer(service::request(json!({ "cmd": "status" }))) {
            Ok(v) => {
                let runs = v["value"].as_array().cloned().unwrap_or_default();
                if runs.is_empty() {
                    println!("No loops.");
                }
                for r in runs {
                    // A loop names its project and feature; 006's runs, their task.
                    let (kind, what) = match (r["project"].as_str(), r["feature"].as_str()) {
                        (Some(p), Some(f)) => ("loop", format!("{p} {f}")),
                        _ => ("run", r["task"].as_str().unwrap_or_default().to_string()),
                    };
                    println!(
                        "{kind:<4} {:<4} {:<16} {:<40} {}",
                        r["id"],
                        r["stage"].as_str().unwrap_or_default(),
                        what.chars().take(40).collect::<String>(),
                        r["needs_engineer"].as_str().unwrap_or_default()
                    );
                }
                ExitCode::SUCCESS
            }
            Err(_) => {
                println!("The Fallow service is not running.");
                ExitCode::SUCCESS
            }
        },
        // 014 (contracts/cli.md): Quit as the icon does it; `--installer` waits until Fallow
        // has exited, and says a cancel with exit 1. 016: on macOS too.
        #[cfg(any(windows, target_os = "macos"))]
        "quit" => {
            let installer = args.get(1).and_then(|a| a.to_str()) == Some("--installer");
            // Exit 1: the engineer cancelled; 2: an error (the installer tells them apart).
            match service::quit(installer) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => {
                    eprintln!("fallow: Quit was cancelled; Fallow keeps running");
                    ExitCode::from(1)
                }
                Err(e) => {
                    eprintln!("fallow: {e}");
                    ExitCode::from(2)
                }
            }
        }
        "stop" => match answer(service::request(json!({ "cmd": "stop" }))) {
            Ok(_) => {
                println!("The Fallow service is stopping; runs resume at its next start.");
                ExitCode::SUCCESS
            }
            // 016 (research M-22): an older Fallow on the old data folder, if any.
            Err(_) if service::stop_older() => {
                println!("The older Fallow service is stopping; its runs resume in this version.");
                ExitCode::SUCCESS
            }
            Err(_) => {
                println!("The Fallow service is not running.");
                ExitCode::SUCCESS
            }
        },
        "bench" => fail("measured series arrive with 006 user story 7"),
        other => usage(format!("unknown command {other:?}")),
    }
}

fn absolute(d: &str) -> std::path::PathBuf {
    std::path::absolute(d).unwrap_or_else(|_| std::path::PathBuf::from(d))
}
