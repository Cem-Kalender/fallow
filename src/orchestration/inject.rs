//! Per-run MCP configuration for each agent style (003 research R-5, contracts/config.md).
//!
//! Fallow points an agent at `fallow mcp-bridge` for one run only, through the agent's own
//! command-line flags or (Gemini) a settings file of its own. It never edits the user's agent
//! configuration. Additions go at the end of the argument list: Claude Code's `--mcp-config` and
//! `--allowedTools` take several values, so nothing positional may follow them.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::identity::Token;
use crate::session::profiles::Style;

/// Where the bridge is and how it reaches this Fallow process.
#[derive(Debug, Clone)]
pub struct Bridge {
    pub exe: PathBuf,
    pub endpoint: String,
    pub fallow_pid: u32,
    /// For per-run files (Gemini); `None` when no cache directory is known.
    pub run_dir: Option<PathBuf>,
}

/// What a session's spawn gets added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Additions {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// A file to write (mode 0600) before the spawn and remove when the session ends.
    pub file: Option<(PathBuf, String)>,
}

/// The bridge's arguments for one session.
pub fn bridge_args(b: &Bridge, session: u32, token: &Token) -> Vec<String> {
    vec![
        "mcp-bridge".into(),
        "--endpoint".into(),
        b.endpoint.clone(),
        "--fallow-pid".into(),
        b.fallow_pid.to_string(),
        "--session".into(),
        session.to_string(),
        "--token".into(),
        token.hex(),
    ]
}

/// The additions for `style`, or why the session runs without orchestration.
pub fn additions(
    style: Style,
    b: &Bridge,
    session: u32,
    token: &Token,
    gemini_system_settings: &dyn Fn() -> bool,
) -> Result<Additions, String> {
    let exe = b.exe.to_string_lossy().into_owned();
    let args = bridge_args(b, session, token);
    match style {
        Style::None => Err("profile".into()),
        Style::Claude => {
            let cfg = json!({ "mcpServers": { "fallow": { "command": exe, "args": args } } });
            Ok(Additions {
                args: vec![
                    "--mcp-config".into(),
                    cfg.to_string(),
                    "--allowedTools".into(),
                    "mcp__fallow__*".into(),
                ],
                ..Additions::default()
            })
        }
        Style::Codex => {
            let list = args
                .iter()
                .map(|a| toml_string(a))
                .collect::<Vec<_>>()
                .join(",");
            let c = |kv: String| ["-c".to_string(), kv];
            let mut out = Vec::new();
            out.extend(c(format!(
                "mcp_servers.fallow.command={}",
                toml_string(&exe)
            )));
            out.extend(c(format!("mcp_servers.fallow.args=[{list}]")));
            out.extend(c("mcp_servers.fallow.tool_timeout_sec=120".into()));
            out.extend(c(
                r#"mcp_servers.fallow.default_tools_approval_mode="approve""#.into(),
            ));
            Ok(Additions {
                args: out,
                ..Additions::default()
            })
        }
        Style::Gemini => {
            // Replacing an administrator's system settings would drop their policy; Fallow
            // only checks whether such a file exists and never reads it.
            if gemini_system_settings() {
                return Err("Gemini system settings present".into());
            }
            let Some(dir) = &b.run_dir else {
                return Err("no cache folder for the Gemini settings file".into());
            };
            let path = dir.join(format!("gemini-{session}.json"));
            let cfg = json!({ "mcpServers": { "fallow": {
                "command": exe, "args": args, "trust": true, "timeout": 120000
            } } });
            Ok(Additions {
                env: vec![(
                    "GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(),
                    path.to_string_lossy().into_owned(),
                )],
                file: Some((path, cfg.to_string())),
                ..Additions::default()
            })
        }
    }
}

