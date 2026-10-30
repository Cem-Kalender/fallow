//! Pipelines (007 contracts/pipeline.md, research S-12): TOML files built from fixed steps, and
//! whether a pipeline verifies what it builds (FR-015).

use serde::{Deserialize, Serialize};

use super::agent::{Kind, Source};

/// The built-in step: Fallow runs the project's gates against the baseline.
pub const GATES: &str = "fallow:gates";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Task,
    Phase,
    Checkpoint,
    Once,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum At {
    Start,
    #[default]
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFail {
    Repair,
    Engineer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub agent: String,
    pub when: When,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    pub on_fail: OnFail,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parallel_with: Vec<String>,
}

impl Step {
    pub fn at(&self) -> At {
        self.at.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    #[serde(default = "three")]
    pub task_repairs: u32,
    #[serde(default = "three")]
    pub review_rounds: u32,
}

fn three() -> u32 {
    3
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            task_repairs: 3,
            review_rounds: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Raw {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub caps: Caps,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub caps: Caps,
    pub steps: Vec<Step>,
    pub source: Source,
    /// Gates after every task, and a checker at every checkpoint (FR-015).
    pub verifying: bool,
    /// What is not checked, when not verifying.
    pub missing: Vec<String>,
}

impl Pipeline {
    /// The builder step (the one maker per task).
    pub fn builder(&self) -> Option<&Step> {
        self.steps
            .iter()
            .find(|s| s.when == When::Task && s.agent != GATES)
    }

    /// The pipeline as TOML, as a loop records it.
    pub fn to_toml(&self) -> String {
        let raw = Raw {
            name: self.name.clone(),
            description: self.description.clone(),
            caps: self.caps,
            steps: self.steps.clone(),
        };
        toml::to_string(&raw).unwrap_or_default()
    }
}

/// Parses and checks a pipeline. `kind_of` gives an agent definition's kind, or `None` when
/// there is no such agent. Problems name the step by its number (from 1).
pub fn parse(
    file: &str,
    stem: &str,
    text: &str,
    source: Source,
    kind_of: &dyn Fn(&str) -> Option<Kind>,
) -> Result<Pipeline, Vec<String>> {
    let raw: Raw = toml::from_str(text).map_err(|e| vec![format!("{file}: {}", e.message())])?;
    let mut problems = Vec::new();
    if raw.name != stem {
        problems.push(format!(
            "{file}: name {:?} differs from the file name {stem:?}",
            raw.name
        ));
    }
    let mut makers_per_task = 0;
    for (i, s) in raw.steps.iter().enumerate() {
        let at = |m: String| format!("{file}: step {}: {m}", i + 1);
        if s.at.is_some() && s.when == When::Task {
            problems.push(at("`at` is for phase, checkpoint and once steps".into()));
        }
        for g in &s.paths {
            if globset::Glob::new(g).is_err() {
                problems.push(at(format!("paths: {g:?} is not a glob")));
            }
        }
        if s.agent == GATES {
            if !matches!(s.when, When::Task | When::Phase) {
                problems.push(at("fallow:gates runs per task or per phase".into()));
            }
            continue;
        }
        match kind_of(&s.agent) {
            None => problems.push(at(format!("no agent named {:?}", s.agent))),
            Some(Kind::Lead) => problems.push(at("the lead is not a pipeline step".into())),
            Some(Kind::Maker) => {
                if s.when == When::Task {
                    makers_per_task += 1;
                }
            }
            Some(Kind::Checker) => {
                if s.when == When::Task {
                    problems.push(at(
                        "a checker runs at phases and checkpoints, not per task".into()
                    ));
                }
                if s.at() == At::Start {
                    problems.push(at("a checker at the start has nothing to check yet".into()));
                }
            }
        }
        for other in &s.parallel_with {
            let ok = raw.steps.iter().any(|o| {
                &o.agent == other
                    && o.when == s.when
                    && o.at() == s.at()
                    && kind_of(other) == Some(Kind::Checker)
            });
            if !ok || kind_of(&s.agent) != Some(Kind::Checker) {
                problems.push(at(format!(
                    "parallel_with {other:?}: only checkers at the same point run together"
                )));
            }
        }
    }
    if makers_per_task != 1 {
        problems.push(format!(
            "{file}: a pipeline needs exactly one maker with when = \"task\" (the builder); it has {makers_per_task}"
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let mut missing = Vec::new();
    if !raw
        .steps
        .iter()
        .any(|s| s.agent == GATES && s.when == When::Task)
    {
        missing.push("no gates after each task".to_string());
    }
    if !raw.steps.iter().any(|s| {
        s.when == When::Checkpoint
            && s.at() == At::End
            && s.paths.is_empty()
            && kind_of(&s.agent) == Some(Kind::Checker)
    }) {
        missing.push("no review at checkpoints".to_string());
    }
    Ok(Pipeline {
        name: raw.name,
        description: raw.description,
        caps: raw.caps,
        steps: raw.steps,
        source,
        verifying: missing.is_empty(),
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(name: &str) -> Option<Kind> {
        match name {
            "builder" | "test-writer" | "docs-writer" => Some(Kind::Maker),
            "reviewer" | "security-reviewer" => Some(Kind::Checker),
            "lead" => Some(Kind::Lead),
            _ => None,
        }
    }

    fn p(text: &str) -> Result<Pipeline, Vec<String>> {
        parse("p.toml", "p", text, Source::User, &kinds)
    }

    #[test]
    fn the_shipped_defaults_parse_and_only_solo_is_unverified() {
        for (stem, text) in super::super::DEFAULT_PIPELINES {
            let pl = parse("x", stem, text, Source::Default, &kinds)
                .unwrap_or_else(|e| panic!("{stem}: {e:?}"));
            assert_eq!(pl.verifying, *stem != "solo", "{stem}: {:?}", pl.missing);
            assert_eq!(pl.builder().unwrap().agent, "builder");
        }
    }

    #[test]
    fn the_contracts_example_parses_with_a_path_limited_checker() {
        let pl = p(r#"
name = "p"
[caps]
task_repairs = 2
[[step]]
agent = "builder"
when = "task"
on_fail = "repair"
[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"
[[step]]
agent = "reviewer"
when = "checkpoint"
on_fail = "repair"
[[step]]
agent = "security-reviewer"
when = "checkpoint"
paths = ["api/**", "auth/**"]
parallel_with = ["reviewer"]
on_fail = "repair"
[[step]]
agent = "docs-writer"
when = "once"
at = "end"
on_fail = "engineer"
"#)
        .unwrap();
        assert!(pl.verifying);
        assert_eq!(pl.caps.task_repairs, 2);
        assert_eq!(pl.caps.review_rounds, 3);
        let again = parse("x", "p", &pl.to_toml(), Source::User, &kinds).unwrap();
        assert_eq!(again.steps, pl.steps, "the recorded TOML reads back");
    }

    #[test]
    fn a_path_limited_review_alone_does_not_verify() {
        let pl = p("name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"security-reviewer\"\nwhen = \"checkpoint\"\npaths = [\"api/**\"]\non_fail = \"repair\"\n").unwrap();
        assert!(!pl.verifying);
        assert_eq!(
            pl.missing,
            ["no gates after each task", "no review at checkpoints"]
        );
    }

    #[test]
    fn the_checks_refuse_with_the_step_number() {
        let e = p(
            "name = \"p\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"task\"\non_fail = \"repair\"\n",
        )
        .unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("step 1") && x.contains("not per task")),
            "{e:?}"
        );
        assert!(e.iter().any(|x| x.contains("exactly one maker")), "{e:?}");
        let e = p("name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n[[step]]\nagent = \"ghost\"\nwhen = \"phase\"\non_fail = \"engineer\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\nat = \"start\"\non_fail = \"repair\"\n").unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("step 2") && x.contains("per task or per phase")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|x| x.contains("step 3") && x.contains("no agent named")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|x| x.contains("step 4") && x.contains("nothing to check")),
            "{e:?}"
        );
        let e = p("name = \"p\"\nwhatever = 1\n").unwrap_err();
        assert!(e[0].contains("unknown field"), "{e:?}");
        let e = parse(
            "q.toml",
            "q",
            "name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n",
            Source::User,
            &kinds,
        )
        .unwrap_err();
        assert!(e[0].contains("differs from the file name"));
    }
}
