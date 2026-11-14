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
  fallow                       open the dashboard (starts the service if needed)
  fallow status                list the loops
  fallow stop                  stop the service (runs resume at the next start)
  fallow serve [--config-dir <dir>] [--data-dir <dir>]
                               run the service in the foreground
  fallow autostart             print the command that starts Fallow at login
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

#[cfg(not(windows))]
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

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
    match first {
        // 014 (contracts/cli.md): Fallow's window on Windows, never a browser.
        #[cfg(windows)]
        "" => {
            if !fallow::platform::display::interactive() {
                return fail(
                    "the Studio needs Fallow's window and a desktop; `fallow serve` runs the service without one",
                );
            }
            match service::open_window(None) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }
        #[cfg(not(windows))]
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
        "stop" => match answer(service::request(json!({ "cmd": "stop" }))) {
            Ok(_) => {
                println!("The Fallow service is stopping; runs resume at its next start.");
                ExitCode::SUCCESS
            }
            Err(_) => {
                println!("The Fallow service is not running.");
                ExitCode::SUCCESS
            }
        },
        "autostart" => {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "fallow".into());
            println!(
                "Fallow does not change your login items itself. To start it at login, run this once in PowerShell:\n\n$s = (New-Object -ComObject WScript.Shell).CreateShortcut(\"$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Fallow.lnk\"); $s.TargetPath = '{exe}'; $s.Arguments = 'serve'; $s.WindowStyle = 7; $s.Save()"
            );
            ExitCode::SUCCESS
        }
        "bench" => fail("measured series arrive with 006 user story 7"),
        other => usage(format!("unknown command {other:?}")),
    }
}

fn absolute(d: &str) -> std::path::PathBuf {
    std::path::absolute(d).unwrap_or_else(|_| std::path::PathBuf::from(d))
}
