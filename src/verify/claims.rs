//! A builder's claims, checked against the repository (006 research R-7, FR-005, FR-013).

use std::path::Path;

use serde_json::Value;

use super::git;
use crate::agent::rights::PLAN_GLOBS;
use crate::plan::{Plan, glob_set};

/// What a builder's `done` result must show. Fallow commits the step's changes itself when it
/// ends, so the work is the tip of the part's branch; a commit the builder names is a claim that
/// must exist and be on that branch. The tip must be newer than the step's base, change only
/// files the part may change, and contain every decision commit the part must take. `Ok` gives
/// the verified commit; `Err` the reasons it is refused. The files are counted from
/// `files_base`, the part's last accepted commit, when the tip descends from it.
#[allow(clippy::too_many_arguments)]
pub fn check_build(
    repo: &Path,
    branch: &str,
    base: &str,
    files_base: &str,
    result: &Value,
    plan: &Plan,
    part: &str,
    takes: &[String],
) -> Result<String, Vec<String>> {
    let mut why = Vec::new();
    if let Some(claimed) = result["commit"].as_str().filter(|c| !c.trim().is_empty()) {
        match git::rev(repo, claimed) {
            Err(_) => return Err(vec![format!("commit {claimed} does not exist")]),
            Ok(c) if !git::on_branch(repo, &c, branch) => {
                return Err(vec![format!(
                    "commit {} is not on branch {branch}",
                    short(&c)
                )]);
            }
            Ok(_) => {}
        }
    }
    let commit = match git::rev(repo, branch) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("branch {branch}: {e}")]),
    };
    if commit == base || !git::is_ancestor(repo, base, &commit) {
        why.push(format!(
            "the step changed nothing: branch {branch} is still at {}",
            short(base)
        ));
        return Err(why);
    }
    // The files are counted from the part's last accepted commit: a refused change stays
    // counted until a later step is accepted.
    let from = if git::is_ancestor(repo, files_base, &commit) {
        files_base
    } else {
        base
    };
    let files = match git::files_between(repo, from, &commit) {
        Ok(f) => f,
        Err(e) => {
            why.push(format!("cannot read the commit's changes: {e}"));
            return Err(why);
        }
    };
    if files.is_empty() {
        why.push(format!("commit {} changes no files", short(&commit)));
    }
    let outside = outside_files(plan, part, &files);
    if !outside.is_empty() {
        why.push(format!(
            "files outside what part {part} may change: {}",
            outside.join(", ")
        ));
    }
    for t in takes {
        if !git::is_ancestor(repo, t, &commit) {
            why.push(format!(
                "your work lacks commit {}: merge it into your branch and report again",
                short(t)
            ));
        }
    }
    if why.is_empty() { Ok(commit) } else { Err(why) }
}

/// Changed files a part may not change: the plan's own files always; with owned globs,
/// anything outside them and the plan's shared files.
pub fn outside_files(plan: &Plan, part: &str, files: &[String]) -> Vec<String> {
    let plan_files = plan.plan_files();
    let owns = plan.part(part).map(|p| p.owns.clone()).unwrap_or_default();
    let mut allowed = owns.clone();
    allowed.extend(plan.shared.iter().cloned());
    let allowed_set = glob_set(&allowed);
    files
        .iter()
        .filter(|f| {
            let is_plan = plan_files.iter().any(|p| p == *f) || f.starts_with(".fallow/");
            if is_plan {
                return true;
            }
            if owns.is_empty() {
                return false;
            }
            !allowed_set.as_ref().is_some_and(|s| s.is_match(f.as_str()))
        })
        .cloned()
        .collect()
}

/// The planner's commit may change only the plan's files (`.fallow/`, `contracts/`, `specs/`,
/// `docs/`).
pub fn check_plan_commit(
    repo: &Path,
    branch: &str,
    base: &str,
    commit: &str,
) -> Result<String, Vec<String>> {
    let Ok(full) = git::rev(repo, commit) else {
        return Err(vec![format!("commit {commit} does not exist")]);
    };
    let mut why = Vec::new();
    if !git::on_branch(repo, &full, branch) {
        why.push(format!("commit {} is not on branch {branch}", short(&full)));
    }
    let globs: Vec<String> = PLAN_GLOBS.iter().map(|s| s.to_string()).collect();
    let set = glob_set(&globs);
    match git::files_between(repo, base, &full) {
        Ok(files) => {
            let bad: Vec<&String> = files
                .iter()
                .filter(|f| !set.as_ref().is_some_and(|s| s.is_match(f.as_str())))
                .collect();
            if !bad.is_empty() {
                why.push(format!(
                    "the planner changed files outside the plan: {}",
                    bad.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        Err(e) => why.push(format!("cannot read the commit's changes: {e}")),
    }
    if why.is_empty() { Ok(full) } else { Err(why) }
}

pub fn short(c: &str) -> &str {
    &c[..c.len().min(10)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(text: &str) -> Plan {
        crate::plan::parse(".fallow/work.toml", text).unwrap()
    }

    #[test]
    fn outside_files_for_one_builder_and_for_parts() {
        let solo = plan("criteria = [{ id = \"a\", text = \"x\", must_fail = true }]");
        assert!(
            outside_files(&solo, "main", &["shop/x.py".into(), "tests/t.py".into()]).is_empty()
        );
        assert_eq!(
            outside_files(&solo, "main", &[".fallow/work.toml".into()]),
            [".fallow/work.toml"]
        );
        let parts = plan(
            "build = \"parts\"\nreason = \"r\"\nshared = [\"Cargo.lock\"]\n[[part]]\nid = \"a\"\nowns = [\"src/a/**\"]\ncriteria = [{ id = \"x\", text = \"x\", must_fail = true }]\n[[part]]\nid = \"b\"\nowns = [\"src/b/**\"]\ncriteria = [{ id = \"x\", text = \"x\", must_fail = true }]\n[[contract]]\npath = \"contracts/ab.md\"\nbetween = [\"a\", \"b\"]\n",
        );
        assert_eq!(
            outside_files(
                &parts,
                "a",
                &[
                    "src/a/x.rs".into(),
                    "Cargo.lock".into(),
                    "src/b/y.rs".into(),
                    "contracts/ab.md".into()
                ]
            ),
            ["src/b/y.rs", "contracts/ab.md"]
        );
    }
}
