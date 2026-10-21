//! 002 T007: agent profiles (FR-023, FR-024, FR-027, contracts/config.md).

use std::path::Path;

use fallow::app::state::AppState;
use fallow::app::workspace::Where;
use fallow::session::SessionState;
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

#[test]
fn built_ins_turn_on_notifications_for_the_run_only() {
    // Research R-4: per-run flags, listed in the profile.
    let b = builtins();
    let args = |id: &str| b.iter().find(|p| p.id == id).unwrap().args.clone();
    assert_eq!(
        args("claude"),
        ["--settings", r#"{"preferredNotifChannel":"terminal_bell"}"#]
    );
    assert_eq!(
        args("codex"),
        [
            "-c",
            "tui.notifications=true",
            "-c",
            r#"tui.notification_method="bel""#
        ]
    );
    assert!(args("gemini").is_empty());
    assert!(args("aider").is_empty());
    assert!(args("shell").is_empty());

    // The Claude Code value is inline JSON, not a settings file: nothing on disk is edited.
    let v: serde_json::Value = serde_json::from_str(&args("claude")[1]).unwrap();
    assert_eq!(v["preferredNotifChannel"], "terminal_bell");
    for p in &b {
        for a in &p.args {
            assert!(
                !a.ends_with(".json") && !a.ends_with(".toml") && !std::path::Path::new(a).exists(),
                "{}: {a:?} looks like a settings file",
                p.id
            );
        }
    }

    // The profile view lists each flag.
    let claude = &b[0];
    assert!(
        claude
            .details()
            .iter()
            .any(|d| d.contains("--settings") && d.contains("terminal_bell")),
        "{:?}",
        claude.details()
    );

    // Overriding the profile removes the flags.
    let text = "[[profile]]\nid = \"claude\"\nname = \"Claude Code\"\ncommand = \"claude\"\n";
    let (ps, notices) = load(Some(("profiles.toml", text)));
    assert!(notices.is_empty(), "{notices:?}");
    assert!(ps[0].args.is_empty());
}

#[test]
fn details_show_env_names_but_never_values() {
    let text = "[[profile]]\nid = \"mine\"\nname = \"Mine\"\ncommand = \"agent\"\nargs = [\"--fast\"]\n[profile.env]\nAPI_TOKEN = \"secret-value-456\"\n";
    let (ps, _) = load(Some(("profiles.toml", text)));
    let d = ps
        .iter()
        .find(|p| p.id == "mine")
        .unwrap()
        .details()
        .join("\n");
    assert!(d.contains("command: agent"), "{d}");
    assert!(d.contains("args: --fast"), "{d}");
    assert!(d.contains("API_TOKEN"), "{d}");
    assert!(!d.contains("secret-value-456"), "{d}");
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-prof-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::path::absolute(&d).unwrap()
}

#[test]
fn reload_configuration_reads_profiles_again() {
    let cfg = temp("reload");
    let mut s = AppState::new(None, Vec::new());
    s.store.dir = Some(cfg.clone());
    assert!(!s.profiles.iter().any(|p| p.id == "mine"));
    std::fs::write(
        cfg.join("profiles.toml"),
        "[[profile]]\nid = \"mine\"\nname = \"Mine\"\ncommand = \"agent\"\n",
    )
    .unwrap();
    s.run("config.reload");
    assert!(s.profiles.iter().any(|p| p.id == "mine"));
    assert!(s.notices.is_empty(), "{:?}", s.notices);

    // A broken file: the built-ins remain and the error is reported as file:line, once.
    std::fs::write(cfg.join("profiles.toml"), "[[profile]\n").unwrap();
    s.run("config.reload");
    s.run("config.reload");
    assert!(!s.profiles.iter().any(|p| p.id == "mine"));
    assert_eq!(s.profiles.len(), 5);
    let errors: Vec<&String> = s
        .notices
        .iter()
        .filter(|n| n.starts_with("profiles.toml:1:"))
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", s.notices);
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn a_failing_command_shows_its_error_in_the_session_only() {
    let d = temp("failing");
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    let (tx, _rx) = std::sync::mpsc::channel();
    s.events = Some(tx);
    let (ps, _) = load(Some((
        "profiles.toml",
        "[[profile]]\nid = \"broken\"\nname = \"Broken\"\ncommand = \"no-such-agent-xyz\"\n",
    )));
    s.profiles = ps;
    s.start_session_at("broken", Where::ProjectFolder).unwrap();
    let sess = &s.sessions.sessions[0];
    match &sess.state {
        SessionState::Failed(why) => assert!(why.contains("not found"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(s.notices.is_empty(), "not a global notice: {:?}", s.notices);
    assert_eq!(s.sessions.sessions.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn orchestration_style_per_profile() {
    // 003 T005, contracts/config.md "profiles.toml: orchestration".
    use fallow::session::profiles::Style;
    let styles: Vec<Style> = builtins().into_iter().map(|p| p.orchestration).collect();
    assert_eq!(
        styles,
        [
            Style::Claude,
            Style::Codex,
            Style::Gemini,
            Style::None,
            Style::None
        ]
    );
    let text = r#"
[[profile]]
id = "claude"
name = "Claude (opus)"
command = "claude"

[[profile]]
id = "wrapped"
name = "Wrapped Codex"
command = "my-codex"
orchestration = "codex"

[[profile]]
id = "plain"
name = "Plain"
command = "plain"

[[profile]]
id = "bad"
name = "Bad"
command = "bad"
orchestration = "telepathy"
"#;
    let (profiles, notices) = load(Some(("profiles.toml", text)));
    let get = |id: &str| {
        profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.orchestration)
    };
    assert_eq!(
        get("claude"),
        Some(Style::Claude),
        "a replaced built-in keeps its style"
    );
    assert_eq!(get("wrapped"), Some(Style::Codex));
    assert_eq!(
        get("plain"),
        Some(Style::None),
        "user profiles default to none"
    );
    assert_eq!(get("bad"), None, "an invalid value drops the entry");
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].line, Some(22));
    assert!(notices[0].message.contains("telepathy"));
}
