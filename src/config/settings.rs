//! Fallow's own settings, `<config dir>/fallow/settings.toml` (contracts/config.md).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{Level, Notice};

/// How much is decided without the engineer (FR-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Autonomy {
    /// The plan, the merge and what the planner cannot decide.
    High,
    /// Also changes to contracts or criteria.
    #[default]
    Medium,
    /// Also each repair round.
    Low,
}

impl Autonomy {
    pub fn parse(s: &str) -> Option<Autonomy> {
        match s {
            "high" => Some(Autonomy::High),
            "medium" => Some(Autonomy::Medium),
            "low" => Some(Autonomy::Low),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Autonomy::High => "high",
            Autonomy::Medium => "medium",
            Autonomy::Low => "low",
        }
    }
}

/// Defaults for every run.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Defaults {
    pub autonomy: Autonomy,
    pub limit_usd: f64,
    pub limit_min: u64,
    pub step_min: u64,
    pub repair_cap: u32,
    pub max_parallel: u32,
    pub memory_budget_mb: u64,
    /// 007: how long the lead may take to answer an agent's question before it is relayed to
    /// the engineer (research S-10).
    pub lead_answer_sec: u64,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            autonomy: Autonomy::Medium,
            limit_usd: 10.0,
            limit_min: 120,
            step_min: 30,
            repair_cap: 2,
            max_parallel: 4,
            memory_budget_mb: 0,
            lead_answer_sec: 180,
        }
    }
}

/// The agent and model of a role (FR-037). Empty strings mean "not chosen here".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RoleChoice {
    pub agent: String,
    pub model: String,
}

impl RoleChoice {
    /// `other` where it chooses something, else `self`.
    pub fn overlaid(&self, other: &RoleChoice) -> RoleChoice {
        RoleChoice {
            agent: if other.agent.is_empty() {
                self.agent.clone()
            } else {
                other.agent.clone()
            },
            model: if other.model.is_empty() {
                self.model.clone()
            } else {
                other.model.clone()
            },
        }
    }
}

/// How to call an agent: a command and fixed leading arguments, speaking the protocol of
/// `adapter` (only `claude` so far, research R-3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDef {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "claude")]
    pub adapter: String,
}

fn claude() -> String {
    "claude".into()
}

/// The engineer's own tool for a hosting service (010 research R-2): `[hosting.github]`. Fallow
/// runs it to read a branch's protection and to open and follow pull requests; never with a
/// credential of its own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTool {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
struct Raw {
    defaults: Defaults,
    roles: BTreeMap<String, RoleChoice>,
    agents: BTreeMap<String, AgentDef>,
    hosting: BTreeMap<String, HostTool>,
}

/// The settings in force.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub defaults: Defaults,
    /// planner, builder, reviewer.
    pub roles: BTreeMap<String, RoleChoice>,
    pub agents: BTreeMap<String, AgentDef>,
    /// 010: the hosting services' tools named in the settings; the built-in one otherwise
    /// (`host_tool`).
    pub hosting: BTreeMap<String, HostTool>,
}

pub const ROLES: [&str; 3] = ["planner", "builder", "reviewer"];

