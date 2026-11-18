//! What an agent may do (007 research S-7, S-11; constitution VI): rights by kind (maker,
//! checker, lead) and, for the lead's chats, by chat mode. A runtime turns these into its own
//! enforcement; Fallow never asks an agent to restrict itself.

use crate::run::events::GateSpec;

/// The files the lead writes: spec documents, and drafts the engineer may take up (constitution
/// VI, FR-007a). Never `.fallow/agents`, `.fallow/pipelines` or `.fallow/squad.toml`: those decide
/// what agents may run, and only the engineer saves them (FR-026, FR-028; Phase 2 review).
pub const SPEC_GLOBS: &[&str] = &["specs/**", ".specify/memory/**", ".fallow/drafts/**"];
/// The folders of `SPEC_GLOBS`, which Fallow commits after a lead's turn.
pub const SPEC_DIRS: &[&str] = &["specs", ".specify/memory", ".fallow/drafts"];
/// What controls agents, denied to the lead whatever else it may write.
pub const CONTROL_GLOBS: &[&str] = &[
    ".fallow/agents/**",
    ".fallow/pipelines/**",
    ".fallow/squad.toml",
];

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
    /// The tool that answers permission prompts ("Ask", research S-7).
    pub prompt_tool: Option<String>,
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
pub fn expand_may_run(
    may_run: &[String],
    gates: &[GateSpec],
    plan_commands: &[String],
) -> Vec<String> {
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
        prompt_tool: None,
    }
}

/// A checker: runs the listed commands and changes nothing.
pub fn checker(commands: Vec<String>) -> Rights {
    Rights {
        edits: Edits::None,
        protect: Vec::new(),
        commands,
        prompt_tool: None,
    }
}

/// The lead in a chat: writes only spec documents; what it runs follows the mode. `approved` is
/// the project's approved commands with its gates.
pub fn lead(approved: Vec<String>) -> Rights {
    // 015 (research R-1): in either chat mode every command outside the approved ones, and
    // every edit outside the spec documents, comes to Fallow's permission prompt, which
    // applies the mode; no rule lets the runtime allow everything.
    Rights {
        edits: Edits::Only(SPEC_GLOBS.iter().map(|s| s.to_string()).collect()),
        protect: CONTROL_GLOBS.iter().map(|s| s.to_string()).collect(),
        commands: approved,
        prompt_tool: Some(APPROVE_TOOL.into()),
    }
}

/// 012 (FR-008, research R-6): the assistant of a solo session. It edits its working copy,
/// never `protect` (the control files); `commands` (the gates and the approved prefixes the
/// judge finds safe) run without asking; everything else goes to Fallow's permission prompt,
/// where the judge and the engineer decide.
pub fn assistant(commands: Vec<String>, protect: Vec<String>) -> Rights {
    Rights {
        edits: Edits::Anywhere,
        protect,
        commands,
        prompt_tool: Some(APPROVE_TOOL.into()),
    }
}

/// The lead answering an agent's question during a loop: approved commands only, whatever its
/// chat's mode (FR-007a), and nothing asked: no one is in the chat.
pub fn lead_in_loop(approved: Vec<String>) -> Rights {
    Rights {
        prompt_tool: None,
        ..lead(approved)
    }
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
        let m = maker(
            vec!["python -m unittest".into()],
            vec!["specs/007-x/spec.md".into()],
        );
        assert_eq!(m.edits, Edits::Anywhere);
        assert_eq!(m.protect, ["specs/007-x/spec.md"]);
        let c = checker(vec!["python -m unittest".into()]);
        assert_eq!(c.edits, Edits::None);
        assert!(c.prompt_tool.is_none());
    }

    #[test]
    fn the_lead_always_asks_fallow_and_never_runs_everything() {
        let approved = vec!["python -m shop".to_string()];
        let r = lead(approved.clone());
        assert_eq!(r.prompt_tool.as_deref(), Some(APPROVE_TOOL));
        assert_eq!(r.commands, approved);
        assert_eq!(
            r.edits,
            Edits::Only(SPEC_GLOBS.iter().map(|s| s.to_string()).collect()),
            "the lead writes only spec documents"
        );
        let l = lead_in_loop(approved.clone());
        assert_eq!(l.commands, approved);
        assert!(l.prompt_tool.is_none());
    }

    #[test]
    fn rule_prefixes() {
        assert_eq!(
            prefix_of_rule("Bash(npm run lint:*)").as_deref(),
            Some("npm run lint")
        );
        assert_eq!(
            prefix_of_rule("PowerShell(make:*)").as_deref(),
            Some("make")
        );
        assert_eq!(prefix_of_rule("Edit"), None);
    }
}
