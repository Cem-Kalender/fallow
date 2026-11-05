//! The project's squad file, `.fallow/squad.toml` (contracts/config.md). Fallow reads it at the
//! run's starting commit and never writes it. Roles are fixed (planner, builder, reviewer), so
//! 004's `[[role]]` tables and `[merge]` are no longer read.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::settings::{Autonomy, ROLES, RoleChoice};
use super::{Level, Notice};

pub const SQUAD_FILE: &str = ".fallow/squad.toml";

/// How a gate's failing checks are named, for comparing with the baseline (004 research R-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failures {
    /// Recognise common test runners' output.
    Auto,
    /// Only the exit code counts.
    ExitCode,
    /// The first group of this pattern names a failing check, per line.
    Regex(String),
}

/// A command Fallow runs itself (constitution I).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDef {
    pub name: String,
    pub command: String,
    pub timeout_min: u64,
    pub baseline: bool,
    pub failures: Failures,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    name: String,
    command: String,
    windows: Option<String>,
    macos: Option<String>,
    linux: Option<String>,
    timeout_min: Option<u64>,
    baseline: Option<bool>,
    failures: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct Raw {
    repair_cap: Option<u32>,
    max_parallel_builders: Option<u32>,
    test_patterns: Option<Vec<String>>,
    autonomy: Option<Autonomy>,
    limit_usd: Option<f64>,
    limit_min: Option<u64>,
    gate: Vec<RawGate>,
    roles: BTreeMap<String, RoleChoice>,
    allow: BTreeMap<String, Vec<String>>,
    /// 011: kept as a table and read by hand, so an unknown key under it is a notice, not a
    /// whole squad file refused.
    updates: Option<toml::Table>,
    /// 012 (research R-7): `[instructions] extra = [...]`, read the same way.
    instructions: Option<toml::Table>,
}

const KNOWN: &[&str] = &[
    "repair_cap",
    "max_parallel_builders",
    "test_patterns",
    "autonomy",
    "limit_usd",
    "limit_min",
    "gate",
    "roles",
    "allow",
    "updates",
    "instructions",
];
const LEGACY: &[&str] = &[
    "name",
    "publish",
    "role",
    "merge",
    "wait_notice_min",
    "signal_unanswered_min",
    "run_idle_end_min",
    "target_branch",
];

/// The default test file patterns (005), used to tell a part's tests from its code.
pub const DEFAULT_TEST_PATTERNS: &[&str] = &[
    "tests/**",
    "test/**",
    "**/*_test.*",
    "**/*.test.*",
    "**/test_*.py",
    "**/*_tests.*",
    "**/*Test.*",
    "**/*Tests.*",
];

/// What the project's squad file says, everything optional except the gates it declares.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Squad {
    pub gates: Vec<GateDef>,
    pub repair_cap: Option<u32>,
    pub max_parallel_builders: Option<u32>,
    pub test_patterns: Vec<String>,
    pub autonomy: Option<Autonomy>,
    pub limit_usd: Option<f64>,
    pub limit_min: Option<u64>,
    pub roles: BTreeMap<String, RoleChoice>,
    /// Role → extra permission rules (`Bash(npm run lint:*)`), research R-4.
    pub allow: BTreeMap<String, Vec<String>>,
    /// 011: the parts of an update that act on their own, each switchable (FR-016c, R-8b).
    pub updates: UpdateSwitches,
    /// 012: further instruction files handed to every agent (`[instructions] extra`).
    pub instructions_extra: Vec<String>,
}

/// 011: `[updates]` (FR-016c): Free's own updates, the merge line, the overlap warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateSwitches {
    pub auto: bool,
    pub line: bool,
    pub overlap: bool,
}

impl Default for UpdateSwitches {
    fn default() -> Self {
        UpdateSwitches {
            auto: true,
            line: true,
            overlap: true,
        }
    }
}

