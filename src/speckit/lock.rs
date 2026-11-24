//! The lock checks (007 FR-008, research S-8, contracts/spec-kit-files.md): what must hold
//! before a feature's documents are locked and its loop starts.

use serde::{Deserialize, Serialize};

use super::{Problem, plan, spec, tasks};
use crate::run::events::PhaseSpec;

/// One finding of the analyze step, as the lead reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: String,
    pub text: String,
    #[serde(default)]
    pub dismissed: bool,
}

/// What the lock reads from the documents when they pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Locked {
    pub phases: Vec<PhaseSpec>,
    pub spec: spec::Spec,
    pub commands: Vec<String>,
}

/// Checks the feature folder's documents (`spec`, `plan`, `tasks`: their text, `None` when
/// missing), the step reached, and the analyze findings. `dir` is `specs/NNN-name`.
pub fn check(
    dir: &str,
    spec_text: Option<&str>,
    plan_text: Option<&str>,
    tasks_text: Option<&str>,
    analyzed: bool,
    findings: &[Finding],
) -> Result<Locked, Vec<String>> {
    let mut problems: Vec<String> = Vec::new();
    let show = |ps: Vec<Problem>| ps.into_iter().map(|p| p.to_string()).collect::<Vec<_>>();
    if !analyzed {
        problems.push("the analyze step has not run: ask the lead to run it".into());
    }
    for f in findings {
        if f.severity.eq_ignore_ascii_case("critical") && !f.dismissed {
            problems.push(format!("analyze found a CRITICAL problem: {}", f.text));
        }
    }
    let spec_file = format!("{dir}/spec.md");
    let plan_file = format!("{dir}/plan.md");
    let tasks_file = format!("{dir}/tasks.md");
    let spec = match spec_text {
        None => {
            problems.push(format!("{spec_file} is missing"));
            None
        }
        Some(t) => match spec::read(&spec_file, t) {
            Ok(s) => Some(s),
            Err(ps) => {
                problems.extend(show(ps));
                None
            }
        },
    };
    let commands = match plan_text {
        None => {
            problems.push(format!("{plan_file} is missing"));
            Vec::new()
        }
        Some(t) => match plan::commands(&plan_file, t) {
            Ok(c) => c,
            Err(ps) => {
                problems.extend(show(ps));
                Vec::new()
            }
        },
    };
    let phases = match tasks_text {
        None => {
            problems.push(format!("{tasks_file} is missing"));
            None
        }
        Some(t) => match tasks::read(&tasks_file, t) {
            Ok(p) => Some(p),
            Err(ps) => {
                problems.extend(show(ps));
                None
            }
        },
    };
    if let Some(s) = &spec {
        for m in &s.markers {
            problems.push(format!(
                "{spec_file}:{}: an open clarification: {}",
                m.line, m.text
            ));
        }
        if s.stories.is_empty() {
            problems.push(format!("{spec_file}: no user stories"));
        }
        for st in &s.stories {
            if st.scenarios.is_empty() {
                problems.push(format!(
                    "{spec_file}:{}: {} has no acceptance scenarios",
                    st.line, st.id
                ));
            } else if !st.scenarios.iter().any(|sc| sc.must_fail) {
                problems.push(format!(
                    "{spec_file}:{}: {} has no scenario marked (must fail)",
                    st.line, st.id
                ));
            }
        }
        if let Some(ps) = &phases {
            for p in ps {
                if let Some(story) = &p.story {
                    if s.story(story).is_none() {
                        problems.push(format!(
                            "{tasks_file}: phase {} is for {story}, which {spec_file} does not have",
                            p.n
                        ));
                    }
                }
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Locked {
        phases: phases.unwrap_or_default(),
        spec: spec.unwrap_or_default(),
        commands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "### User Story 1 - Add (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** x, **Then** y.\n2. **Given** 9 over stock, **Then** refused (must fail)\n";
    const TASKS: &str =
        "## Phase 1: Setup\n- [ ] T001 a\n## Phase 2: User Story 1 - Add\n- [ ] T002 [US1] b\n";
    const PLAN: &str = "# Plan\n## Commands\n- `python -m shop`\n";

    #[test]
    fn good_documents_lock() {
        let l = check(
            "specs/001-x",
            Some(SPEC),
            Some(PLAN),
            Some(TASKS),
            true,
            &[],
        )
        .unwrap();
        assert_eq!(l.phases.len(), 2);
        assert_eq!(l.commands, ["python -m shop"]);
    }

    #[test]
    fn every_problem_is_named() {
        let spec = "### User Story 1 - Add (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** x [NEEDS CLARIFICATION: which?]\n";
        let tasks = "## Phase 2: User Story 2 - Other\n- [ ] T002 [US2] b\n";
        let crit = [Finding {
            severity: "CRITICAL".into(),
            text: "tasks miss FR-3".into(),
            dismissed: false,
        }];
        let e = check("d", Some(spec), None, Some(tasks), false, &crit).unwrap_err();
        let all = e.join("\n");
        for want in [
            "analyze step has not run",
            "CRITICAL problem: tasks miss FR-3",
            "d/plan.md is missing",
            "open clarification",
            "US1 has no scenario marked (must fail)",
            "is for US2, which d/spec.md does not have",
        ] {
            assert!(all.contains(want), "missing {want:?} in\n{all}");
        }
        let dismissed = [Finding {
            dismissed: true,
            ..crit[0].clone()
        }];
        assert!(check("d", Some(SPEC), Some(PLAN), Some(TASKS), true, &dismissed).is_ok());
        let e = check(
            "d",
            Some(SPEC),
            Some("## Commands\n- `git push`\n"),
            Some(TASKS),
            true,
            &[],
        )
        .unwrap_err();
        assert!(e[0].contains("git push"));
    }
}
