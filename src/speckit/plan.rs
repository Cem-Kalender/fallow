//! Reading `plan.md`'s `## Commands` (007 contracts/spec-kit-files.md): the commands agents may
//! run besides the gates, approved with the lock.

use super::Problem;
use crate::project::commands::command_problem;

/// The commands under `## Commands`, one per list item in backticks. Each must pass the
/// approved-command check; problems name the line.
pub fn commands(file: &str, text: &str) -> Result<Vec<String>, Vec<Problem>> {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut inside = false;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with("## ") {
            inside = line.trim_end() == "## Commands";
            continue;
        }
        if !inside {
            continue;
        }
        let t = line.trim();
        let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
            continue;
        };
        let Some(cmd) = item
            .strip_prefix('`')
            .and_then(|r| r.split_once('`'))
            .map(|(c, _)| c.trim())
        else {
            problems.push(Problem::new(
                file,
                i + 1,
                "a command is a list item in backticks",
            ));
            continue;
        };
        match command_problem(cmd) {
            Some(why) => {
                problems.push(Problem::new(file, i + 1, format!("command {cmd:?}: {why}")))
            }
            None => {
                if !out.iter().any(|c: &String| c == cmd) {
                    out.push(cmd.to_string());
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_read_and_checked() {
        let plan = "# Plan\n\n## Commands\n\n- `python -m shop`\n- `python -m shop` (again)\n\n## Structure\n\n- `rm -rf x`\n";
        assert_eq!(commands("plan.md", plan).unwrap(), ["python -m shop"]);
        let bad = "## Commands\n- `git push`\n- python -m shop\n";
        let e = commands("plan.md", bad).unwrap_err();
        assert_eq!(e.len(), 2);
        assert!(e[0].to_string().starts_with("plan.md:2:"));
        assert!(commands("plan.md", "# no section\n").unwrap().is_empty());
    }
}
