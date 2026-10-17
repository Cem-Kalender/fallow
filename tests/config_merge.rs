//! Config loading and merging (FR-021, research R-9, contracts/config.md).

use fallow::config::{self, Config, CwdPolicy, Kind, Level, Settings, UserFile};

fn settings(text: &str) -> Config {
    config::load_from(
        Some(UserFile {
            name: "settings.toml",
            text,
        }),
        None,
        None,
    )
}

fn signatures(text: &str) -> Config {
    config::load_from(
        None,
        Some(UserFile {
            name: "signatures.toml",
            text,
        }),
        None,
    )
}

fn providers(text: &str) -> Config {
    config::load_from(
        None,
        None,
        Some(UserFile {
            name: "providers.toml",
            text,
        }),
    )
}

fn errors(c: &Config) -> Vec<String> {
    c.notices
        .iter()
        .filter(|n| n.level == Level::Error)
        .map(|n| n.to_string())
        .collect()
}

fn warnings(c: &Config) -> Vec<String> {
    c.notices
        .iter()
        .filter(|n| n.level == Level::Warning)
        .map(|n| n.to_string())
        .collect()
}

fn has(list: &[String], needle: &str) -> bool {
    list.iter().any(|s| s.contains(needle))
}

#[test]
fn built_in_defaults_are_valid_and_match_the_contract() {
    let c = config::load_from(None, None, None);
    assert!(
        c.notices.is_empty(),
        "built-in defaults produced notices: {:?}",
        c.notices
    );
    assert_eq!(c.settings, Settings::default());
    assert_eq!(c.settings.refresh_interval_ms, 2000);
    assert_eq!(c.settings.recent_window_minutes, 30);
    assert!(!c.settings.detection.browsers);
    assert!(c.settings.dns.enabled);
    assert_eq!(c.settings.dns.refresh_minutes, 15);
    assert_eq!(
        c.settings.windows.read_working_directory,
        CwdPolicy::OnDetect
    );
    assert_eq!(c.settings.editor.read_only_threshold_mb, 10);
    assert_eq!(c.settings.editor.tab_width, 4);
    assert!(c.settings.ui.mouse);

    let ids: Vec<&str> = c.signatures.iter().map(|s| s.id.as_str()).collect();
    for id in [
        "claude-code",
        "codex",
        "aider",
        "gemini-cli",
        "opencode",
        "goose",
        "ollama",
        "llama-server",
        "lm-studio",
        "vllm",
        "koboldcpp",
        "cursor",
        "windsurf",
        "chrome",
        "msedge",
        "firefox",
        "safari",
        "brave",
    ] {
        assert!(ids.contains(&id), "missing built-in signature {id}");
    }
    let pids: Vec<&str> = c.providers.iter().map(|p| p.id.as_str()).collect();
    for id in [
        "anthropic",
        "openai",
        "google",
        "mistral",
        "cohere",
        "groq",
        "deepseek",
        "openrouter",
        "xai",
    ] {
        assert!(pids.contains(&id), "missing built-in provider {id}");
    }
    for s in &c.signatures {
        assert_eq!(
            s.local_server.is_some(),
            s.kind == Kind::LocalModelServer,
            "{}",
            s.id
        );
    }
    for b in ["chrome", "msedge", "firefox", "safari", "brave"] {
        let s = c.signatures.iter().find(|s| s.id == b).unwrap();
        assert_eq!(s.kind, Kind::Browser);
    }
}

