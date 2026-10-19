//! 002 T007: agent profiles (FR-023, FR-024, FR-027, contracts/config.md).

use std::path::Path;

use fallow::session::profiles::{Shim, builtins, load, parse_npm_shim, resolve};

#[test]
fn built_in_profiles() {
    let ids: Vec<String> = builtins().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["claude", "codex", "gemini", "aider", "shell"]);
    let b = builtins();
    assert_eq!(b[0].command, "claude");
    assert_eq!(b[1].command, "codex");
    assert_eq!(b[2].command, "gemini");
    assert_eq!(b[3].command, "aider");
    assert!(b.iter().all(|p| p.builtin && p.env.is_empty()));
}

#[test]
fn user_profiles_merge_by_id() {
    let text = r#"
[[profile]]
id = "claude"
name = "Claude (opus)"
command = "claude"
args = ["--model", "opus"]

[[profile]]
id = "aider"
enabled = false

[[profile]]
id = "my-agent"
name = "Mine"
command = "/opt/mine/bin/agent"
[profile.env]
API_TOKEN = "secret-value-123"
"#;
    let (ps, notices) = load(Some(("profiles.toml", text)));
    assert!(notices.is_empty(), "{notices:?}");
    let ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["claude", "codex", "gemini", "shell", "my-agent"]);
    let claude = &ps[0];
    assert_eq!(claude.name, "Claude (opus)");
    assert_eq!(claude.args, ["--model", "opus"]);
    assert!(
        !claude.builtin,
        "a user entry replaces the built-in completely"
    );
    let mine = ps.iter().find(|p| p.id == "my-agent").unwrap();
    assert_eq!(mine.env_names(), ["API_TOKEN"]);
    // FR-027: values never appear in Debug output.
    let dbg = format!("{mine:?} {ps:?}");
    assert!(!dbg.contains("secret-value-123"), "{dbg}");
    assert!(dbg.contains("API_TOKEN"));
}

#[test]
fn invalid_entries_are_reported_with_file_and_line() {
    let text = "[[profile]]\nid = \"Bad Id\"\ncommand = \"x\"\nname = \"x\"\n\n[[profile]]\nid = \"empty\"\nname = \"e\"\ncommand = \"  \"\n\n[[profile]]\nid = \"envbad\"\nname = \"n\"\ncommand = \"c\"\n[profile.env]\nSECRETX = 5\n\n[[profile]]\nid = \"long\"\nname = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ncommand = \"c\"\n";
    let (ps, notices) = load(Some(("profiles.toml", text)));
    let msgs: Vec<String> = notices.iter().map(ToString::to_string).collect();
    assert_eq!(msgs.len(), 4, "{msgs:?}");
    assert!(msgs[0].starts_with("profiles.toml:1:") && msgs[0].contains("[a-z0-9_-]{1,32}"));
    assert!(msgs[1].starts_with("profiles.toml:6:") && msgs[1].contains("command"));
    assert!(
        msgs[2].contains("env") && !msgs[2].contains('5'),
        "value not echoed"
    );
    assert!(msgs[3].contains("1..=64"));
    assert_eq!(ps.len(), 5, "only the built-ins remain");
    let (ps, notices) = load(Some(("profiles.toml", "[[profile]\n")));
    assert_eq!(ps.len(), 5);
    assert!(notices[0].to_string().starts_with("profiles.toml:1:"));
}

#[test]
fn repeated_ids_use_the_later_entry_and_warn() {
    let text = "[[profile]]\nid = \"x\"\nname = \"first\"\ncommand = \"a\"\n[[profile]]\nid = \"x\"\nname = \"second\"\ncommand = \"b\"\n";
    let (ps, notices) = load(Some(("profiles.toml", text)));
    assert_eq!(ps.iter().find(|p| p.id == "x").unwrap().name, "second");
    assert_eq!(notices.len(), 1);
    assert!(notices[0].to_string().contains("repeated"));
}

#[test]
fn npm_shims_resolve_to_the_program_they_run() {
    let dir = Path::new(r"C:\Users\u\AppData\Roaming\npm");
    let js = "@ECHO off\r\nGOTO start\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@google\\gemini-cli\\dist\\index.js\" %*\r\n";
    assert_eq!(
        parse_npm_shim(js, dir),
        Some(Shim::Node(
            dir.join("node_modules")
                .join("@google")
                .join("gemini-cli")
                .join("dist")
                .join("index.js")
        ))
    );
    let exe = "@ECHO off\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";
    assert_eq!(
        parse_npm_shim(exe, dir),
        Some(Shim::Exe(
            dir.join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin")
                .join("claude.exe")
        ))
    );
    assert_eq!(parse_npm_shim("@echo hello\r\n", dir), None);
}

#[test]
fn missing_commands_say_so() {
    let e = resolve("definitely-not-an-agent-xyz").unwrap_err();
    assert!(e.contains("not found"), "{e}");
    // The current executable can be resolved by absolute path.
    let me = std::env::current_exe().unwrap();
    assert_eq!(resolve(&me.display().to_string()).unwrap().program, me);
}
