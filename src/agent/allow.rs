//! The rights of each role, as Claude Code permission rules under `dontAsk` (006 research R-4,
//! FR-036, constitution VI). Anything not listed is refused without a prompt; Claude Code also
//! allows commands it classifies as read-only (verified T001).

use crate::run::events::GateSpec;

const READ: &[&str] = &["Read", "Grep", "Glob"];
const READ_SHELL: &[&str] = &[
    "Bash(ls:*)",
    "Bash(dir:*)",
    "Bash(cat:*)",
    "Bash(type:*)",
    "Bash(head:*)",
    "Bash(tail:*)",
    "Bash(findstr:*)",
    "Bash(grep:*)",
    "Bash(git status:*)",
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(echo:*)",
    "Bash(pwd:*)",
    "Bash(wc:*)",
    "Bash(sort:*)",
    "Bash(which:*)",
    "Bash(where:*)",
];
const GIT_WRITE: &[&str] = &["Bash(git add:*)", "Bash(git commit:*)", "Bash(git merge:*)"];
/// Moving around inside the worktree, which agents often put in front of a command.
const MOVE: &[&str] = &["Bash(cd:*)"];
/// Files the planner may write.
pub const PLAN_GLOBS: &[&str] = &[".fallow/**", "contracts/**", "specs/**", "docs/**"];
/// The folders of `PLAN_GLOBS`, which Fallow commits after a planning turn.
pub const PLAN_DIRS: &[&str] = &[".fallow", "contracts", "specs", "docs"];

/// Each gate command as a rule, and the program it runs (`python -m unittest` → `python`), so
/// builders and reviewers can run their own checks with the same tool.
fn gate_rules(gates: &[GateSpec]) -> Vec<String> {
    let mut out = Vec::new();
    for g in gates {
        let cmd = g.command.trim();
        out.push(format!("Bash({cmd}:*)"));
        if let Some(program) = cmd.split_whitespace().next() {
            let r = format!("Bash({program}:*)");
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

fn base(extra: &[String]) -> Vec<String> {
    let mut v: Vec<String> = READ
        .iter()
        .chain(READ_SHELL)
        .chain(MOVE)
        .map(|s| s.to_string())
        .collect();
    v.push("mcp__fallow__*".into());
    v.extend(extra.iter().cloned());
    v
}

/// Every `Bash(...)` rule again for Claude Code's PowerShell tool on Windows (verified on a real
/// run: a command refused as Bash is tried as PowerShell).
fn with_powershell(mut v: Vec<String>) -> Vec<String> {
    let ps: Vec<String> = v
        .iter()
        .filter_map(|r| {
            r.strip_prefix("Bash(")
                .map(|rest| format!("PowerShell({rest}"))
        })
        .collect();
    for r in ps {
        if !v.contains(&r) {
            v.push(r);
        }
    }
    v
}

/// The builder: read, edit and write, git within its worktree, the gate commands, plus the
/// squad file's `[allow] builder`.
pub fn builder(gates: &[GateSpec], extra: &[String]) -> Vec<String> {
    let mut v = base(extra);
    v.extend(["Edit", "Write"].map(String::from));
    v.extend(GIT_WRITE.iter().map(|s| s.to_string()));
    v.extend(gate_rules(gates));
    v.dedup();
    with_powershell(v)
}

/// The reviewer: the builder's list without editing, writing or committing.
pub fn reviewer(gates: &[GateSpec], extra: &[String]) -> Vec<String> {
    let mut v = base(extra);
    v.extend(gate_rules(gates));
    v.dedup();
    with_powershell(v)
}

/// The planner: read everything, write only the plan's files, and commit them.
pub fn planner(extra: &[String]) -> Vec<String> {
    let mut v = base(extra);
    for g in PLAN_GLOBS {
        v.push(format!("Edit({g})"));
        v.push(format!("Write({g})"));
    }
    v.extend(["Bash(git add:*)", "Bash(git commit:*)"].map(String::from));
    with_powershell(v)
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
    fn roles_get_what_r4_lists() {
        let b = builder(&gates(), &["Bash(npm run lint:*)".into()]);
        for r in [
            "Edit",
            "Write",
            "Bash(git commit:*)",
            "Bash(python -m unittest:*)",
            "Bash(python:*)",
            "Bash(npm run lint:*)",
            "mcp__fallow__*",
        ] {
            assert!(b.iter().any(|x| x == r), "builder lacks {r}");
        }
        let r = reviewer(&gates(), &[]);
        for no in [
            "Edit",
            "Write",
            "Bash(git commit:*)",
            "Bash(git add:*)",
            "Bash(git merge:*)",
        ] {
            assert!(!r.iter().any(|x| x == no), "reviewer has {no}");
        }
        assert!(r.iter().any(|x| x == "Bash(python -m unittest:*)"));
        let p = planner(&[]);
        assert!(p.iter().any(|x| x == "Edit(.fallow/**)"));
        assert!(!p.iter().any(|x| x == "Edit"));
        assert!(!b.iter().any(|x| x.contains("dangerously")));
        assert!(b.iter().any(|x| x == "PowerShell(python -m unittest:*)"));
        assert!(!r.iter().any(|x| x == "PowerShell(git commit:*)"));
    }
}
