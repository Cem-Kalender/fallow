//! 012 T002: the fake agent's new keys (`expect_images`, `web`, `exec`), its reading of the ask
//! rules, `--tools` and stream-json input, and its argument log.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-fake012-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs the fake agent in `dir` with `args` and `stdin`; returns its stream-json lines.
fn run(dir: &Path, scenario: &str, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Vec<Value> {
    let sc = dir.join("s.toml");
    std::fs::write(&sc, scenario).unwrap();
    let _ = std::fs::remove_file(dir.join("s.used"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fallow-fake-agent"));
    cmd.current_dir(dir)
        .arg("--scenario")
        .arg(&sc)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn result(lines: &[Value]) -> &Value {
    lines.iter().rev().find(|l| l["type"] == "result").unwrap()
}

fn tool_results(lines: &[Value]) -> Vec<(bool, String)> {
    lines
        .iter()
        .filter(|l| l["type"] == "user")
        .flat_map(|l| {
            l["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|c| c["type"] == "tool_result")
        .map(|c| {
            (
                c["is_error"].as_bool().unwrap_or(false),
                c["content"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

const IMG_MSG: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Fallow step: assistant_turn\nlook"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}]}}"#;

#[test]
fn expect_images_counts_the_stream_json_image_blocks() {
    let d = scratch("img");
    let sc =
        "[[step]]\nkind = \"assistant_turn\"\nexpect_images = 1\noutput = { reply = \"seen\" }\n";
    let ok = run(&d, sc, &["--input-format", "stream-json"], IMG_MSG, &[]);
    assert_eq!(result(&ok)["is_error"], false, "{ok:?}");
    assert_eq!(result(&ok)["structured_output"]["reply"], "seen");
    let sc2 = "[[step]]\nkind = \"assistant_turn\"\nexpect_images = 2\n";
    let bad = run(&d, sc2, &["--input-format", "stream-json"], IMG_MSG, &[]);
    assert_eq!(result(&bad)["is_error"], true);
    assert!(
        result(&bad)["result"]
            .as_str()
            .unwrap()
            .contains("expected 2 images, got 1")
    );
    // Without stream-json input there are no images.
    let none = run(&d, sc, &[], "Fallow step: assistant_turn\nlook", &[]);
    assert_eq!(result(&none)["is_error"], true);
}

#[test]
fn web_steps_are_web_fetch_calls_refused_when_the_tool_is_off() {
    let d = scratch("web");
    let sc = "[[step]]\nkind = \"assistant_turn\"\nweb = [\"https://example.com\"]\n";
    let on = run(&d, sc, &[], "Fallow step: assistant_turn\n", &[]);
    let uses: Vec<&Value> = on
        .iter()
        .flat_map(|l| l["message"]["content"].as_array().into_iter().flatten())
        .filter(|c| c["type"] == "tool_use" && c["name"] == "WebFetch")
        .collect();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0]["input"]["url"], "https://example.com");
    assert_eq!(tool_results(&on), vec![(false, "fetched".to_string())]);
    let off = run(
        &d,
        sc,
        &["--disallowedTools", "WebFetch", "WebSearch"],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(tool_results(&off)[0].0);
    let not_listed = run(
        &d,
        sc,
        &["--tools", "Read,Bash"],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(tool_results(&not_listed)[0].0);
}

#[test]
fn exec_runs_commands_through_the_rules_ask_beating_allow() {
    let d = scratch("exec");
    let sc = "[[step]]\nkind = \"assistant_turn\"\nexec = [\"echo made > made.txt\"]\n";
    // An allow rule runs it.
    let lines = run(
        &d,
        sc,
        &[
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            "Bash(echo:*)",
        ],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(!tool_results(&lines)[0].0, "{lines:?}");
    assert!(d.join("made.txt").exists());
    std::fs::remove_file(d.join("made.txt")).unwrap();
    // A whole-tool ask rule beats it; with no prompt tool it is refused, as Claude Code does.
    let ask = r#"{"permissions":{"ask":["Bash","PowerShell","Edit","Write","MultiEdit","NotebookEdit"]}}"#;
    let lines = run(
        &d,
        sc,
        &[
            "--permission-mode",
            "default",
            "--allowedTools",
            "Bash(echo:*)",
            "--settings",
            ask,
        ],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(tool_results(&lines)[0].0);
    assert!(!d.join("made.txt").exists());
    assert_eq!(
        result(&lines)["permission_denials"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A deny rule wins, and so does a tool list without Bash.
    let lines = run(
        &d,
        sc,
        &[
            "--allowedTools",
            "Bash",
            "--disallowedTools",
            "Bash(echo:*)",
        ],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(tool_results(&lines)[0].0);
    let lines = run(
        &d,
        sc,
        &["--allowedTools", "Bash", "--tools", "Read"],
        "Fallow step: assistant_turn\n",
        &[],
    );
    assert!(tool_results(&lines)[0].0);
    assert!(!d.join("made.txt").exists());
}

#[test]
fn the_argument_log_records_every_invocation() {
    let d = scratch("args");
    let log = d.join("args.log");
    let sc = "[[step]]\nkind = \"assistant_turn\"\n[[step]]\nkind = \"assistant_turn\"\n";
    let l = log.display().to_string();
    run(
        &d,
        sc,
        &["--setting-sources", "user"],
        "Fallow step: assistant_turn\n",
        &[("FALLOW_FAKE_ARGS_LOG", &l)],
    );
    run(
        &d,
        sc,
        &["--model", "haiku"],
        "Fallow step: assistant_turn\n",
        &[("FALLOW_FAKE_ARGS_LOG", &l)],
    );
    let text = std::fs::read_to_string(&log).unwrap();
    let calls: Vec<&str> = text
        .split("---\n")
        .filter(|c| !c.trim().is_empty())
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("--setting-sources\nuser\n"));
    assert!(calls[1].contains("--model\nhaiku\n"));
}
