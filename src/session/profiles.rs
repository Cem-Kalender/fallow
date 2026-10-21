//! Agent profiles (002 FR-023, FR-024, data-model.md "AgentProfile", contracts/config.md).
//!
//! Built-in profiles are compiled in; `profiles.toml` overrides or extends them by `id`. A profile
//! is offered only when its command is found, which is checked when the start menu opens, never
//! at startup. Environment additions are passed to the agent; their values are never displayed
//! or logged (FR-027).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use crate::config::merge::line_of;
use crate::config::{Level, Notice};

pub const PROFILES_FILE: &str = "profiles.toml";

#[derive(Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// Environment additions: (name, value). Values are never shown.
    pub env: Vec<(String, String)>,
    pub builtin: bool,
    /// How Fallow connects the agent to its coordination tools for one run (003 research R-5).
    pub orchestration: Style,
}

/// How an agent is pointed at Fallow's MCP server for one run (003 contracts/config.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Claude Code's `--mcp-config`.
    Claude,
    /// Codex CLI's `-c mcp_servers.…` overrides.
    Codex,
    /// Gemini CLI's system settings path.
    Gemini,
    /// No way to add a tool server: the session runs without orchestration.
    #[default]
    None,
}

impl Style {
    pub fn parse(s: &str) -> Option<Style> {
        Some(match s {
            "claude" => Style::Claude,
            "codex" => Style::Codex,
            "gemini" => Style::Gemini,
            "none" => Style::None,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Style::Claude => "claude",
            Style::Codex => "codex",
            Style::Gemini => "gemini",
            Style::None => "none",
        }
    }
}

impl std::fmt::Debug for AgentProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProfile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &self.env_names())
            .field("builtin", &self.builtin)
            .field("orchestration", &self.orchestration)
            .finish()
    }
}

impl AgentProfile {
    /// The names of the environment additions (what the UI may show).
    pub fn env_names(&self) -> Vec<&str> {
        self.env.iter().map(|(k, _)| k.as_str()).collect()
    }
}

fn builtin(id: &str, name: &str, command: &str, args: &[&str], style: Style) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        name: name.into(),
        command: command.into(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
        env: Vec::new(),
        builtin: true,
        orchestration: style,
    }
}

/// Claude Code: ring the terminal bell when it needs the user, for this run only (research R-4;
/// its `auto` channel only notifies in iTerm2, Ghostty and Kitty).
pub const CLAUDE_NOTIFY_ARGS: [&str; 2] =
    ["--settings", r#"{"preferredNotifChannel":"terminal_bell"}"#];

/// Codex CLI: notifications on, delivered as BEL, for this run only (research R-4).
pub const CODEX_NOTIFY_ARGS: [&str; 4] = [
    "-c",
    "tui.notifications=true",
    "-c",
    r#"tui.notification_method="bel""#,
];

/// The command of the built-in "shell" profile.
pub fn shell_command() -> String {
    if cfg!(windows) {
        "pwsh.exe|powershell.exe".into()
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| s.starts_with('/'))
            .unwrap_or_else(|| "/bin/sh".into())
    }
}

/// The built-in profiles (contracts/config.md table). Claude Code and Codex get the per-run
/// notification flags of research R-4; no agent settings file is ever edited. Gemini CLI has no
/// such flag (its window titles carry the status), and Aider's `--notifications-command` is not
/// verified, so they rely on the bell, titles and the quiet fallback. Overriding a profile by
/// `id` removes its flags.
pub fn builtins() -> Vec<AgentProfile> {
    vec![
        builtin(
            "claude",
            "Claude Code",
            "claude",
            &CLAUDE_NOTIFY_ARGS,
            Style::Claude,
        ),
        builtin(
            "codex",
            "Codex CLI",
            "codex",
            &CODEX_NOTIFY_ARGS,
            Style::Codex,
        ),
        builtin("gemini", "Gemini CLI", "gemini", &[], Style::Gemini),
        builtin("aider", "Aider", "aider", &[], Style::None),
        builtin("shell", "Shell", &shell_command(), &[], Style::None),
    ]
}

