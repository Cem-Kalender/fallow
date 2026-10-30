//! The plan of a run, `.fallow/work.toml` (006 research R-8, contracts/plan.md).
//!
//! The planner writes it and commits it on the run's plan branch; Fallow reads it at a commit,
//! never from a worktree, and never writes it. A run is one task: the plan says whether one
//! builder builds it (the default, constitution IV) or parallel parts with owned files and a
//! contract per boundary. Criteria are named per part and addressed as `part/id` everywhere
//! (constitution III). Prose stays in the documents `docs` lists.

use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::Spanned;

use crate::config::merge::line_of;

pub const PLAN_FILE: &str = ".fallow/work.toml";
pub const MAX_PARTS: usize = 8;
/// The one part of a plan built by one builder.
pub const MAIN: &str = "main";

/// How the work is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Build {
    /// One builder, one part named `main` (the default).
    Builder,
    /// Parallel parts with owned files and a contract per boundary (FR-021).
    Parts,
}

/// What must hold for a part to be done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub text: String,
    /// A case that must be rejected; the review shows it was (FR-007).
    #[serde(default)]
    pub must_fail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    /// Globs of the files this part may change: its code and its tests. Empty for the one
    /// builder of a `builder` plan: everything except the plan's own files.
    pub owns: Vec<String>,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub path: String,
    pub between: Vec<String>,
    /// A gate that runs the contract's tests.
    pub tests: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The commit it was read at.
    pub commit: Option<String>,
    pub docs: Vec<String>,
    pub shared: Vec<String>,
    pub test_patterns: Option<Vec<String>>,
    /// Commands the builder and the reviewer may run besides the gates, with any arguments
    /// (`python -m shop`): approved with the plan.
    pub commands: Vec<String>,
    pub build: Build,
    pub reason: Option<String>,
    pub parts: Vec<Part>,
    pub contracts: Vec<Contract>,
}

impl Plan {
    pub fn part(&self, id: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// Every criterion as (`part/id`, criterion).
    pub fn criteria(&self) -> Vec<(String, &Criterion)> {
        self.parts
            .iter()
            .flat_map(|p| {
                p.criteria
                    .iter()
                    .map(move |c| (format!("{}/{}", p.id, c.id), c))
            })
            .collect()
    }

    /// The contracts a part builds against.
    pub fn contracts_of<'a>(&'a self, part: &'a str) -> impl Iterator<Item = &'a Contract> + 'a {
        self.contracts
            .iter()
            .filter(move |c| c.between.iter().any(|p| p == part))
    }

