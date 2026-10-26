//! Roles: permission levels mapped to each agent CLI's own flags, and the tools and events a role
//! may use (004 research R-2, spec FR-008 to FR-015).
//!
//! Repository permissions are enforced by the agent CLI from these flags, not by Fallow. Fallow
//! enforces what it controls (its tools and events) and checks outcomes it can see (FR-013).

use super::squad::{Level, Role, Squad};
use crate::session::profiles::{AgentProfile, Style};

/// Flags that make an agent skip its own permission checks (FR-014).
const BYPASS_FLAGS: &[&str] = &[
    "--dangerously-skip-permissions",
    "--dangerously-bypass-approvals-and-sandbox",
    "--yolo",
];
const BYPASS_VALUES: &[(&str, &str)] = &[
    ("--permission-mode", "bypassPermissions"),
    ("--approval-mode", "yolo"),
];

fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

/// The agent's flags for a level (research R-2), or `None` when Fallow knows no safe mapping.
pub fn level_flags(style: Style, level: Level) -> Option<Vec<String>> {
    Some(match (style, level) {
        (Style::Claude, Level::ReadOnly) => v(&[
            "--permission-mode",
            "dontAsk",
            "--disallowedTools",
            "Edit",
            "Write",
            "NotebookEdit",
        ]),
        (Style::Claude, Level::Edit) => v(&["--permission-mode", "acceptEdits"]),
        // Claude Code runs commands with its Bash tool, or its PowerShell tool on Windows.
        (Style::Claude, Level::EditRun) => v(&[
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Bash",
            "PowerShell",
        ]),
        (Style::Codex, Level::ReadOnly) => {
            v(&["--sandbox", "read-only", "--ask-for-approval", "never"])
        }
        (Style::Codex, Level::Edit) => v(&[
            "--sandbox",
            "workspace-write",
            "--ask-for-approval",
            "on-request",
        ]),
        (Style::Codex, Level::EditRun) => v(&[
            "--sandbox",
            "workspace-write",
            "--ask-for-approval",
            "never",
        ]),
        (Style::Gemini, Level::ReadOnly) => v(&["--approval-mode", "plan"]),
        (Style::Gemini, Level::Edit) => v(&["--approval-mode", "auto_edit"]),
        (Style::Gemini, Level::EditRun) | (Style::None, _) => return None,
    })
}

/// Whether a command line makes the agent skip its own permission checks.
pub fn bypasses(args: &[String]) -> bool {
    args.iter().any(|a| BYPASS_FLAGS.contains(&a.as_str()))
        || args.windows(2).any(|w| {
            BYPASS_VALUES
                .iter()
                .any(|(flag, value)| w[0] == *flag && w[1] == *value)
        })
        || args.iter().any(|a| {
            BYPASS_VALUES
                .iter()
                .any(|(flag, value)| *a == format!("{flag}={value}"))
        })
}

/// The extra arguments a session in `role` starts with on `profile`: the level's flags (or the
/// squad's own for this agent) and the model. `Err` when there are none: an agent cannot take a
/// role Fallow cannot express for it (FR-008).
pub fn role_args(role: &Role, profile: &AgentProfile) -> Result<Vec<String>, String> {
    let mut args = match role.flags.get(&profile.id) {
        Some(f) => f.clone(),
        None => level_flags(profile.orchestration, role.level).ok_or_else(|| {
            format!(
                "{} has no flags for the {} level; give them in the squad as [role.flags] {} = [...]",
                profile.name,
                role.level.as_str(),
                profile.id
            )
        })?,
    };
    if let Some(m) = &role.model {
        args.push("--model".into());
        args.push(m.clone());
    }
    Ok(args)
}

/// Options that take several values (Claude Code): merged into one occurrence each, at the end,
/// so that nothing positional follows them (003 research R-5).
const LIST_FLAGS: &[&str] = &["--allowedTools", "--disallowedTools"];

/// Merges repeated list options in a command line into one occurrence each.
pub fn merge_list_flags(args: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(flag) = LIST_FLAGS.iter().find(|f| args[i] == **f) {
            let entry = match lists.iter_mut().find(|(f, _)| f == flag) {
                Some(e) => e,
                None => {
                    lists.push(((*flag).to_string(), Vec::new()));
                    lists.last_mut().unwrap()
                }
            };
            i += 1;
            while i < args.len() && !args[i].starts_with('-') {
                if !entry.1.contains(&args[i]) {
                    entry.1.push(args[i].clone());
                }
                i += 1;
            }
        } else {
            out.push(args[i].clone());
            i += 1;
        }
    }
    for (flag, values) in lists {
        out.push(flag);
        out.extend(values);
    }
    out
}