impl AgentProfile {
    /// What the start menu shows about a profile: the command, its arguments and the names of
    /// its environment additions, never their values (FR-024, FR-027).
    pub fn details(&self) -> Vec<String> {
        let mut out = vec![format!("command: {}", self.command.replace('|', " or "))];
        if !self.args.is_empty() {
            out.push(format!("args: {}", self.args.join(" ")));
        }
        if !self.env.is_empty() {
            out.push(format!(
                "env: {} (values hidden)",
                self.env_names().join(", ")
            ));
        }
        if self.orchestration != Style::None {
            out.push(format!("orchestration: {}", self.orchestration.as_str()));
        }
        out.push(if self.builtin {
            "built-in; override by id in profiles.toml".into()
        } else {
            "from profiles.toml".into()
        });
        out
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawFile {
    #[serde(default)]
    profile: Vec<Spanned<RawProfile>>,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    id: Option<Spanned<String>>,
    name: Option<Spanned<String>>,
    command: Option<Spanned<String>>,
    #[serde(default)]
    args: Vec<String>,
    enabled: Option<bool>,
    orchestration: Option<Spanned<String>>,
    #[serde(default)]
    env: toml::Table,
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn valid_env_name(n: &str) -> bool {
    let mut b = n.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Merges `profiles.toml` (if any) into the built-ins. Problems become `file:line` notices; the
/// rest still applies.
pub fn load(file: Option<(&str, &str)>) -> (Vec<AgentProfile>, Vec<Notice>) {
    let mut out = builtins();
    let mut notices = Vec::new();
    let Some((name, text)) = file else {
        return (out, notices);
    };
    let err = |line: Option<usize>, message: String| Notice {
        file: name.to_string(),
        line,
        level: Level::Error,
        message,
    };
    let raw: RawFile = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            notices.push(err(
                line,
                format!("{} (user profiles are ignored)", e.message()),
            ));
            return (out, notices);
        }
    };
    let mut seen: Vec<(String, usize)> = Vec::new();
    for entry in raw.profile {
        let line = line_of(text, entry.span().start);
        let p = entry.into_inner();
        let Some(id) = p.id.map(Spanned::into_inner) else {
            notices.push(err(Some(line), "profile needs an id".into()));
            continue;
        };
        if !valid_id(&id) {
            notices.push(err(
                Some(line),
                format!("id {id:?}: expected [a-z0-9_-]{{1,32}}"),
            ));
            continue;
        }
        if let Some((_, earlier)) = seen.iter().find(|(s, _)| *s == id) {
            notices.push(Notice {
                file: name.to_string(),
                line: Some(*earlier),
                level: Level::Warning,
                message: format!("id {id:?} is repeated on line {line}; the later entry is used"),
            });
        }
        seen.push((id.clone(), line));
        if p.enabled == Some(false) {
            out.retain(|q| q.id != id);
            continue;
        }
        let base = out.iter().find(|q| q.id == id).cloned();
        let name_v = p
            .name
            .map(Spanned::into_inner)
            .or(base.as_ref().map(|b| b.name.clone()));
        let command = p.command.map(Spanned::into_inner);
        let Some(name_v) = name_v.filter(|n| (1..=64).contains(&n.chars().count())) else {
            notices.push(err(
                Some(line),
                format!("profile {id:?}: name must be 1..=64 chars"),
            ));
            continue;
        };
        let Some(command) = command.filter(|c| !c.trim().is_empty()) else {
            notices.push(err(
                Some(line),
                format!("profile {id:?}: command must not be empty"),
            ));
            continue;
        };
        let mut env = Vec::new();
        let mut bad_env = false;
        for (k, v) in p.env {
            match (valid_env_name(&k), v) {
                (true, toml::Value::String(s)) => env.push((k, s)),
                _ => {
                    // The value is not echoed (FR-027).
                    notices.push(err(
                        Some(line),
                        format!(
                            "profile {id:?}: env {k:?} needs a name like [A-Za-z_][A-Za-z0-9_]* and a string value"
                        ),
                    ));
                    bad_env = true;
                }
            }
        }
        if bad_env {
            continue;
        }
        // A user profile replacing a built-in keeps the built-in's style unless it names one.
        let orchestration = match p.orchestration {
            None => base.as_ref().map_or(Style::None, |b| b.orchestration),
            Some(s) => match Style::parse(s.get_ref()) {
                Some(st) => st,
                None => {
                    notices.push(err(
                        Some(line_of(text, s.span().start)),
                        format!(
                            "profile {id:?}: orchestration = {:?}: expected \"claude\", \"codex\", \"gemini\" or \"none\"",
                            s.get_ref()
                        ),
                    ));
                    continue;
                }
            },
        };
        let profile = AgentProfile {
            id: id.clone(),
            name: name_v,
            command,
            args: p.args,
            env,
            builtin: false,
            orchestration,
        };
        match out.iter_mut().find(|q| q.id == id) {
            Some(q) => *q = profile,
            None => out.push(profile),
        }
    }
    (out, notices)
}

/// What to run for a profile command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub program: PathBuf,
    /// Arguments that go before the profile's own (the script of an npm shim).
    pub prefix_args: Vec<String>,
}

/// Finds `command` on `PATH` (with `PATHEXT` on Windows), or checks it as a path. On Windows an
/// npm `.cmd` shim is resolved to the program it runs, because running a batch file needs
/// `cmd.exe`, with its argument-escaping problem and its "Terminate batch job" prompt
/// (research R-7). `a|b` tries `a`, then `b`.
pub fn resolve(command: &str) -> Result<Resolved, String> {
    let mut last = String::new();
    for c in command.split('|') {
        match resolve_one(c.trim()) {
            Ok(r) => return Ok(r),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn resolve_one(command: &str) -> Result<Resolved, String> {
    let direct = |p: PathBuf| Resolved {
        program: p,
        prefix_args: Vec::new(),
    };
    let path = Path::new(command);
    let found = if path.components().count() > 1 || path.is_absolute() {
        candidates(path).into_iter().find(|p| p.is_file())
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .flat_map(|d| candidates(&d.join(command)))
                .find(|p| p.is_file())
        })
    };
    let Some(found) = found else {
        return Err(format!("{command}: not found on PATH"));
    };
    let ext = found
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if cfg!(windows) && matches!(ext.as_deref(), Some("cmd" | "bat")) {
        let text =
            std::fs::read_to_string(&found).map_err(|e| format!("{}: {e}", found.display()))?;
        let dir = found.parent().unwrap_or(Path::new("."));
        return match parse_npm_shim(&text, dir) {
            Some(Shim::Exe(p)) => Ok(direct(p)),
            Some(Shim::Node(script)) => {
                let node = [dir.join("node.exe")]
                    .into_iter()
                    .find(|p| p.is_file())
                    .map(Ok)
                    .unwrap_or_else(|| resolve_one("node").map(|r| r.program))?;
                Ok(Resolved {
                    program: node,
                    prefix_args: vec![script.display().to_string()],
                })
            }
            None => Err(format!(
                "{}: a batch file that is not an npm shim; point the profile at the program it runs",
                found.display()
            )),
        };
    }
    Ok(direct(found))
}

/// The file names to try for `p`: as given, then with each `PATHEXT` extension on Windows.
fn candidates(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    if cfg!(windows) && p.extension().is_none() {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        v.clear();
        for e in exts.split(';').filter(|e| !e.is_empty()) {
            v.push(p.with_extension(e.trim_start_matches('.')));
        }
    }
    v
}

/// What an npm `.cmd` shim runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shim {
    /// A native program.
    Exe(PathBuf),
    /// A JavaScript file run by node.
    Node(PathBuf),
}

/// Reads the target out of an npm-generated `.cmd` shim: the quoted `"%dp0%\…"` path on the line
/// that passes `%*`.
pub fn parse_npm_shim(text: &str, dir: &Path) -> Option<Shim> {
    let line = text.lines().rev().find(|l| l.contains("%*"))?;
    let marker = "\"%dp0%\\";
    let start = line.rfind(marker)? + marker.len();
    let end = start + line[start..].find('"')?;
    let rel = &line[start..end];
    let mut p = dir.to_path_buf();
    for part in rel.split('\\').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("exe") => Some(Shim::Exe(p)),
        Some("js" | "cjs" | "mjs") => Some(Shim::Node(p)),
        _ => None,
    }
}