#[test]
fn built_in_signatures_match_typical_command_lines() {
    let c = config::load_from(None, None, None);
    let sig = |id: &str| c.signatures.iter().find(|s| s.id == id).unwrap().clone();
    let cmd = |id: &str, line: &str| sig(id).match_cmdline.unwrap().is_match(line);
    assert!(cmd(
        "claude-code",
        "node /usr/lib/node_modules/@anthropic-ai/claude-code/cli.js"
    ));
    assert!(cmd("claude-code", "/home/u/.local/bin/claude --resume"));
    assert!(!cmd("claude-code", "/usr/bin/claudette"));
    assert!(cmd("aider", "python3 -m aider --model sonnet"));
    assert!(cmd("goose", "goose session"));
    assert!(!cmd("goose", "goose postgres up"));
    let ollama = sig("ollama");
    assert!(ollama.match_name.unwrap().matcher.is_match("ollama"));
    let model = ollama.model_from_cmdline.unwrap();
    assert_eq!(
        &model.captures("ollama run llama3.2:3b").unwrap()[1],
        "llama3.2:3b"
    );
}

#[test]
fn user_entry_replaces_built_in_entry_completely() {
    let c = signatures(
        r#"
[[signature]]
id = "claude-code"
kind = "agent_cli"
match.name = "my-claude"
"#,
    );
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let s = c.signatures.iter().find(|s| s.id == "claude-code").unwrap();
    assert_eq!(s.match_name.as_ref().unwrap().pattern, "my-claude");
    assert!(
        s.match_cmdline.is_none(),
        "built-in cmdline must not survive the override"
    );
    assert!(
        s.provider.is_none(),
        "built-in provider must not survive the override"
    );
    assert_eq!(
        c.signatures
            .iter()
            .filter(|s| s.id == "claude-code")
            .count(),
        1
    );
}

#[test]
fn user_entry_with_new_id_is_added() {
    let c = signatures(
        r#"
[[signature]]
id = "my-agent"
kind = "script"
match.cmdline = 'my_agent\.py'
"#,
    );
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let builtin = config::load_from(None, None, None).signatures.len();
    assert_eq!(c.signatures.len(), builtin + 1);
    assert_eq!(c.signatures.last().unwrap().id, "my-agent");
}

#[test]
fn enabled_false_disables_a_built_in_entry() {
    let c = providers(
        r#"
[[provider]]
id = "openai"
hostnames = ["api.openai.com"]
enabled = false
"#,
    );
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let p = c.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!p.enabled);
    let c = signatures(
        r#"
[[signature]]
id = "cursor"
kind = "editor"
match.name = "cursor*"
enabled = false
"#,
    );
    assert!(
        !c.signatures
            .iter()
            .find(|s| s.id == "cursor")
            .unwrap()
            .enabled
    );
}

#[test]
fn refresh_interval_minimum_500() {
    let c = settings("refresh_interval_ms = 499\n");
    assert!(
        has(
            &errors(&c),
            "settings.toml:1: 499 is out of range: minimum 500"
        ),
        "{:?}",
        c.notices
    );
    assert_eq!(
        c.settings.refresh_interval_ms, 2000,
        "invalid value keeps the default"
    );
    let c = settings("refresh_interval_ms = 500\n");
    assert!(c.notices.is_empty());
    assert_eq!(c.settings.refresh_interval_ms, 500);
}

#[test]
fn dns_refresh_minutes_1_to_60() {
    for bad in [0, 61] {
        let c = settings(&format!("[dns]\nrefresh_minutes = {bad}\n"));
        assert!(has(&errors(&c), "settings.toml:2:"), "{:?}", c.notices);
        assert!(has(&errors(&c), "1..=60"));
        assert_eq!(c.settings.dns.refresh_minutes, 15);
    }
    for good in [1, 60] {
        let c = settings(&format!("[dns]\nrefresh_minutes = {good}\n"));
        assert!(c.notices.is_empty());
        assert_eq!(c.settings.dns.refresh_minutes, good);
    }
}

#[test]
fn partial_settings_keep_other_defaults() {
    let c = settings("[ui]\nmouse = false\n[windows]\nread_working_directory = \"never\"\n");
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    assert!(!c.settings.ui.mouse);
    assert_eq!(c.settings.windows.read_working_directory, CwdPolicy::Never);
    assert_eq!(c.settings.refresh_interval_ms, 2000);
}