fn os_command(g: &RawGate) -> String {
    let over = if cfg!(windows) {
        g.windows.as_ref()
    } else if cfg!(target_os = "macos") {
        g.macos.as_ref()
    } else {
        g.linux.as_ref()
    };
    over.filter(|c| !c.trim().is_empty())
        .unwrap_or(&g.command)
        .clone()
}

impl Squad {
    /// Parses a squad file. Problems are notices; a gate with a problem is left out.
    pub fn parse(file: &str, text: &str, notices: &mut Vec<Notice>) -> Squad {
        let Some(raw) = super::merge::parse::<Raw>(file, text, KNOWN, LEGACY, notices) else {
            return Squad {
                test_patterns: DEFAULT_TEST_PATTERNS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                ..Squad::default()
            };
        };
        let mut updates = UpdateSwitches::default();
        if let Some(t) = &raw.updates {
            for (k, v) in t {
                let slot = match k.as_str() {
                    "auto" => &mut updates.auto,
                    "line" => &mut updates.line,
                    "overlap" => &mut updates.overlap,
                    _ => {
                        notices.push(Notice {
                            file: file.into(),
                            line: None,
                            level: Level::Warning,
                            message: format!(
                                "unknown key updates.{k}: auto, line and overlap are known"
                            ),
                        });
                        continue;
                    }
                };
                match v.as_bool() {
                    Some(b) => *slot = b,
                    None => notices.push(Notice {
                        file: file.into(),
                        line: None,
                        level: Level::Warning,
                        message: format!("updates.{k} must be true or false; kept on"),
                    }),
                }
            }
        }
        let mut instructions_extra = Vec::new();
        if let Some(t) = &raw.instructions {
            for (k, v) in t {
                match (k.as_str(), v.as_array()) {
                    ("extra", Some(a)) => {
                        instructions_extra =
                            a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
                    }
                    _ => notices.push(Notice {
                        file: file.into(),
                        line: None,
                        level: Level::Warning,
                        message: format!(
                            "instructions.{k}: only `extra`, a list of paths, is known"
                        ),
                    }),
                }
            }
        }
        let mut err = |m: String| {
            notices.push(Notice {
                file: file.into(),
                line: None,
                level: Level::Error,
                message: m,
            })
        };
        let mut gates: Vec<GateDef> = Vec::new();
        for g in &raw.gate {
            if g.name.trim().is_empty() || g.command.trim().is_empty() {
                err("a gate needs a name and a command".into());
                continue;
            }
            if gates.iter().any(|x| x.name == g.name) {
                err(format!("gate {:?} is declared twice", g.name));
                continue;
            }
            let failures = match g.failures.as_deref() {
                None | Some("auto") => Failures::Auto,
                Some("exit-code") => Failures::ExitCode,
                Some(p) => match regex::Regex::new(p) {
                    Ok(_) => Failures::Regex(p.to_string()),
                    Err(e) => {
                        err(format!("gate {:?}: failures: {e}", g.name));
                        Failures::Auto
                    }
                },
            };
            gates.push(GateDef {
                name: g.name.clone(),
                command: os_command(g),
                timeout_min: g.timeout_min.unwrap_or(30).clamp(1, 240),
                baseline: g.baseline.unwrap_or(true),
                failures,
            });
        }
        for r in raw.roles.keys().chain(raw.allow.keys()) {
            if !ROLES.contains(&r.as_str()) {
                err(format!("{r}: roles are planner, builder and reviewer"));
            }
        }
        let test_patterns = raw.test_patterns.unwrap_or_else(|| {
            DEFAULT_TEST_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
        for p in &test_patterns {
            if globset::Glob::new(p).is_err() {
                err(format!("test_patterns: bad glob {p:?}"));
            }
        }
        Squad {
            gates,
            repair_cap: raw.repair_cap,
            max_parallel_builders: raw.max_parallel_builders.map(|n| n.max(1)),
            test_patterns,
            autonomy: raw.autonomy,
            limit_usd: raw.limit_usd.filter(|u| *u > 0.0),
            limit_min: raw.limit_min.filter(|m| *m > 0),
            roles: raw
                .roles
                .into_iter()
                .filter(|(r, _)| ROLES.contains(&r.as_str()))
                .collect(),
            allow: raw
                .allow
                .into_iter()
                .filter(|(r, _)| ROLES.contains(&r.as_str()))
                .collect(),
            updates,
            instructions_extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sandbox_squad_file_parses_with_notices_for_old_keys() {
        let text = r#"
name = "default"
repair_cap = 2
max_parallel_builders = 2

[[role]]
name = "lead"

[[gate]]
name = "tests"
command = "python -m unittest"
linux = "python3 -m unittest"
timeout_min = 30
baseline = true
failures = "auto"

[merge]
requires = ["gate:tests"]

[roles.reviewer]
model = "opus"

[allow]
builder = ["Bash(npm run lint:*)"]
"#;
        let mut n = Vec::new();
        let s = Squad::parse("squad.toml", text, &mut n);
        assert_eq!(s.gates.len(), 1);
        assert_eq!(s.gates[0].name, "tests");
        if cfg!(windows) {
            assert_eq!(s.gates[0].command, "python -m unittest");
        }
        assert_eq!(s.repair_cap, Some(2));
        assert_eq!(s.roles["reviewer"].model, "opus");
        assert_eq!(s.allow["builder"], vec!["Bash(npm run lint:*)"]);
        assert!(!s.test_patterns.is_empty());
        let old: Vec<&str> = n.iter().map(|x| x.message.as_str()).collect();
        assert!(
            old.iter().any(|m| m.starts_with("name is no longer read")),
            "{old:?}"
        );
        assert!(
            old.iter().any(|m| m.starts_with("role is no longer read")),
            "{old:?}"
        );
        assert!(
            old.iter().any(|m| m.starts_with("merge is no longer read")),
            "{old:?}"
        );
    }

    #[test]
    fn bad_gates_are_left_out() {
        let mut n = Vec::new();
        let s = Squad::parse(
            "squad.toml",
            "[[gate]]\nname = \"a\"\ncommand = \"\"\n[[gate]]\nname = \"b\"\ncommand = \"x\"\nfailures = \"(\"\n",
            &mut n,
        );
        assert_eq!(s.gates.len(), 1);
        assert_eq!(s.gates[0].failures, Failures::Auto);
        assert_eq!(n.iter().filter(|x| x.level == Level::Error).count(), 2);
    }

    #[test]
    fn instructions_extra_is_a_list_of_paths() {
        let mut n = Vec::new();
        let s = Squad::parse(
            "squad.toml",
            "[instructions]
extra = [\"docs/AGENT_NOTES.md\"]
other = 1
",
            &mut n,
        );
        assert_eq!(s.instructions_extra, ["docs/AGENT_NOTES.md"]);
        assert!(n.iter().any(|x| x.message.contains("instructions.other")), "{n:?}");
    }

    #[test]
    fn updates_switches_parse_with_notices_for_unknown_keys() {
        let mut n = Vec::new();
        let s = Squad::parse(
            "squad.toml",
            "[updates]\nauto = false\noverlap = false\nlinee = true\n\n[[gate]]\nname = \"t\"\ncommand = \"x\"\n",
            &mut n,
        );
        assert_eq!(
            s.updates,
            UpdateSwitches {
                auto: false,
                line: true,
                overlap: false
            }
        );
        assert_eq!(s.gates.len(), 1, "the rest of the file is kept");
        assert!(
            n.iter()
                .any(|x| x.message.contains("unknown key updates.linee")),
            "{n:?}"
        );
        let d = Squad::parse("squad.toml", "", &mut Vec::new());
        assert_eq!(d.updates, UpdateSwitches::default());
    }
}
