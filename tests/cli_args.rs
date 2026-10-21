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
fn snapshot_json_prints_one_snapshot_without_a_terminal() {
    let o = fallow(&["--snapshot-json", "--no-dns"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_str(stdout(&o).trim()).expect("valid JSON");
    for field in ["taken_at", "groups", "llm", "records", "summary", "notes"] {
        assert!(v.get(field).is_some(), "missing {field}");
    }
    assert!(v["summary"]["total_ram"].as_u64().unwrap() > 0);
    let notes = v["notes"].as_array().unwrap();
    assert!(notes.iter().any(|n| {
        n["text"]
            .as_str()
            .unwrap()
            .starts_with("Not covered: containers")
    }));
    // Records are LLM group members only.
    let llm = v["llm"].as_array().unwrap().len();
    assert!(v["records"].as_array().unwrap().len() >= llm);
}

#[test]
fn snapshot_stream_prints_one_line_per_cycle() {
    let dir = temp_dir("stream");
    std::fs::write(dir.join("settings.toml"), "refresh_interval_ms = 500\n").unwrap();
    let o = fallow(&[
        "--snapshot-stream",
        "2",
        "--no-dns",
        "--config-dir",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let lines: Vec<serde_json::Value> = stdout(&o)
        .lines()
        .map(|l| serde_json::from_str(l).expect("JSON line"))
        .collect();
    assert!((3..=5).contains(&lines.len()), "{} lines", lines.len());
    for (i, l) in lines.iter().enumerate() {
        assert_eq!(l["cycle"].as_u64(), Some(i as u64 + 1));
        assert!(l["llm"].is_array());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn snapshot_hold_stays_alive() {
    let started = std::time::Instant::now();
    let o = fallow(&["--snapshot-hold", "1", "--no-dns"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert!(started.elapsed() >= std::time::Duration::from_secs(1));
    assert!(serde_json::from_str::<serde_json::Value>(stdout(&o).trim()).is_ok());
}

#[test]
fn mcp_bridge_is_internal_and_never_starts_the_tui() {
    // 003 T010, contracts/mcp-tools.md "Bridge invocation".
    let o = fallow(&["mcp-bridge"]);
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert!(stderr(&o).contains("--endpoint"));
    assert!(
        stdout(&o).is_empty(),
        "stdout carries only relayed messages"
    );
    let o = fallow(&[
        "mcp-bridge",
        "--endpoint",
        "x",
        "--fallow-pid",
        "1",
        "--session",
        "1",
        "--token",
        "zz",
    ]);
    assert_eq!(code(&o), 2);
    // Nothing listens there: exit 3 (cannot connect).
    let t = "ab".repeat(32);
    let endpoint = if cfg!(windows) {
        r"\.\pipe\fallow-nobody-here".to_string()
    } else {
        temp_dir("bridge")
            .join("none.sock")
            .to_string_lossy()
            .into_owned()
    };
    let o = fallow(&[
        "mcp-bridge",
        "--endpoint",
        &endpoint,
        "--fallow-pid",
        "1",
        "--session",
        "1",
        "--token",
        &t,
    ]);
    assert_eq!(code(&o), 3, "{}", stderr(&o));
    assert!(stdout(&o).is_empty());
    assert!(!stderr(&o).contains(&t), "the token is never printed");
}
