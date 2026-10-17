//! Command-line parsing (contracts/cli.md).
//!
//! Hand-written, with no argument-parsing crate, to keep startup cost down (FR-041).

use std::ffi::OsString;
use std::path::PathBuf;

/// Usage text for `--help`. The hidden test-only options are deliberately absent.
pub const USAGE: &str = "\
Fallow: a terminal editor that shows the LLM processes on your machine.

Usage: fallow [OPTIONS] [PATH]

Arguments:
  [PATH]  Project directory to open. Without it, Fallow starts with no project.

Options:
      --all-users           Show all users' processes (only when already running elevated)
      --no-dns              Don't resolve provider hostnames for this run
      --config-dir <DIR>    Use DIR instead of the platform config directory
      --print-config-paths  Print the resolved config file paths and exit
  -V, --version             Print the version and exit
  -h, --help                Print this help and exit

Exit codes: 0 normal quit, 1 startup failure, 2 invalid arguments.
";

/// Options for a normal run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub path: Option<PathBuf>,
    pub all_users: bool,
    pub no_dns: bool,
    pub config_dir: Option<PathBuf>,
    pub print_config_paths: bool,
    /// Hidden: print one JSON snapshot and exit (T048).
    pub snapshot_json: bool,
    /// Hidden: like `--snapshot-json`, then stay alive for SECS seconds (T048).
    pub snapshot_hold: Option<u64>,
    /// Hidden: one JSON line per refresh cycle for SECS seconds (T048).
    pub snapshot_stream: Option<u64>,
    /// Hidden: on exit, write `pid=… ticks=… cycles=…` to this file (benchmarks, R-12).
    pub bench_stats: Option<PathBuf>,
}

impl Options {
    /// True when one of the hidden snapshot modes was requested; they never start the TUI.
    pub fn snapshot_mode(&self) -> bool {
        self.snapshot_json || self.snapshot_hold.is_some() || self.snapshot_stream.is_some()
    }
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Run(Options),
    Help,
    Version,
}

/// Parses the arguments after the program name. `Err` carries a one-line message; the caller
/// prints it and exits with code 2.
pub fn parse<I>(args: I) -> Result<Action, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut opts = Options::default();
    let mut args = args.into_iter();
    let mut only_positional = false;
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if only_positional || !text.starts_with('-') || text == "-" {
            set_path(&mut opts, arg)?;
            continue;
        }
        if text == "--" {
            only_positional = true;
            continue;
        }
        // `--name=value` or `--name value`.
        let (name, inline) = match text.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(OsString::from(v))),
            _ => (text.clone(), None),
        };
        let mut value = |what: &str| -> Result<OsString, String> {
            match inline.clone().or_else(|| args.next()) {
                Some(v) if !v.is_empty() => Ok(v),
                _ => Err(format!("{name} needs a value ({what})")),
            }
        };
        let flag = |inline: &Option<OsString>| match inline {
            Some(_) => Err(format!("{name} does not take a value")),
            None => Ok(true),
        };
        match name.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "--all-users" => opts.all_users = flag(&inline)?,
            "--no-dns" => opts.no_dns = flag(&inline)?,
            "--print-config-paths" => opts.print_config_paths = flag(&inline)?,
            "--snapshot-json" => opts.snapshot_json = flag(&inline)?,
            "--config-dir" => opts.config_dir = Some(PathBuf::from(value("DIR")?)),
            "--bench-stats" => opts.bench_stats = Some(PathBuf::from(value("PATH")?)),
            "--snapshot-hold" => opts.snapshot_hold = Some(seconds(&name, value("SECS")?)?),
            "--snapshot-stream" => opts.snapshot_stream = Some(seconds(&name, value("SECS")?)?),
            _ => return Err(format!("unknown option {text}")),
        }
    }
    let snapshot_modes = [
        opts.snapshot_json,
        opts.snapshot_hold.is_some(),
        opts.snapshot_stream.is_some(),
    ];
    if snapshot_modes.iter().filter(|&&on| on).count() > 1 {
        return Err(
            "--snapshot-json, --snapshot-hold and --snapshot-stream are mutually exclusive".into(),
        );
    }
    Ok(Action::Run(opts))
}

fn set_path(opts: &mut Options, arg: OsString) -> Result<(), String> {
    if opts.path.is_some() {
        return Err(format!(
            "unexpected extra argument {}",
            arg.to_string_lossy()
        ));
    }
    opts.path = Some(PathBuf::from(arg));
    Ok(())
}

fn seconds(name: &str, v: OsString) -> Result<u64, String> {
    let text = v.to_string_lossy();
    text.parse::<u64>()
        .map_err(|_| format!("{name} needs a whole number of seconds, got {text:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Action, String> {
        parse(args.iter().map(OsString::from))
    }

    fn opts(args: &[&str]) -> Options {
        match run(args) {
            Ok(Action::Run(o)) => o,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    #[test]
    fn no_arguments_is_an_idle_run() {
        assert_eq!(opts(&[]), Options::default());
    }

    #[test]
    fn options_and_values() {
        let o = opts(&["--all-users", "--no-dns", "--config-dir", "/c", "proj"]);
        assert!(o.all_users && o.no_dns);
        assert_eq!(o.config_dir, Some(PathBuf::from("/c")));
        assert_eq!(o.path, Some(PathBuf::from("proj")));
        let o = opts(&[
            "--config-dir=/d",
            "--bench-stats=/s",
            "--snapshot-hold",
            "20",
        ]);
        assert_eq!(o.config_dir, Some(PathBuf::from("/d")));
        assert_eq!(o.bench_stats, Some(PathBuf::from("/s")));
        assert_eq!(o.snapshot_hold, Some(20));
        assert!(o.snapshot_mode());
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(run(&["--no-dns", "-h"]), Ok(Action::Help));
        assert_eq!(run(&["-V", "--bogus"]), Ok(Action::Version));
    }

    #[test]
    fn errors() {
        assert!(run(&["--bogus"]).is_err());
        assert!(run(&["a", "b"]).is_err());
        assert!(run(&["--config-dir"]).is_err());
        assert!(run(&["--config-dir="]).is_err());
        assert!(run(&["--no-dns=yes"]).is_err());
        assert!(run(&["--snapshot-hold", "ten"]).is_err());
        assert!(run(&["--snapshot-json", "--snapshot-stream", "5"]).is_err());
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(
            opts(&["--", "--weird-dir"]).path,
            Some(PathBuf::from("--weird-dir"))
        );
    }
}