/// A TOML string: a literal string (no escapes, so Windows paths stay as they are) unless it
/// contains a quote or a control character, then a basic string with escapes.
pub fn toml_string(s: &str) -> String {
    if !s.contains('\'') && !s.chars().any(char::is_control) {
        format!("'{s}'")
    } else {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
}

/// Whether Gemini CLI would read a system settings file on this machine: the override variable
/// is set, or the platform's file exists.
pub fn gemini_system_settings_exist() -> bool {
    if std::env::var_os("GEMINI_CLI_SYSTEM_SETTINGS_PATH").is_some() {
        return true;
    }
    let path: PathBuf = if cfg!(windows) {
        Path::new(r"C:\ProgramData\gemini-cli\settings.json").into()
    } else if cfg!(target_os = "macos") {
        Path::new("/Library/Application Support/GeminiCli/settings.json").into()
    } else {
        Path::new("/etc/gemini-cli/settings.json").into()
    };
    path.exists()
}

/// Writes a per-run file readable only by the current user.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(text.as_bytes())
    }
    #[cfg(windows)]
    {
        // The run directory is under %LOCALAPPDATA%, whose inherited access list admits only the
        // user (and SYSTEM and administrators).
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge() -> Bridge {
        Bridge {
            exe: PathBuf::from(r"C:\Program Files\Fallow\fallow.exe"),
            endpoint: r"\\.\pipe\fallow-12-abcd".into(),
            fallow_pid: 12,
            run_dir: Some(PathBuf::from("/run/fallow")),
        }
    }

    fn token() -> Token {
        Token::from_bytes([7; 32])
    }

    #[test]
    fn claude_style_appends_mcp_config_and_allowed_tools() {
        let a = additions(Style::Claude, &bridge(), 3, &token(), &|| false).unwrap();
        assert_eq!(a.args.len(), 4);
        assert_eq!(a.args[0], "--mcp-config");
        assert_eq!(a.args[2], "--allowedTools");
        assert_eq!(a.args[3], "mcp__fallow__*");
        let v: serde_json::Value = serde_json::from_str(&a.args[1]).unwrap();
        let s = &v["mcpServers"]["fallow"];
        assert_eq!(s["command"], r"C:\Program Files\Fallow\fallow.exe");
        let args: Vec<&str> = s["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "mcp-bridge",
                "--endpoint",
                r"\\.\pipe\fallow-12-abcd",
                "--fallow-pid",
                "12",
                "--session",
                "3",
                "--token",
                &"07".repeat(32),
            ]
        );
        assert!(a.env.is_empty() && a.file.is_none());
    }

    #[test]
    fn codex_style_uses_toml_literal_strings() {
        let a = additions(Style::Codex, &bridge(), 3, &token(), &|| false).unwrap();
        assert_eq!(
            a.args[..4],
            [
                "-c".to_string(),
                r"mcp_servers.fallow.command='C:\Program Files\Fallow\fallow.exe'".into(),
                "-c".into(),
                format!(
                    r"mcp_servers.fallow.args=['mcp-bridge','--endpoint','\\.\pipe\fallow-12-abcd','--fallow-pid','12','--session','3','--token','{}']",
                    "07".repeat(32)
                ),
            ]
        );
        assert!(
            a.args
                .contains(&"mcp_servers.fallow.tool_timeout_sec=120".to_string())
        );
        // Each value parses as TOML to the original string.
        let v: toml::Table =
            toml::from_str(&format!("x = {}", toml_string(r"C:\a 'b'\c"))).unwrap();
        assert_eq!(v["x"].as_str(), Some(r"C:\a 'b'\c"));
    }

    #[test]
    fn gemini_style_writes_its_own_settings_file_unless_the_system_has_one() {
        let a = additions(Style::Gemini, &bridge(), 5, &token(), &|| false).unwrap();
        let (path, text) = a.file.unwrap();
        assert_eq!(path, Path::new("/run/fallow").join("gemini-5.json"));
        assert_eq!(a.env[0].0, "GEMINI_CLI_SYSTEM_SETTINGS_PATH");
        assert_eq!(a.env[0].1, path.to_string_lossy());
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mcpServers"]["fallow"]["trust"], true);
        assert_eq!(v["mcpServers"]["fallow"]["timeout"], 120000);
        assert!(a.args.is_empty());
        assert_eq!(
            additions(Style::Gemini, &bridge(), 5, &token(), &|| true).unwrap_err(),
            "Gemini system settings present"
        );
    }

    #[test]
    fn no_style_means_no_orchestration() {
        assert_eq!(
            additions(Style::None, &bridge(), 1, &token(), &|| false).unwrap_err(),
            "profile"
        );
    }
}
