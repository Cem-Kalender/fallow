//! What an agent may do (007 research S-7, S-11; constitution VI): rights by kind (maker,
//! checker, lead) and, for the lead's chats, by chat mode. A runtime turns these into its own
//! enforcement; Fallow never asks an agent to restrict itself.

use crate::run::events::GateSpec;

/// The files the lead writes: spec documents only (constitution VI, FR-007a).
pub const SPEC_GLOBS: &[&str] = &["specs/**", ".specify/memory/**", ".fallow/**"];
/// The folders of `SPEC_GLOBS`, which Fallow commits after a lead's turn.
pub const SPEC_DIRS: &[&str] = &["specs", ".specify/memory", ".fallow"];

/// 006's planner wrote these; kept while 006's planner path remains (removed with T044).
pub const PLAN_GLOBS: &[&str] = &[".fallow/**", "contracts/**", "specs/**", "docs/**"];
pub const PLAN_DIRS: &[&str] = &[".fallow", "contracts", "specs", "docs"];

/// Which files an agent may change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edits {
    None,
    /// Anywhere in its working copy, except `Rights::protect`.
    Anywhere,
    /// Only these globs, relative to its working copy.
    Only(Vec<String>),
}

/// Rights as data (contracts/runtime.md). Reading files is always allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rights {
    pub edits: Edits,
    /// Files that may never be changed, whatever `edits` says (the locked documents).
    pub protect: Vec<String>,
    /// Command prefixes allowed with any arguments after them (`python -m unittest`).
    pub commands: Vec<String>,
    /// Any command at all ("Run anything", FR-007a).
    pub any_command: bool,
    /// Not even the read-only commands a runtime allows by itself ("Read only", research S-7).
    pub no_commands: bool,
    /// The tool that answers permission prompts ("Ask", research S-7).
    pub prompt_tool: Option<String>,
}

/// The chat modes of the lead's conversations (FR-007a), in Shift+Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    ReadOnly,
    Approved,
    Ask,
    RunAnything,
}

impl Mode {
    /// The mode Shift+Tab moves to.
    pub fn next(self) -> Mode {
        match self {
            Mode::Ask => Mode::RunAnything,
            Mode::RunAnything => Mode::ReadOnly,
            Mode::ReadOnly => Mode::Approved,
            Mode::Approved => Mode::Ask,
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "read_only" => Mode::ReadOnly,
            "approved" => Mode::Approved,
            "ask" => Mode::Ask,
            "run_anything" => Mode::RunAnything,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::ReadOnly => "Read only",
            Mode::Approved => "Approved commands",
            Mode::Ask => "Ask",
            Mode::RunAnything => "Run anything",
        }
    }
}

/// The MCP tool Fallow serves for permission prompts.
pub const APPROVE_TOOL: &str = "mcp__fallow__approve";

fn push_unique(v: &mut Vec<String>, items: impl IntoIterator<Item = String>) {
    for i in items {
        let i = i.trim().to_string();
        if !i.is_empty() && !v.contains(&i) {
            v.push(i);
        }
    }
}

/// The commands an agent may run: its definition's `may_run`, with `gates` and `plan.commands`
/// expanded (contracts/agent-definition.md).
pub fn expand_may_run(may_run: &[String], gates: &[GateSpec], plan_commands: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for m in may_run {
        match m.as_str() {
            "gates" => push_unique(&mut out, gates.iter().map(|g| g.command.clone())),
            "plan.commands" => push_unique(&mut out, plan_commands.iter().cloned()),
            c => push_unique(&mut out, [c.to_string()]),
        }
    }
    out
}

/// A maker: edits its working copy, never the protected files, and runs the listed commands.
pub fn maker(commands: Vec<String>, protect: Vec<String>) -> Rights {
    Rights {
        edits: Edits::Anywhere,
        protect,
        commands,
        any_command: false,
        no_commands: false,
        prompt_tool: None,
    }
}

/// A checker: runs the listed commands and changes nothing.
pub fn checker(commands: Vec<String>) -> Rights {
    Rights {
        edits: Edits::None,
        protect: Vec::new(),
        commands,
        any_command: false,
        no_commands: false,
        prompt_tool: None,
    }
}