impl Default for Settings {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        agents.insert(
            "claude".to_string(),
            AgentDef {
                command: "claude".into(),
                args: Vec::new(),
                adapter: "claude".into(),
            },
        );
        let roles = ROLES
            .iter()
            .map(|r| {
                (
                    r.to_string(),
                    RoleChoice {
                        agent: "claude".into(),
                        model: String::new(),
                    },
                )
            })
            .collect();
        Settings {
            defaults: Defaults::default(),
            roles,
            agents,
            hosting: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// The program and leading arguments of a hosting service's tool: the settings' own, or the
    /// service's usual tool on the PATH (`gh` for GitHub).
    pub fn host_tool(&self, service: &str) -> Option<HostTool> {
        if let Some(t) = self.hosting.get(service) {
            return Some(t.clone());
        }
        (service == "github").then(|| HostTool {
            command: "gh".into(),
            args: Vec::new(),
        })
    }

    /// Parses a settings file over the built-in settings.
    pub fn parse(file: &str, text: &str, notices: &mut Vec<Notice>) -> Settings {
        let mut s = Settings::default();
        let Some(raw) = super::merge::parse::<Raw>(
            file,
            text,
            &["defaults", "roles", "agents", "hosting"],
            &[],
            notices,
        ) else {
            return s;
        };
        s.defaults = raw.defaults;
        for (name, choice) in raw.roles {
            if !ROLES.contains(&name.as_str()) {
                notices.push(Notice {
                    file: file.into(),
                    line: None,
                    level: Level::Warning,
                    message: format!("roles.{name}: roles are planner, builder and reviewer"),
                });
                continue;
            }
            let base = s.roles.get(&name).cloned().unwrap_or_default();
            s.roles.insert(name, base.overlaid(&choice));
        }
        for (name, def) in raw.agents {
            if def.adapter != "claude" {
                notices.push(Notice {
                    file: file.into(),
                    line: None,
                    level: Level::Error,
                    message: format!(
                        "agents.{name}: adapter {:?}: only \"claude\" is supported so far",
                        def.adapter
                    ),
                });
                continue;
            }
            s.agents.insert(name, def);
        }
        for (name, tool) in raw.hosting {
            if name != "github" {
                notices.push(Notice {
                    file: file.into(),
                    line: None,
                    level: Level::Warning,
                    message: format!("hosting.{name}: only \"github\" is supported so far"),
                });
                continue;
            }
            s.hosting.insert(name, tool);
        }
        let d = &s.defaults;
        let bad = [
            (d.limit_usd <= 0.0, "defaults.limit_usd must be above 0"),
            (d.limit_min == 0, "defaults.limit_min must be at least 1"),
            (d.step_min == 0, "defaults.step_min must be at least 1"),
            (
                d.max_parallel == 0,
                "defaults.max_parallel must be at least 1",
            ),
        ];
        let mut fix = false;
        for (is_bad, msg) in bad {
            if is_bad {
                fix = true;
                notices.push(Notice {
                    file: file.into(),
                    line: None,
                    level: Level::Error,
                    message: format!("{msg}; the built-in defaults are used"),
                });
            }
        }
        if fix {
            s.defaults = Defaults::default();
        }
        s
    }

    /// Reads the settings file if there is one.
    pub fn load(path: Option<&Path>) -> (Settings, Vec<Notice>) {
        let mut notices = Vec::new();
        let Some(p) = path else {
            return (Settings::default(), notices);
        };
        match std::fs::read_to_string(p) {
            Ok(text) => {
                let s = Settings::parse(&p.display().to_string(), &text, &mut notices);
                (s, notices)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), notices),
            Err(e) => {
                notices.push(Notice {
                    file: p.display().to_string(),
                    line: None,
                    level: Level::Error,
                    message: e.to_string(),
                });
                (Settings::default(), notices)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_hosting_tool_is_named_in_the_settings_or_is_gh() {
        let mut n = Vec::new();
        let s = Settings::parse("s.toml", "", &mut n);
        assert_eq!(s.host_tool("github").unwrap().command, "gh");
        assert!(s.host_tool("gitlab").is_none());
        let s = Settings::parse(
            "s.toml",
            "[hosting.github]
command = \"fake\"
args = [\"--state\", \"x\"]
[hosting.gitlab]
command = \"glab\"
",
            &mut n,
        );
        let t = s.host_tool("github").unwrap();
        assert_eq!((t.command.as_str(), t.args.len()), ("fake", 2));
        assert_eq!(n.len(), 1, "{n:?}");
        assert!(n[0].message.contains("hosting.gitlab"));
    }

    use super::*;

    #[test]
    fn defaults_match_the_contract() {
        let s = Settings::default();
        assert_eq!(s.defaults.autonomy, Autonomy::Medium);
        assert_eq!(s.defaults.limit_usd, 10.0);
        assert_eq!(s.defaults.limit_min, 120);
        assert_eq!(s.defaults.repair_cap, 2);
        assert_eq!(s.roles["reviewer"].agent, "claude");
        assert!(s.roles["reviewer"].model.is_empty());
    }

    #[test]
    fn a_file_overrides_and_warns() {
        let mut n = Vec::new();
        let s = Settings::parse(
            "settings.toml",
            "[defaults]\nautonomy = \"high\"\nlimit_usd = 5\n[roles.reviewer]\nmodel = \"opus\"\n[roles.boss]\nagent = \"x\"\n[agents.fake]\ncommand = \"fake.exe\"\nargs = [\"--scenario\", \"s.toml\"]\n",
            &mut n,
        );
        assert_eq!(s.defaults.autonomy, Autonomy::High);
        assert_eq!(s.defaults.limit_usd, 5.0);
        assert_eq!(s.defaults.limit_min, 120, "other defaults kept");
        assert_eq!(s.roles["reviewer"].model, "opus");
        assert_eq!(s.roles["reviewer"].agent, "claude");
        assert_eq!(s.agents["fake"].args.len(), 2);
        assert!(n.iter().any(|x| x.message.contains("roles.boss")));
    }
}
