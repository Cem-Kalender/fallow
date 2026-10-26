//! Evidence carried by task reports during a burst (004 US5, FR-023, FR-024).
//!
//! A done report names the commit, the commands run with their results, and the files changed.
//! Fallow checks what it can: that the commit exists and is on the reporting session's branch,
//! and how the claimed files compare with git's. The commands are the agent's claims; the next
//! role receives all of it with its task.

use std::path::Path;

use serde_json::{Value, json};

use super::git;

pub const COMMANDS_MAX: usize = 20;
pub const COMMAND_LEN_MAX: usize = 500;
pub const FILES_MAX: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub commit: String,
    /// (command, result), as the agent reported them.
    pub commands: Vec<(String, String)>,
    pub files: Vec<String>,
    /// What Fallow checked itself.
    pub verified: Vec<String>,
}

impl Evidence {
    pub fn json(&self) -> Value {
        json!({
            "commit": self.commit,
            "commands": self
                .commands
                .iter()
                .map(|(c, r)| json!({ "command": c, "result": r }))
                .collect::<Vec<_>>(),
            "files": self.files,
            "verified": self.verified,
        })
    }

    /// The evidence as the next role reads it with its task.
    pub fn summary(&self) -> String {
        let mut s = format!("commit {}", short(&self.commit));
        if !self.commands.is_empty() {
            let c: Vec<String> = self
                .commands
                .iter()
                .map(|(c, r)| format!("`{c}` → {r}"))
                .collect();
            s.push_str(&format!("; commands: {}", c.join("; ")));
        }
        if !self.files.is_empty() {
            let shown: Vec<&str> = self.files.iter().take(20).map(String::as_str).collect();
            let more = self.files.len().saturating_sub(shown.len());
            s.push_str(&format!("; files: {}", shown.join(", ")));
            if more > 0 {
                s.push_str(&format!(" and {more} more"));
            }
        }
        if !self.verified.is_empty() {
            s.push_str(&format!(" (Fallow checked: {})", self.verified.join("; ")));
        }
        s
    }
}

fn short(c: &str) -> &str {
    &c[..c.len().min(10)]
}

/// Reads the `evidence` argument of `task_report` (limits from data-model "Evidence").
pub fn parse(v: &Value) -> Result<Evidence, String> {
    let o = v.as_object().ok_or("evidence must be an object")?;
    if let Some(k) = o
        .keys()
        .find(|k| !["commit", "commands", "files"].contains(&k.as_str()))
    {
        return Err(format!("evidence: unknown field {k:?}"));
    }
    let commit = o
        .get("commit")
        .and_then(Value::as_str)
        .ok_or("evidence.commit: the commit with your work (7 to 40 hex digits)")?
        .trim()
        .to_string();
    if !(7..=40).contains(&commit.len()) || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("evidence.commit {commit:?}: 7 to 40 hex digits"));
    }
    let mut commands = Vec::new();
    for c in o
        .get("commands")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let command = c["command"].as_str().unwrap_or_default();
        let result = c["result"].as_str().unwrap_or_default();
        if command.is_empty() {
            return Err("evidence.commands: each needs {\"command\": …, \"result\": …}".into());
        }
        if command.chars().count() > COMMAND_LEN_MAX || result.chars().count() > COMMAND_LEN_MAX {
            return Err(format!(
                "evidence.commands: at most {COMMAND_LEN_MAX} characters each"
            ));
        }
        commands.push((command.to_string(), result.to_string()));
    }
    if commands.len() > COMMANDS_MAX {
        return Err(format!("evidence.commands: at most {COMMANDS_MAX}"));
    }
    let files: Vec<String> = o
        .get("files")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(Value::as_str)
        .map(|f| f.replace('\\', "/"))
        .collect();
    if files.len() > FILES_MAX {
        return Err(format!("evidence.files: at most {FILES_MAX}"));
    }
    Ok(Evidence {
        commit,
        commands,
        files,
        verified: Vec::new(),
    })
}

/// Checks evidence in the reporting session's folder (FR-024): the commit must exist, and for a
/// role that edits it must be on the session's branch. Claimed files are compared with git's
/// list since `start`; differences are noted, not refused.
pub fn check(
    dir: &Path,
    branch: Option<&str>,
    start: Option<&str>,
    edits: bool,
    ev: &mut Evidence,
) -> Result<(), String> {
    if !git::commit_exists(dir, &ev.commit) {
        return Err(format!("commit {} does not exist here", ev.commit));
    }
    if edits {
        let at = branch.unwrap_or("HEAD");
        if !git::on_branch(dir, &ev.commit, at) {
            return Err(format!(
                "commit {} is not on your branch {at}: commit your work there and report that commit",
                short(&ev.commit)
            ));
        }
        ev.verified
            .push(format!("commit {} on {at}", short(&ev.commit)));
    } else {
        ev.verified
            .push(format!("commit {} exists", short(&ev.commit)));
    }
    if let Some(start) = start {
        if let Ok(changed) = git::files_between(dir, start, &ev.commit) {
            let claimed: std::collections::BTreeSet<&str> =
                ev.files.iter().map(String::as_str).collect();
            let actual: std::collections::BTreeSet<&str> =
                changed.iter().map(String::as_str).collect();
            if claimed == actual {
                ev.verified
                    .push(format!("{} file(s) match git", actual.len()));
            } else {
                let extra: Vec<&str> = claimed.difference(&actual).copied().collect();
                let missing: Vec<&str> = actual.difference(&claimed).copied().collect();
                let mut n = String::from("files differ from git");
                if !extra.is_empty() {
                    n.push_str(&format!(", not changed: {}", extra.join(", ")));
                }
                if !missing.is_empty() {
                    n.push_str(&format!(", changed but not listed: {}", missing.join(", ")));
                }
                ev.verified.push(n);
            }
        }
    }
    Ok(())
}
