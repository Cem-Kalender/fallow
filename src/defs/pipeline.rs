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

    /// 018: the point a step runs at, `(when, at)`. For a task step other than the builder and
    /// the gates, before or after each task; a checker per task without `at` runs after it
    /// (research R-1). The builder and the gates are the task itself (`At::End`).
    pub fn point_at(&self) -> (When, At) {
        (self.when, self.at())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    #[serde(default = "three")]
    pub task_repairs: u32,
    #[serde(default = "three")]
    pub review_rounds: u32,
    /// 018: the most builders at once on a group of `[P]` tasks; 1..=8, default 1, written only
    /// when not 1 so a pipeline with one builder records what 0.4.0 recorded (research R-1).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub builders: u32,
}

fn three() -> u32 {
    3
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

/// 018: the most builders a pipeline may allow.
pub const MAX_BUILDERS: u32 = 8;

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            task_repairs: 3,
            review_rounds: 3,
            builders: 1,
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
    /// 018: the places of the makers with `when = "task"` and no `at` (one: the builder).
    #[serde(skip)]
    pub makers_per_task: Vec<usize>,
    /// 018: each step's agent kind, as the checks found it (`None` for the gates).
    #[serde(skip)]
    pub kinds: Vec<Option<Kind>>,
}

/// 018: an agent's name in plain words (`security-reviewer`: "security reviewer").
fn plain(agent: &str) -> String {
    agent.replace(['-', '_'], " ")
}

/// 018: names joined in plain words ("a", "a and b", "a, b and c").
fn joined(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// 018: where a step runs, in plain words.
fn place(when: When, at: At) -> &'static str {
    match (when, at) {
        (When::Task, At::Start) => "before each task",
        (When::Task, At::End) => "after each task",
        (When::Phase, At::Start) => "at the start of every phase",
        (When::Phase, At::End) => "at the end of every phase",
        (When::Checkpoint, At::Start) => "before each user story is built",
        (When::Checkpoint, At::End) => "at each user story's checkpoint",
        (When::Once, At::Start) => "once at the start",
        (When::Once, At::End) => "once at the end",
    }
}

impl Pipeline {
    /// The builder step: the one maker per task, the `when = "task"` step without `at` that is
    /// not the gates. A checker per task without `at` comes after it, so the first such step is
    /// the builder in every pipeline the checks accept (a checker before the builder is not, as
    /// `parse` makes sure).
    pub fn builder(&self) -> Option<&Step> {
        self.builder_index().map(|i| &self.steps[i])
    }

    /// The builder's place among the steps.
    pub fn builder_index(&self) -> Option<usize> {
        self.makers_per_task.first().copied()
    }

