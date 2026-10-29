//! The command line (006 contracts/cli.md): it starts or reaches the one service and opens
//! pages; the service does the work.

use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::{Value, json};

use fallow::mcp::bridge;
use fallow::service;

const USAGE: &str = "\
Fallow takes a coding task from a plan you approve to a result it has verified itself.

Usage:
  fallow                       open the runs page (starts the service if needed)
  fallow run \"<task>\" [options]  start a run in the current repository
      --target <branch>          the branch the result is for (default: the current branch)
      --autonomy high|medium|low how much is decided without you
      --limit-usd <n>            the run's cost limit
      --limit-min <n>            the run's time limit
      --role <role>=<agent>[:<model>]  the agent of planner, builder or reviewer
  fallow status                list the runs
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
                    ("--config-dir", Some(d)) => config_dir = Some(std::path::PathBuf::from(d)),
                    ("--data-dir", Some(d)) => data_dir = Some(std::path::PathBuf::from(d)),
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
                    println!("No runs.");
                }
                for r in runs {
                    println!(
                        "#{:<4} {:<16} {:<40} {}",
                        r["id"],
                        r["stage"].as_str().unwrap_or_default(),
                        r["task"]
                            .as_str()
                            .unwrap_or_default()
                            .chars()
                            .take(40)
                            .collect::<String>(),
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
        "run" => {
            let mut rest = args
                .iter()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned());
            let mut task: Option<String> = None;
            let mut req = json!({ "cmd": "run" });
            let mut roles = Vec::new();
            while let Some(a) = rest.next() {
                let mut value = |name: &str| rest.next().ok_or(format!("{name} needs a value"));
                match a.as_str() {
                    "--target" => match value("--target") {
                        Ok(v) => req["target"] = json!(v),
                        Err(e) => return usage(e),
                    },
                    "--autonomy" => match value("--autonomy") {
                        Ok(v) if matches!(v.as_str(), "high" | "medium" | "low") => {
                            req["autonomy"] = json!(v)
                        }
                        Ok(v) => return usage(format!("--autonomy {v}: high, medium or low")),
                        Err(e) => return usage(e),
                    },
                    "--limit-usd" => match value("--limit-usd").map(|v| v.parse::<f64>()) {
                        Ok(Ok(v)) if v > 0.0 => req["limit_usd"] = json!(v),
                        _ => return usage("--limit-usd needs a positive number"),
                    },
                    "--limit-min" => match value("--limit-min").map(|v| v.parse::<u64>()) {
                        Ok(Ok(v)) if v > 0 => req["limit_min"] = json!(v),
                        _ => return usage("--limit-min needs a positive whole number"),
                    },
                    "--role" => match value("--role") {
                        Ok(v) => {
                            let Some((role, choice)) = v.split_once('=') else {
                                return usage("--role role=agent[:model]");
                            };
                            let (agent, model) = choice.split_once(':').unwrap_or((choice, ""));
                            roles.push(json!({ "role": role, "agent": agent, "model": model }));
                        }
                        Err(e) => return usage(e),
                    },
                    s if s.starts_with("--") => return usage(format!("unknown option {s}")),
                    s => {
                        if task.is_some() {
                            return usage("give the task as one quoted argument");
                        }
                        task = Some(s.to_string());
                    }
                }
            }
            let Some(task) = task.filter(|t| !t.trim().is_empty()) else {
                return usage("fallow run needs a task");
            };
            let cwd = match std::env::current_dir() {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            req["task"] = json!(task);
            req["repo"] = json!(cwd.display().to_string());
            req["roles"] = json!(roles);
            if let Err(e) = service::ensure_running() {
                return fail(e);
            }
            match answer(service::request(req)) {
                Ok(v) => {
                    println!("Run #{} started.", v["id"]);
                    open(v["url"].as_str().unwrap_or_default());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        other => usage(format!("unknown command {other:?}")),
    }
}