#[test]
fn signature_requires_id_and_kind() {
    let c = signatures("[[signature]]\nkind = \"script\"\nmatch.name = \"x\"\n");
    assert!(
        has(&errors(&c), "signatures.toml:1: signature without `id`"),
        "{:?}",
        c.notices
    );
    let c = signatures("[[signature]]\nid = \"x\"\nmatch.name = \"x\"\n");
    assert!(has(
        &errors(&c),
        "signatures.toml:1: signature \"x\": `kind` is required"
    ));
    assert!(!c.signatures.iter().any(|s| s.id == "x"));
}

#[test]
fn signature_requires_a_match() {
    let c = signatures("[[signature]]\nid = \"x\"\nkind = \"script\"\n");
    assert!(
        has(
            &errors(&c),
            "needs at least one of `match.name` and `match.cmdline`"
        ),
        "{:?}",
        c.notices
    );
    assert!(!c.signatures.iter().any(|s| s.id == "x"));
}

#[test]
fn local_server_only_with_local_model_server_kind() {
    let c = signatures(
        "[[signature]]\nid = \"x\"\nkind = \"agent_cli\"\nmatch.name = \"x\"\nlocal_server.default_ports = [1]\n",
    );
    assert!(
        has(
            &errors(&c),
            "`local_server` is allowed only with kind = \"local_model_server\""
        ),
        "{:?}",
        c.notices
    );
    assert!(!c.signatures.iter().any(|s| s.id == "x"));
}

#[test]
fn invalid_signature_override_keeps_the_built_in_entry() {
    let c = signatures("[[signature]]\nid = \"ollama\"\nkind = \"local_model_server\"\n");
    assert_eq!(errors(&c).len(), 1, "{:?}", c.notices);
    let s = c.signatures.iter().find(|s| s.id == "ollama").unwrap();
    assert_eq!(s.match_name.as_ref().unwrap().pattern, "ollama*");
}

#[test]
fn invalid_regex_and_unknown_kind_and_unknown_provider() {
    let c = signatures("[[signature]]\nid = \"x\"\nkind = \"script\"\nmatch.cmdline = '('\n");
    assert!(has(
        &errors(&c),
        "signatures.toml:4: signature \"x\": invalid match.cmdline regex"
    ));
    let c = signatures("[[signature]]\nid = \"x\"\nkind = \"robot\"\nmatch.name = \"x\"\n");
    assert!(has(
        &errors(&c),
        "signatures.toml:3: signature \"x\": unknown kind \"robot\""
    ));
    let c = signatures(
        "[[signature]]\nid = \"x\"\nkind = \"script\"\nmatch.name = \"x\"\nprovider = \"nobody\"\n",
    );
    assert!(has(&errors(&c), "signatures.toml:5:"), "{:?}", c.notices);
    assert!(has(&errors(&c), "does not match any provider id"));
}

#[test]
fn provider_hostnames_must_be_plain_dns_names() {
    for (bad, why) in [
        ("*.openai.com", "must not contain wildcards"),
        ("104.18.0.1", "is an IP literal"),
        ("2606:4700::1", "is an IP literal"),
    ] {
        let c = providers(&format!(
            "[[provider]]\nid = \"x\"\nhostnames = [\"{bad}\"]\n"
        ));
        assert!(
            has(&errors(&c), "providers.toml:3:"),
            "{bad}: {:?}",
            c.notices
        );
        assert!(has(&errors(&c), why), "{bad}: {:?}", c.notices);
        assert!(!c.providers.iter().any(|p| p.id == "x"));
    }
}