/// The lead in a chat: writes only spec documents; what it runs follows the mode. `approved` is
/// the project's approved commands with its gates.
pub fn lead(mode: Mode, approved: Vec<String>) -> Rights {
    let mut r = Rights {
        edits: Edits::Only(SPEC_GLOBS.iter().map(|s| s.to_string()).collect()),
        protect: Vec::new(),
        commands: Vec::new(),
        any_command: false,
        no_commands: false,
        prompt_tool: None,
    };
    match mode {
        Mode::ReadOnly => r.no_commands = true,
        Mode::Approved => r.commands = approved,
        Mode::Ask => {
            r.commands = approved;
            r.prompt_tool = Some(APPROVE_TOOL.into());
        }
        Mode::RunAnything => {
            r.commands = approved;
            r.any_command = true;
        }
    }
    r
}

/// The lead answering an agent's question during a loop: approved commands only, whatever its
/// chat's mode (FR-007a).
pub fn lead_in_loop(approved: Vec<String>) -> Rights {
    lead(Mode::Approved, approved)
}

/// The prefix of a 006 rule (`Bash(npm run lint:*)` gives `npm run lint`), for the squad file's
/// `[allow]` lists, which hold rules.
pub fn prefix_of_rule(rule: &str) -> Option<String> {
    let inner = rule
        .strip_prefix("Bash(")
        .or_else(|| rule.strip_prefix("PowerShell("))?
        .strip_suffix(')')?;
    Some(inner.strip_suffix(":*").unwrap_or(inner).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates() -> Vec<GateSpec> {
        vec![GateSpec {
            name: "tests".into(),
            command: "python -m unittest".into(),
            timeout_min: 30,
            baseline: true,
            failures: "auto".into(),
        }]
    }

    #[test]
    fn may_run_expands_gates_and_plan_commands() {
        let c = expand_may_run(
            &["gates".into(), "plan.commands".into(), "npm audit".into()],
            &gates(),
            &["python -m shop".into(), "python -m unittest".into()],
        );
        assert_eq!(c, ["python -m unittest", "python -m shop", "npm audit"]);
    }

    #[test]
    fn kinds_get_their_rights() {
        let m = maker(vec!["python -m unittest".into()], vec!["specs/007-x/spec.md".into()]);
        assert_eq!(m.edits, Edits::Anywhere);
        assert_eq!(m.protect, ["specs/007-x/spec.md"]);
        let c = checker(vec!["python -m unittest".into()]);
        assert_eq!(c.edits, Edits::None);
        assert!(!c.any_command && c.prompt_tool.is_none());
    }

    #[test]
    fn chat_modes_give_their_rights() {
        let approved = vec!["python -m shop".to_string()];
        let r = lead(Mode::ReadOnly, approved.clone());
        assert!(r.no_commands && r.commands.is_empty());
        let r = lead(Mode::Approved, approved.clone());
        assert_eq!(r.commands, approved);
        assert!(r.prompt_tool.is_none() && !r.any_command);
        let r = lead(Mode::Ask, approved.clone());
        assert_eq!(r.prompt_tool.as_deref(), Some(APPROVE_TOOL));
        let r = lead(Mode::RunAnything, approved.clone());
        assert!(r.any_command);
        for m in [Mode::ReadOnly, Mode::Approved, Mode::Ask, Mode::RunAnything] {
            let r = lead(m, approved.clone());
            assert_eq!(
                r.edits,
                Edits::Only(SPEC_GLOBS.iter().map(|s| s.to_string()).collect()),
                "{m:?}: the lead writes only spec documents"
            );
        }
        assert_eq!(lead_in_loop(approved.clone()), lead(Mode::Approved, approved));
    }

    #[test]
    fn shift_tab_cycles_from_ask() {
        let mut m = Mode::Ask;
        let mut seen = vec![m];
        for _ in 0..4 {
            m = m.next();
            seen.push(m);
        }
        assert_eq!(
            seen,
            [Mode::Ask, Mode::RunAnything, Mode::ReadOnly, Mode::Approved, Mode::Ask]
        );
    }

    #[test]
    fn rule_prefixes() {
        assert_eq!(prefix_of_rule("Bash(npm run lint:*)").as_deref(), Some("npm run lint"));
        assert_eq!(prefix_of_rule("PowerShell(make:*)").as_deref(), Some("make"));
        assert_eq!(prefix_of_rule("Edit"), None);
    }
}