    /// The files only the planner may change: this file, the contracts and the documents.
    pub fn plan_files(&self) -> Vec<String> {
        let mut v = vec![PLAN_FILE.to_string()];
        v.extend(self.contracts.iter().map(|c| c.path.clone()));
        v.extend(self.docs.iter().cloned());
        v
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPart {
    id: String,
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    criteria: Vec<Criterion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContract {
    path: String,
    #[serde(default)]
    between: Vec<String>,
    tests: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlan {
    #[serde(default)]
    docs: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
    test_patterns: Option<Vec<String>>,
    #[serde(default)]
    commands: Vec<String>,
    build: Option<String>,
    reason: Option<String>,
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    criteria: Vec<Criterion>,
    #[serde(default)]
    part: Vec<Spanned<RawPart>>,
    #[serde(default)]
    contract: Vec<Spanned<RawContract>>,
}

/// Names: `[a-z][a-z0-9-]{0,31}`.
pub fn valid_id(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn check_criteria(at: &str, part: &str, list: &[Criterion], problems: &mut Vec<String>) {
    if list.is_empty() {
        problems.push(format!("{at}: part {part:?} has no acceptance criteria"));
        return;
    }
    if !list.iter().any(|c| c.must_fail) {
        problems.push(format!(
            "{at}: part {part:?} needs at least one criterion with must_fail = true"
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for c in list {
        if !valid_id(&c.id) {
            problems.push(format!(
                "{at}: {part}/{}: criterion ids are [a-z][a-z0-9-]{{0,31}}",
                c.id
            ));
        }
        if c.text.trim().is_empty() {
            problems.push(format!("{at}: {part}/{} has no text", c.id));
        }
        if seen.contains(&c.id.as_str()) {
            problems.push(format!("{at}: {part}/{} is not unique in its part", c.id));
        }
        seen.push(&c.id);
    }
}

/// Parses a plan. Problems are `file:line: message` (the T050 lesson: name them, never say the
/// file is missing).
pub fn parse(file: &str, text: &str) -> Result<Plan, Vec<String>> {
    let raw: RawPlan = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return Err(vec![format!("{file}:{line}: {}", e.message())]);
        }
    };
    let at = |span: std::ops::Range<usize>| format!("{file}:{}", line_of(text, span.start));
    let top = format!("{file}:1");
    let mut problems = Vec::new();
    for c in &raw.commands {
        if let Some(why) = crate::project::commands::command_problem(c) {
            problems.push(format!("{top}: command {c:?}: {why}"));
        }
    }
    let build = match raw.build.as_deref() {
        None | Some("builder") => Build::Builder,
        Some("parts") => Build::Parts,
        Some(b) => {
            problems.push(format!(
                "{top}: build {b:?}: expected \"builder\" or \"parts\""
            ));
            Build::Builder
        }
    };
    let reason = raw.reason.filter(|r| !r.trim().is_empty());
    let mut parts: Vec<Part> = Vec::new();
    let mut contracts: Vec<Contract> = Vec::new();
    for g in raw.owns.iter().chain(raw.shared.iter()) {
        if globset::Glob::new(g).is_err() {
            problems.push(format!("{top}: bad glob {g:?}"));
        }
    }
    match build {
        Build::Builder => {
            if !raw.part.is_empty() || !raw.contract.is_empty() {
                problems.push(format!(
                    "{top}: [[part]] and [[contract]] need build = \"parts\""
                ));
            }
            check_criteria(&top, MAIN, &raw.criteria, &mut problems);
            parts.push(Part {
                id: MAIN.into(),
                owns: raw.owns,
                criteria: raw.criteria,
            });
        }
        Build::Parts => {
            if reason.is_none() {
                problems.push(format!("{top}: a split needs a reason"));
            }
            if !raw.criteria.is_empty() || !raw.owns.is_empty() {
                problems.push(format!(
                    "{top}: with parts, owns and criteria belong to each [[part]]"
                ));
            }
            if raw.part.len() < 2 || raw.part.len() > MAX_PARTS {
                problems.push(format!("{top}: a split has 2 to {MAX_PARTS} parts"));
            }
            for p in raw.part {
                let where_ = at(p.span());
                let p = p.into_inner();
                if !valid_id(&p.id) || p.id == MAIN {
                    problems.push(format!(
                        "{where_}: part id {:?}: expected [a-z][a-z0-9-]{{0,31}}, not \"main\"",
                        p.id
                    ));
                }
                if parts.iter().any(|x| x.id == p.id) {
                    problems.push(format!("{where_}: part id {:?} is not unique", p.id));
                }
                if p.owns.is_empty() {
                    problems.push(format!("{where_}: part {:?} owns no files", p.id));
                }
                for g in &p.owns {
                    if globset::Glob::new(g).is_err() {
                        problems.push(format!("{where_}: part {:?}: bad glob {g:?}", p.id));
                    }
                }
                check_criteria(&where_, &p.id, &p.criteria, &mut problems);
                parts.push(Part {
                    id: p.id,
                    owns: p.owns,
                    criteria: p.criteria,
                });
            }
            for c in raw.contract {
                let where_ = at(c.span());
                let c = c.into_inner();
                if c.path.trim().is_empty() {
                    problems.push(format!("{where_}: contract: path must not be empty"));
                }
                if c.between.len() != 2 {
                    problems.push(format!(
                        "{where_}: contract {:?}: between must name two parts",
                        c.path
                    ));
                }
                for p in &c.between {
                    if !parts.iter().any(|x| &x.id == p) {
                        problems.push(format!(
                            "{where_}: contract {:?}: undefined part {p:?}",
                            c.path
                        ));
                    }
                }
                contracts.push(Contract {
                    path: c.path,
                    between: c.between,
                    tests: c.tests,
                });
            }
        }
    }
    if problems.is_empty() {
        Ok(Plan {
            commit: None,
            docs: raw.docs,
            shared: raw.shared,
            test_patterns: raw.test_patterns,
            commands: raw.commands.iter().map(|c| c.trim().to_string()).collect(),
            build,
            reason,
            parts,
            contracts,
        })
    } else {
        Err(problems)
    }
}

/// The plan at `commit` in the repository at `dir`: `Ok(None)` when the commit has none.
pub fn at(dir: &Path, commit: &str) -> Result<Option<Plan>, Vec<String>> {
    match crate::verify::git::show(dir, commit, PLAN_FILE) {
        Err(e) => Err(vec![format!("{PLAN_FILE}: {e}")]),
        Ok(None) => Ok(None),
        Ok(Some(text)) => parse(PLAN_FILE, &text).map(|mut m| {
            m.commit = Some(commit.to_string());
            Some(m)
        }),
    }
}

/// A path a glob certainly matches, for comparing globs without files (`**` and `*` filled in).
fn sample(glob: &str) -> String {
    glob.replace("**", "x/y").replace(['*', '?'], "x")
}

pub fn glob_set(globs: &[String]) -> Option<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for g in globs {
        b.add(globset::Glob::new(g).ok()?);
    }
    b.build().ok()
}

/// Files (or, without files, sample paths of the globs) two lists of owned globs share.
pub fn overlap(a: &[String], b: &[String], files: &[String]) -> Vec<String> {
    let (Some(sa), Some(sb)) = (glob_set(a), glob_set(b)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| sa.is_match(f) && sb.is_match(f))
        .take(5)
        .cloned()
        .collect();
    if out.is_empty() {
        for g in a.iter().filter(|g| sb.is_match(sample(g))) {
            out.push(g.clone());
        }
        for g in b.iter().filter(|g| sa.is_match(sample(g))) {
            if !out.contains(g) {
                out.push(g.clone());
            }
        }
    }
    out
}

/// Whether owned globs cover something besides tests and plan documents.
pub fn owns_code(owns: &[String], tests: &[String], docs: &[String], files: &[String]) -> bool {
    let (Some(own), Some(t)) = (glob_set(owns), glob_set(tests)) else {
        return true;
    };
    let not_code = |f: &str| t.is_match(f) || docs.iter().any(|d| d == f);
    let existing: Vec<&String> = files.iter().filter(|f| own.is_match(f)).collect();
    existing.iter().any(|f| !not_code(f)) || owns.iter().any(|g| !not_code(&sample(g)))
}

/// What keeps a parsed plan at `commit` from being approved (data-model.md Plan): parts that
/// overlap or own only tests, and contracts that are not committed. Empty when approvable.
pub fn approval_problems(dir: &Path, plan: &Plan, commit: &str, tests: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if plan.build != Build::Parts {
        return out;
    }
    let files = crate::verify::git::files_at(dir, commit).unwrap_or_default();
    for (i, a) in plan.parts.iter().enumerate() {
        for b in &plan.parts[i + 1..] {
            let shared = overlap(&a.owns, &b.owns, &files);
            if !shared.is_empty() {
                out.push(format!(
                    "parts {:?} and {:?} both own {}",
                    a.id,
                    b.id,
                    shared.join(", ")
                ));
            }
        }
        if !owns_code(&a.owns, tests, &plan.docs, &files) {
            out.push(format!(
                "part {:?} owns only tests or plan documents; a part owns its code and its tests",
                a.id
            ));
        }
        if plan.contracts_of(&a.id).next().is_none() {
            out.push(format!(
                "part {:?} has no contract with another part; each boundary needs one",
                a.id
            ));
        }
    }
    for c in &plan.contracts {
        if !files.iter().any(|f| f == &c.path) {
            out.push(format!("contract {:?} is not committed", c.path));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOLO: &str = r#"
docs = ["specs/001-search/spec.md"]
criteria = [
  { id = "find", text = "find ignores case" },
  { id = "unknown", text = "an unknown name gives None", must_fail = true },
]
"#;

    #[test]
    fn the_plan_names_the_commands_agents_may_run() {
        let p = parse(
            "w",
            &format!(
                "commands = [\"python -m shop\"]
{SOLO}"
            ),
        )
        .unwrap();
        assert_eq!(p.commands, ["python -m shop"]);
        for bad in [
            "python",
            "python -m shop; rm x",
            "python -m shop | more",
            "git commit -m x",
            "powershell -c x",
            "python -m shop > out",
        ] {
            let text = format!(
                "commands = [{bad:?}]
{SOLO}"
            );
            assert!(parse("w", &text).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn a_solo_plan_has_one_part_main() {
        let p = parse(PLAN_FILE, SOLO).unwrap();
        assert_eq!(p.build, Build::Builder);
        assert_eq!(p.parts.len(), 1);
        assert_eq!(p.parts[0].id, MAIN);
        let ids: Vec<String> = p.criteria().into_iter().map(|(k, _)| k).collect();
        assert_eq!(ids, ["main/find", "main/unknown"]);
    }

    #[test]
    fn criteria_need_a_must_fail_case_and_unique_ids_per_part() {
        let e = parse(PLAN_FILE, "criteria = [{ id = \"a\", text = \"x\" }]").unwrap_err();
        assert!(e[0].contains("must_fail"), "{e:?}");
        let e = parse(
            PLAN_FILE,
            "criteria = [{ id = \"a\", text = \"x\", must_fail = true }, { id = \"a\", text = \"y\" }]",
        )
        .unwrap_err();
        assert!(e[0].contains("main/a is not unique"), "{e:?}");
        assert!(parse(PLAN_FILE, "").unwrap_err()[0].contains("no acceptance criteria"));
    }

    #[test]
    fn parts_may_share_criterion_ids() {
        let text = r#"
build = "parts"
reason = "they meet only at the Cart interface"
[[part]]
id = "cart-core"
owns = ["shop/cart.py", "tests/test_cart.py"]
criteria = [{ id = "total", text = "t", must_fail = true }]
[[part]]
id = "receipt"
owns = ["shop/receipt.py", "tests/test_receipt.py"]
criteria = [{ id = "total", text = "t", must_fail = true }]
[[contract]]
path = "contracts/cart.md"
between = ["cart-core", "receipt"]
"#;
        let p = parse(PLAN_FILE, text).unwrap();
        let ids: Vec<String> = p.criteria().into_iter().map(|(k, _)| k).collect();
        assert_eq!(ids, ["cart-core/total", "receipt/total"]);
        assert_eq!(p.contracts_of("receipt").count(), 1);
    }

    #[test]
    fn problems_name_file_and_line() {
        let e = parse(PLAN_FILE, "[[work]]\nname = \"cart\"\n").unwrap_err();
        assert!(e[0].starts_with(".fallow/work.toml:1:"), "{e:?}");
        let e = parse(PLAN_FILE, "build = \"parts\"\n[[part]]\nid = \"main\"\n").unwrap_err();
        assert!(e.iter().any(|x| x.contains("needs a reason")), "{e:?}");
        assert!(e.iter().any(|x| x.contains(":2: part id")), "{e:?}");
    }

    #[test]
    fn overlapping_globs_are_found() {
        let a = vec!["src/api/**".to_string()];
        let b = vec!["src/**".to_string()];
        assert!(!overlap(&a, &b, &[]).is_empty());
        let c = vec!["web/**".to_string()];
        assert!(overlap(&a, &c, &[]).is_empty());
        let tests = vec!["tests/**".to_string()];
        assert!(!owns_code(&["tests/x.py".into()], &tests, &[], &[]));
        assert!(owns_code(
            &["shop/cart.py".into(), "tests/x.py".into()],
            &tests,
            &[],
            &[]
        ));
    }
}