    /// 018 (research R-12, spec FR-002): what the pipeline checks, as plain sentences, in the
    /// order of its steps: the builders on `[P]` tasks when more than one, the gates, and each
    /// agent with where it runs. A checker alongside an earlier checker at the same point
    /// (whichever of the two names the other in `parallel_with`) is said with it ("alongside
    /// the security reviewer"), unless it has `paths` of its own. The builder is not named.
    pub fn checks(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.caps.builders > 1 {
            out.push(format!(
                "Up to {} builders work on [P] tasks at the same time, each in its own working copy.",
                self.caps.builders
            ));
        }
        let builder = self.builder_index();
        for (i, st) in self.steps.iter().enumerate() {
            if Some(i) == builder {
                continue;
            }
            let (when, at) = st.point_at();
            if st.agent == GATES {
                out.push(match when {
                    When::Phase => "Fallow runs the gates at the end of every phase.".into(),
                    _ => "Fallow runs the gates after every task.".into(),
                });
                continue;
            }
            let checker = self.kinds.get(i).copied().flatten() == Some(Kind::Checker);
            let folded = checker
                && st.paths.is_empty()
                && self.steps[..i].iter().enumerate().any(|(j, o)| {
                    o.point_at() == (when, at)
                        && self.kinds.get(j).copied().flatten() == Some(Kind::Checker)
                        && (o.parallel_with.iter().any(|p| p == &st.agent)
                            || st.parallel_with.iter().any(|p| p == &o.agent))
                });
            if folded {
                continue;
            }
            let name = plain(&st.agent);
            let mut sentence = if !checker {
                format!("The {name} works {}", place(when, at))
            } else if (when, at) == (When::Checkpoint, At::End) && st.paths.is_empty() {
                format!(
                    "The {name} checks every user story's acceptance scenarios at its checkpoint"
                )
            } else if (when, at) == (When::Checkpoint, At::End) {
                format!("The {name} checks each story")
            } else {
                format!("The {name} checks the work {}", place(when, at))
            };
            if !st.paths.is_empty() {
                sentence.push_str(&format!(" when {} changed", st.paths.join(" or ")));
            }
            let alongside: Vec<String> = self
                .steps
                .iter()
                .filter(|o| o.agent != st.agent && o.point_at() == (when, at))
                .filter(|o| {
                    st.parallel_with.contains(&o.agent) || o.parallel_with.contains(&st.agent)
                })
                .map(|o| format!("the {}", plain(&o.agent)))
                .collect();
            if checker && !alongside.is_empty() {
                sentence.push_str(&format!(", alongside {}", joined(&alongside)));
            }
            sentence.push('.');
            out.push(sentence);
        }
        out
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

/// Parses and checks a pipeline. `kind_of` gives an agent definition's kind, or why there is no
/// usable agent of that name (017 FR-019). Problems name the step by its number (from 1).
pub fn parse(
    file: &str,
    stem: &str,
    text: &str,
    source: Source,
    kind_of: &dyn Fn(&str) -> Result<Kind, String>,
) -> Result<Pipeline, Vec<String>> {
    let raw: Raw = toml::from_str(text).map_err(|e| vec![format!("{file}: {}", e.message())])?;
    let mut problems = Vec::new();
    if raw.name != stem {
        problems.push(format!(
            "{file}: name {:?} differs from the file name {stem:?}",
            raw.name
        ));
    }
    if !(1..=MAX_BUILDERS).contains(&raw.caps.builders) {
        problems.push(format!(
            "{file}: caps: builders must be between 1 and {MAX_BUILDERS}; it is {}",
            raw.caps.builders
        ));
    }
    // 018 (research R-1): the builder is the one maker per task without `at`.
    let task_makers: Vec<(usize, bool)> = raw
        .steps
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.when == When::Task && s.agent != GATES && kind_of(&s.agent) == Ok(Kind::Maker)
        })
        .map(|(i, s)| (i, s.at.is_some()))
        .collect();
    let makers_per_task: Vec<usize> = task_makers
        .iter()
        .filter(|(_, at)| !at)
        .map(|(i, _)| *i)
        .collect();
    for (i, s) in raw.steps.iter().enumerate() {
        let at = |m: String| format!("{file}: step {}: {m}", i + 1);
        for g in &s.paths {
            if globset::Glob::new(g).is_err() {
                problems.push(at(format!("paths: {g:?} is not a glob")));
            }
        }
        if s.agent == GATES {
            if !matches!(s.when, When::Task | When::Phase) {
                problems.push(at("fallow:gates runs per task or per phase".into()));
            }
            if s.at.is_some() {
                problems.push(at(
                    "fallow:gates runs after the work it checks: no `at`".into()
                ));
            }
            if !s.paths.is_empty() {
                problems.push(at("fallow:gates runs on every change: no `paths`".into()));
            }
            continue;
        }
        match kind_of(&s.agent) {
            Err(why) => problems.push(at(why)),
            Ok(Kind::Lead) => problems.push(at("the lead is not a pipeline step".into())),
            Ok(Kind::Assistant) => problems.push(at("the assistant is not a pipeline step".into())),
            Ok(Kind::Maker) => {
                if s.when == When::Task && s.at.is_none() && !s.paths.is_empty() {
                    problems.push(at("the builder builds every task: no `paths`".into()));
                }
                if s.when == When::Task && s.at.is_some() && makers_per_task.is_empty() {
                    problems.push(at(
                        "the builder (the maker per task) takes no `at`; with it, this maker runs before or after each task and the pipeline has no builder".into(),
                    ));
                }
            }
            Ok(Kind::Checker) => {
                if s.at() == At::Start && s.on_fail == OnFail::Repair {
                    problems.push(at(
                        "a checker at a start must ask the engineer (on_fail = \"engineer\"): nothing of that point's work is built yet".into(),
                    ));
                }
            }
        }
        for other in &s.parallel_with {
            let ok = raw.steps.iter().any(|o| {
                &o.agent == other
                    && o.point_at() == s.point_at()
                    && kind_of(other) == Ok(Kind::Checker)
            });
            if !ok || kind_of(&s.agent) != Ok(Kind::Checker) {
                problems.push(at(format!(
                    "parallel_with {other:?}: only checkers at the same point run together"
                )));
            }
        }
    }
    if makers_per_task.len() != 1 {
        problems.push(format!(
            "{file}: a pipeline needs exactly one maker with when = \"task\" and no `at` (the builder); it has {}",
            makers_per_task.len()
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
            && kind_of(&s.agent) == Ok(Kind::Checker)
    }) {
        missing.push("no review at checkpoints".to_string());
    }
    Ok(Pipeline {
        name: raw.name,
        description: raw.description,
        caps: raw.caps,
        source,
        verifying: missing.is_empty(),
        missing,
        makers_per_task,
        kinds: raw
            .steps
            .iter()
            .map(|s| (s.agent != GATES).then(|| kind_of(&s.agent).ok()).flatten())
            .collect(),
        steps: raw.steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(name: &str) -> Result<Kind, String> {
        match name {
            "builder" | "test-writer" | "docs-writer" => Ok(Kind::Maker),
            "reviewer" | "security-reviewer" | "lint-checker" => Ok(Kind::Checker),
            "lead" => Ok(Kind::Lead),
            "assistant" => Ok(Kind::Assistant),
            _ => Err(format!("there is no agent named {name:?}")),
        }
    }

    #[test]
    fn the_assistant_is_not_a_pipeline_step() {
        let e = p("name = \"p\"
[[step]]
agent = \"assistant\"
when = \"task\"
on_fail = \"repair\"
")
        .unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("the assistant is not a pipeline step")),
            "{e:?}"
        );
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
    fn gates_and_the_builder_take_no_paths() {
        let e = p("name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\npaths = [\"docs/**\"]\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n").unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("step 2") && x.contains("no `paths`")),
            "{e:?}"
        );
    }

    #[test]
    fn the_checks_refuse_with_the_step_number() {
        let e = p(
            "name = \"p\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"task\"\non_fail = \"repair\"\n",
        )
        .unwrap_err();
        // 018: a checker per task runs after each task; only the builder is missing.
        assert!(!e.iter().any(|x| x.contains("step 1")), "{e:?}");
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
                .any(|x| x.contains("step 4")
                    && x.contains("nothing of that point's work is built yet")),
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

    // ---- 018 (T003): the format's additions -------------------------------------------------

    const PIPELINE_040_STANDARD: &str = r#"name = "standard"
description = "One builder task by task, the gates after every task, and a review at every user story's checkpoint"

[caps]
task_repairs = 3
review_rounds = 3

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
"#;
    const PIPELINE_040_TEST_FIRST: &str = r#"name = "test-first"
description = "Standard, with each user story's acceptance tests written first by the test-writer"

[caps]
task_repairs = 3
review_rounds = 3

[[step]]
agent = "test-writer"
when = "checkpoint"
at = "start"
on_fail = "engineer"

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
"#;
    const PIPELINE_040_SOLO: &str = r#"name = "solo"
description = "One builder task by task with the gates, and no review: results are not verified by Fallow"

[caps]
task_repairs = 3
review_rounds = 3

[[step]]
agent = "builder"
when = "task"
on_fail = "repair"

[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"
"#;

    /// Standard's steps with `caps` (the lines after `[caps]`) and `more` steps after them.
    fn standard_with(caps: &str, more: &str) -> String {
        format!(
            "name = \"p\"\n[caps]\n{caps}\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n{more}"
        )
    }

    fn step(agent: &str, when: &str, at: Option<&str>, on_fail: &str, extra: &str) -> String {
        let at = at.map(|a| format!("at = \"{a}\"\n")).unwrap_or_default();
        format!(
            "[[step]]\nagent = \"{agent}\"\nwhen = \"{when}\"\n{at}on_fail = \"{on_fail}\"\n{extra}"
        )
    }

    #[test]
    fn builders_default_to_one_and_stay_within_one_to_eight() {
        assert_eq!(p(&standard_with("", "")).unwrap().caps.builders, 1);
        for n in [1, 8] {
            let pl = p(&standard_with(&format!("builders = {n}"), "")).unwrap();
            assert_eq!(pl.caps.builders, n);
        }
        for n in [0, 9] {
            let e = p(&standard_with(&format!("builders = {n}"), "")).unwrap_err();
            assert!(
                e.iter()
                    .any(|x| x.contains("builders must be between 1 and 8")),
                "{n}: {e:?}"
            );
        }
    }

    #[test]
    fn one_builder_writes_what_0_4_0_wrote() {
        for (stem, text) in [
            ("standard", PIPELINE_040_STANDARD),
            ("test-first", PIPELINE_040_TEST_FIRST),
            ("solo", PIPELINE_040_SOLO),
        ] {
            let pl = parse("x", stem, text, Source::Default, &kinds).unwrap();
            assert_eq!(pl.caps.builders, 1);
            let toml = pl.to_toml();
            assert!(!toml.contains("builders"), "{stem}: {toml}");
            assert_eq!(toml, text, "{stem}: 0.4.0's TOML, byte for byte");
        }
        let pl = p(&standard_with("builders = 3", "")).unwrap();
        assert!(pl.to_toml().contains("builders = 3"), "{}", pl.to_toml());
    }

    #[test]
    fn steps_before_and_after_each_task() {
        let pl = p(&standard_with(
            "",
            &[
                step("test-writer", "task", Some("start"), "engineer", ""),
                step("docs-writer", "task", Some("end"), "repair", ""),
                step("reviewer", "task", Some("start"), "engineer", ""),
                step("lint-checker", "task", Some("end"), "repair", ""),
                step("security-reviewer", "task", None, "repair", ""),
            ]
            .concat(),
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(pl.builder().unwrap().agent, "builder");
        let point = |agent: &str| {
            pl.steps
                .iter()
                .find(|s| s.agent == agent)
                .unwrap()
                .point_at()
        };
        assert_eq!(point("test-writer"), (When::Task, At::Start));
        assert_eq!(point("lint-checker"), (When::Task, At::End));
        assert_eq!(
            point("security-reviewer"),
            (When::Task, At::End),
            "a checker per task without `at` runs after the task"
        );
        assert!(
            pl.verifying,
            "checkers per task change nothing: {:?}",
            pl.missing
        );
    }

    #[test]
    fn the_builder_and_the_gates_take_no_at() {
        let e = p("name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\nat = \"start\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\nat = \"end\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n").unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("step 1") && x.contains("the builder") && x.contains("`at`")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|x| x.contains("step 2") && x.contains("fallow:gates") && x.contains("`at`")),
            "{e:?}"
        );
        let e = p(&standard_with(
            "",
            &step("fallow:gates", "phase", Some("start"), "repair", ""),
        ))
        .unwrap_err();
        assert!(
            e.iter().any(|x| x.contains("step 4") && x.contains("`at`")),
            "{e:?}"
        );
    }

    #[test]
    fn a_checker_at_a_start_asks_the_engineer() {
        for when in ["task", "phase", "checkpoint", "once"] {
            let e = p(&standard_with(
                "",
                &step("lint-checker", when, Some("start"), "repair", ""),
            ))
            .unwrap_err();
            assert!(
                e.iter().any(|x| x.contains("step 4")
                    && x.contains("nothing of that point's work is built yet")),
                "{when}: {e:?}"
            );
            p(&standard_with(
                "",
                &step("lint-checker", when, Some("start"), "engineer", ""),
            ))
            .unwrap_or_else(|e| panic!("{when}: {e:?}"));
        }
    }

    #[test]
    fn only_checkers_at_one_point_run_alongside_each_other() {
        let e = p(&standard_with(
            "",
            &[
                step(
                    "test-writer",
                    "phase",
                    None,
                    "engineer",
                    "parallel_with = [\"docs-writer\"]\n",
                ),
                step("docs-writer", "phase", None, "engineer", ""),
            ]
            .concat(),
        ))
        .unwrap_err();
        assert!(
            e.iter().any(|x| x.contains("step 4")
                && x.contains("only checkers at the same point run together")),
            "{e:?}"
        );
        p(&standard_with(
            "",
            &[
                step(
                    "lint-checker",
                    "phase",
                    Some("end"),
                    "repair",
                    "parallel_with = [\"security-reviewer\"]\n",
                ),
                step("security-reviewer", "phase", None, "repair", ""),
            ]
            .concat(),
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
        p(&standard_with(
            "",
            &[
                step(
                    "lint-checker",
                    "task",
                    Some("end"),
                    "repair",
                    "parallel_with = [\"security-reviewer\"]\n",
                ),
                step("security-reviewer", "task", None, "repair", ""),
            ]
            .concat(),
        ))
        .unwrap_or_else(|e| panic!("checkers after each task, alongside: {e:?}"));
        let e = p(&standard_with(
            "",
            &[
                step(
                    "lint-checker",
                    "phase",
                    Some("end"),
                    "repair",
                    "parallel_with = [\"security-reviewer\"]\n",
                ),
                step("security-reviewer", "task", None, "repair", ""),
            ]
            .concat(),
        ))
        .unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("step 4") && x.contains("same point")),
            "{e:?}"
        );
    }

    #[test]
    fn exactly_one_builder_is_still_required() {
        let e = p(&standard_with(
            "",
            &step("docs-writer", "task", None, "repair", ""),
        ))
        .unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("exactly one maker") && x.contains("it has 2")),
            "{e:?}"
        );
        let e = p("name = \"p\"\n[[step]]\nagent = \"test-writer\"\nwhen = \"task\"\nat = \"start\"\non_fail = \"engineer\"\n").unwrap_err();
        assert!(
            e.iter()
                .any(|x| x.contains("exactly one maker") && x.contains("it has 0")),
            "a maker before each task is not the builder: {e:?}"
        );
    }

    // ---- 018 US1 (T007): what a pipeline checks, in plain words ------------------------------

    #[test]
    fn a_pipeline_says_what_it_checks() {
        let std = parse(
            "x",
            "standard",
            PIPELINE_040_STANDARD,
            Source::Default,
            &kinds,
        )
        .unwrap();
        assert_eq!(
            std.checks(),
            [
                "Fallow runs the gates after every task.",
                "The reviewer checks every user story's acceptance scenarios at its checkpoint.",
            ]
        );
        let solo = parse("x", "solo", PIPELINE_040_SOLO, Source::Default, &kinds).unwrap();
        assert_eq!(solo.checks(), ["Fallow runs the gates after every task."]);
        let tf = parse(
            "x",
            "test-first",
            PIPELINE_040_TEST_FIRST,
            Source::Default,
            &kinds,
        )
        .unwrap();
        assert_eq!(
            tf.checks()[0],
            "The test writer works before each user story is built."
        );
        let more = p(&standard_with(
            "builders = 3",
            &[
                step(
                    "security-reviewer",
                    "checkpoint",
                    None,
                    "repair",
                    "paths = [\"api/**\", \"auth/**\"]\n",
                ),
                step("docs-writer", "once", Some("end"), "engineer", ""),
            ]
            .concat(),
        ))
        .unwrap();
        assert_eq!(
            more.checks(),
            [
                "Up to 3 builders work on [P] tasks at the same time, each in its own working copy.",
                "Fallow runs the gates after every task.",
                "The reviewer checks every user story's acceptance scenarios at its checkpoint.",
                "The security reviewer checks each story when api/** or auth/** changed.",
                "The docs writer works once at the end.",
            ]
        );
        let alongside = p(&standard_with(
            "",
            &step("security-reviewer", "checkpoint", None, "repair", ""),
        )
        .replace(
            "agent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n",
            "agent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\nparallel_with = [\"security-reviewer\"]\n",
        ))
        .unwrap();
        assert_eq!(
            alongside.checks()[1],
            "The reviewer checks every user story's acceptance scenarios at its checkpoint, alongside the security reviewer."
        );
        assert_eq!(alongside.checks().len(), 2, "{:?}", alongside.checks());
        // Named the other way round (the later checker names the earlier): the same sentences.
        let back = p(&standard_with(
            "",
            &step(
                "security-reviewer",
                "checkpoint",
                None,
                "repair",
                "parallel_with = [\"reviewer\"]\n",
            ),
        ))
        .unwrap();
        assert_eq!(back.checks(), alongside.checks());
    }

    #[test]
    fn checkers_elsewhere_than_checkpoints_do_not_check_every_story() {
        let pl = p("name = \"p\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"phase\"\non_fail = \"repair\"\n[[step]]\nagent = \"lint-checker\"\nwhen = \"task\"\non_fail = \"repair\"\n").unwrap();
        assert!(!pl.verifying);
        assert_eq!(pl.missing, ["no review at checkpoints"]);
        assert_eq!(
            pl.checks(),
            [
                "Fallow runs the gates after every task.",
                "The reviewer checks the work at the end of every phase.",
                "The lint checker checks the work after each task.",
            ]
        );
    }
}
