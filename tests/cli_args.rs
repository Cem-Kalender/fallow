//! The command line (contracts/cli.md): every option, exit codes, hidden options absent from
//! `--help`. Runs the real binary with stdout piped, so it never reaches the TUI.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fallow(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fallow"))
        .args(args)
        .output()
        .expect("run fallow")
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("exit code")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn help_lists_public_options_and_hides_test_options() {
    for flag in ["--help", "-h"] {
        let o = fallow(&[flag]);
        assert_eq!(code(&o), 0);
        let text = stdout(&o);
        for opt in [
            "[PATH]",
            "--all-users",
            "--no-dns",
            "--config-dir <DIR>",
            "--print-config-paths",
            "-V, --version",
            "-h, --help",
        ] {
            assert!(text.contains(opt), "{flag}: help is missing {opt}\n{text}");
        }
        for hidden in [
            "--snapshot-json",
            "--snapshot-hold",
            "--snapshot-stream",
            "--bench-stats",
        ] {
            assert!(!text.contains(hidden), "{flag}: help shows hidden {hidden}");
        }
    }
}

#[test]
fn version() {
    for flag in ["--version", "-V"] {
        let o = fallow(&[flag]);
        assert_eq!(code(&o), 0);
        assert_eq!(
            stdout(&o).trim(),
            concat!("fallow ", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn invalid_arguments_exit_2() {
    for args in [
        &["--bogus"][..],
        &["-x"],
        &["one", "two"],
        &["--config-dir"],
        &["--config-dir="],
        &["--no-dns=yes"],
        &["--all-users=1"],
        &["--snapshot-hold"],
        &["--snapshot-hold", "soon"],
        &["--snapshot-stream", "-5"],
        &["--bench-stats"],
        &["--snapshot-json", "--snapshot-hold", "3"],
    ] {
        let o = fallow(args);
        assert_eq!(code(&o), 2, "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("fallow --help"),
            "{args:?}: {}",
            stderr(&o)
        );
        assert!(stdout(&o).is_empty());
    }
}

#[test]
fn print_config_paths_uses_config_dir() {
    let dir = temp_dir("paths");
    std::fs::write(dir.join("signatures.toml"), "").unwrap();
    let d = dir.to_str().unwrap();
    let joined = format!("--config-dir={d}");
    for args in [
        vec!["--config-dir", d, "--print-config-paths"],
        vec![&joined, "--print-config-paths"],
    ] {
        let o = fallow(&args);
        assert_eq!(code(&o), 0, "{}", stderr(&o));
        let text = stdout(&o);
        assert!(text.contains(&format!("config directory: {d}")), "{text}");
        assert!(text.contains("settings.toml (not present)"), "{text}");
        assert!(text.contains("signatures.toml (present)"), "{text}");
        assert!(text.contains("providers.toml (not present)"), "{text}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn print_config_paths_defaults_to_platform_dir() {
    let o = fallow(&["--print-config-paths"]);
    assert_eq!(code(&o), 0);
    let text = stdout(&o);
    let first = text.lines().next().unwrap_or_default().replace('\\', "/");
    assert!(
        first.ends_with("/fallow") || first.contains("(none"),
        "{text}"
    );
}

#[test]
fn unreadable_path_is_a_startup_failure() {
    let missing = std::env::temp_dir().join("fallow-cli-does-not-exist-9f2c");
    let o = fallow(&[missing.to_str().unwrap()]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(stderr(&o).contains("cannot open"));
}

#[test]
fn no_terminal_is_a_startup_failure() {
    let o = fallow(&[]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("needs a terminal"), "{}", stderr(&o));
}

#[test]
fn every_run_option_is_accepted() {
    // Accepted options reach the terminal check and fail there with 1, never with 2.
    let dir = temp_dir("accept");
    let d = dir.to_str().unwrap();
    let stats = dir.join("stats.txt");
    let s = stats.to_str().unwrap();
    for args in [
        &["--all-users"][..],
        &["--no-dns"],
        &["--config-dir", d],
        &["--bench-stats", s],
        &[d],
        &["--", d],
        &[
            "--all-users",
            "--no-dns",
            "--config-dir",
            d,
            "--bench-stats",
            s,
            d,
        ],
    ] {
        let o = fallow(args);
        assert_eq!(code(&o), 1, "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("needs a terminal"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn snapshot_options_are_parsed_but_not_implemented_yet() {
    // T048 implements them; until then they fail cleanly without starting the TUI.
    for args in [
        &["--snapshot-json"][..],
        &["--snapshot-hold", "1"],
        &["--snapshot-stream", "1"],
    ] {
        let o = fallow(args);
        assert_eq!(code(&o), 1, "{args:?}");
        assert!(stderr(&o).contains("not implemented yet"));
    }
}