#[test]
fn provider_ranges_must_be_cidr() {
    let c = providers(
        "[[provider]]\nid = \"x\"\nhostnames = [\"api.x.example\"]\nranges = [\"10.0.0.0/33\"]\n",
    );
    assert!(has(
        &errors(&c),
        "providers.toml:4: provider \"x\": range \"10.0.0.0/33\" is not valid CIDR"
    ));
    let c = providers("[[provider]]\nid = \"x\"\nranges = [\"10.1.0.0/16\", \"fd00::/8\"]\n");
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    assert_eq!(
        c.providers
            .iter()
            .find(|p| p.id == "x")
            .unwrap()
            .ranges
            .len(),
        2
    );
}

#[test]
fn provider_needs_hostnames_or_ranges() {
    let c = providers("[[provider]]\nid = \"x\"\ndisplay_name = \"X\"\n");
    assert!(has(
        &errors(&c),
        "providers.toml:1: provider \"x\": needs at least one hostname or range"
    ));
    assert!(!c.providers.iter().any(|p| p.id == "x"));
}

#[test]
fn syntax_error_reports_file_and_line_and_keeps_defaults() {
    let text = "[[provider]]\nid = \"openai\"\nhostnames = [\"api.example.com\"]\n\n[[provider]]\nid = \"broken\n";
    let c = providers(text);
    let errs = errors(&c);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].starts_with("providers.toml:6: "), "{errs:?}");
    let openai = c.providers.iter().find(|p| p.id == "openai").unwrap();
    assert_eq!(
        openai.hostnames,
        ["api.openai.com"],
        "the whole user file is ignored"
    );
    assert!(!c.providers.iter().any(|p| p.id == "broken"));
}

#[test]
fn type_error_reports_file_and_line() {
    let c = settings("\n\nrefresh_interval_ms = \"fast\"\n");
    let errs = errors(&c);
    assert!(
        errs.len() == 1 && errs[0].starts_with("settings.toml:3: "),
        "{errs:?}"
    );
    assert_eq!(c.settings, Settings::default());
}

#[test]
fn unknown_keys_are_warnings_with_lines() {
    let c =
        settings("refresh_interval_ms = 1000\ncolour = \"red\"\n[ui]\nmouse = false\ntheme = 1\n");
    let warns = warnings(&c);
    assert!(errors(&c).is_empty(), "{:?}", c.notices);
    assert!(
        has(&warns, "settings.toml:2: unknown key `colour`"),
        "{warns:?}"
    );
    assert!(
        has(&warns, "settings.toml:5: unknown key `ui.theme`"),
        "{warns:?}"
    );
    assert_eq!(
        c.settings.refresh_interval_ms, 1000,
        "known keys still apply"
    );
    assert!(!c.settings.ui.mouse);

    let c = signatures(
        "[[signature]]\nid = \"x\"\nkind = \"script\"\nmatch.name = \"x\"\nmatch.path = \"y\"\n",
    );
    assert!(has(
        &warnings(&c),
        "signatures.toml:5: unknown key `signature.match.path`"
    ));
    assert!(c.signatures.iter().any(|s| s.id == "x"));
}

#[test]
fn duplicate_ids_in_one_file_use_the_later_entry() {
    let c = providers(
        "[[provider]]\nid = \"x\"\nranges = [\"10.0.0.0/8\"]\n[[provider]]\nid = \"x\"\nranges = [\"11.0.0.0/8\"]\n",
    );
    assert!(
        has(&warnings(&c), "providers.toml:1: duplicate id \"x\""),
        "{:?}",
        c.notices
    );
    let x = c.providers.iter().find(|p| p.id == "x").unwrap();
    assert_eq!(x.ranges[0].to_string(), "11.0.0.0/8");
}

#[test]
fn files_are_read_from_the_config_dir() {
    let dir = std::env::temp_dir().join(format!("fallow-config-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.toml"), "refresh_interval_ms = 750\n").unwrap();
    let paths = config::ConfigPaths::resolve(Some(&dir));
    let c = config::load(&paths);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    assert_eq!(c.settings.refresh_interval_ms, 750);
}