/// `*` matches any run of characters.
pub fn pattern_match(pattern: &str, s: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == s;
    }
    let mut rest = s;
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(p) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(p);
        } else {
            match rest.find(p) {
                Some(at) => rest = &rest[at + p.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Events only Fallow signals (gates, FR-020).
pub fn fallow_only(event: &str) -> bool {
    event.starts_with("gate-")
}

/// Whether `role` may signal `event` under `squad` (FR-012). A specific pattern (not just `*`)
/// claims an event for its roles; `*` covers only events no role claims specifically, so a lead
/// with `*` cannot give a reviewer's verdict.
pub fn may_signal(squad: &Squad, role: &Role, event: &str) -> bool {
    if fallow_only(event) {
        return false;
    }
    let specific = |r: &Role| {
        r.signals
            .iter()
            .any(|p| p != "*" && pattern_match(p, event))
    };
    if squad.roles.iter().any(specific) {
        return specific(role);
    }
    role.signals.iter().any(|p| p == "*")
        || !squad
            .roles
            .iter()
            .any(|r| r.signals.iter().any(|p| pattern_match(p, event)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::squad::{self, Level};
    use crate::session::profiles::builtins;

    fn profile(id: &str) -> AgentProfile {
        builtins().into_iter().find(|p| p.id == id).unwrap()
    }

    fn role(level: Level) -> Role {
        Role {
            level,
            ..squad::builtin().role("builder").unwrap().clone()
        }
    }

    fn args(r: &Role, p: &str) -> Vec<String> {
        role_args(r, &profile(p)).unwrap()
    }

    #[test]
    fn exact_flags_per_agent_and_level() {
        // research R-2.
        assert_eq!(
            args(&role(Level::ReadOnly), "claude"),
            [
                "--permission-mode",
                "dontAsk",
                "--disallowedTools",
                "Edit",
                "Write",
                "NotebookEdit"
            ]
        );
        assert_eq!(
            args(&role(Level::Edit), "claude"),
            ["--permission-mode", "acceptEdits"]
        );
        assert_eq!(
            args(&role(Level::EditRun), "claude"),
            [
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash",
                "PowerShell"
            ]
        );
        assert_eq!(
            args(&role(Level::ReadOnly), "codex"),
            ["--sandbox", "read-only", "--ask-for-approval", "never"]
        );
        assert_eq!(
            args(&role(Level::Edit), "codex"),
            [
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
        assert_eq!(
            args(&role(Level::EditRun), "codex"),
            [
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never"
            ]
        );
        assert_eq!(
            args(&role(Level::ReadOnly), "gemini"),
            ["--approval-mode", "plan"]
        );
        assert_eq!(
            args(&role(Level::Edit), "gemini"),
            ["--approval-mode", "auto_edit"]
        );
    }

    #[test]
    fn no_mapping_means_no_role_unless_the_squad_gives_flags() {
        let e = role_args(&role(Level::EditRun), &profile("gemini")).unwrap_err();
        assert!(e.contains("[role.flags] gemini"), "{e}");
        assert!(role_args(&role(Level::ReadOnly), &profile("shell")).is_err());
        let mut r = role(Level::EditRun);
        r.flags.insert(
            "gemini".into(),
            vec!["--approval-mode".into(), "yolo".into()],
        );
        r.model = Some("pro".into());
        assert_eq!(
            args(&r, "gemini"),
            ["--approval-mode", "yolo", "--model", "pro"]
        );
    }

    #[test]
    fn bypass_flags_are_recognized() {
        let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(bypasses(&s(&["--dangerously-skip-permissions"])));
        assert!(bypasses(&s(&["--permission-mode", "bypassPermissions"])));
        assert!(bypasses(&s(&["--permission-mode=bypassPermissions"])));
        assert!(bypasses(&s(&[
            "--dangerously-bypass-approvals-and-sandbox"
        ])));
        assert!(bypasses(&s(&["--yolo"])));
        assert!(bypasses(&s(&["--approval-mode", "yolo"])));
        assert!(!bypasses(&s(&["--permission-mode", "acceptEdits"])));
        assert!(!bypasses(&args(&role(Level::EditRun), "codex")));
    }

    #[test]
    fn list_options_are_merged_once_at_the_end() {
        let a: Vec<String> = [
            "--settings",
            "{}",
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Bash",
            "--disallowedTools",
            "Edit",
            "Write",
            "--mcp-config",
            "{}",
            "--allowedTools",
            "mcp__fallow__*",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            merge_list_flags(a),
            [
                "--settings",
                "{}",
                "--permission-mode",
                "acceptEdits",
                "--mcp-config",
                "{}",
                "--allowedTools",
                "Bash",
                "mcp__fallow__*",
                "--disallowedTools",
                "Edit",
                "Write"
            ]
        );
    }

    #[test]
    fn who_may_signal_what() {
        let s = squad::builtin();
        let lead = s.role("lead").unwrap();
        let builder = s.role("builder").unwrap();
        let reviewer = s.role("reviewer").unwrap();
        assert!(may_signal(&s, reviewer, "review-approved"));
        assert!(!may_signal(&s, lead, "review-approved"), "no self-approval");
        assert!(!may_signal(&s, builder, "review-approved"));
        assert!(may_signal(&s, builder, "api-ready"));
        assert!(!may_signal(&s, reviewer, "api-ready"));
        assert!(
            may_signal(&s, lead, "kickoff"),
            "unclaimed events: the lead's *"
        );
        assert!(!may_signal(&s, builder, "kickoff"));
        for r in [lead, builder, reviewer] {
            assert!(!may_signal(&s, r, "gate-tests-passed"), "Fallow only");
        }
        assert!(pattern_match("a*b*c", "aXXbYc"));
        assert!(!pattern_match("a*c", "abd"));
    }
}
