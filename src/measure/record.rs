//! Run records of loops (007 research S-20; 005 contracts/run-record.md; constitution V and
//! VIII): written once when a loop ends, for comparing pipelines and settings later.
//!
//! A record holds only numbers, identifiers, fingerprints and fixed-value outcomes. It never
//! holds text the engineer or an agent wrote, names the engineer chose (a project, a feature, a
//! custom agent or pipeline), paths, commands or output: those are replaced by `custom`, counted,
//! or left out. `check` enforces this before anything is written, and the CI marker test checks
//! it from the outside (`tests/record_marker.rs`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::run::events::{Level, QuestionState, Stage, StepKind};
use crate::run::state::RunState;

/// The folder below Fallow's data folder; the name carries the format version.
pub const DIR: &str = "records/v1";

/// The names a record may carry where a name goes: what Fallow ships, or `custom`.
const AGENTS: &[&str] = &[
    "lead",
    "builder",
    "reviewer",
    "test-writer",
    "security-reviewer",
    "docs-writer",
    "fallow:gates",
    "custom",
];
const PIPELINES: &[&str] = &["standard", "test-first", "solo", "custom"];
const RUNTIMES: &[&str] = &["claude", "fake", "custom"];
const WHEN: &[&str] = &["task", "phase", "checkpoint", "once"];
const AT: &[&str] = &["start", "end"];
const ON_FAIL: &[&str] = &["repair", "engineer"];
const LEVELS: &[&str] = &["strict", "normal", "free"];
const OUTCOMES: &[&str] = &["merged", "ready", "done_unverified", "stopped"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRow {
    pub agent: String,
    pub when: String,
    pub at: String,
    /// How many path globs limit it (the globs are the engineer's text).
    pub paths: u32,
    pub on_fail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRow {
    pub agent: String,
    pub runtime: String,
    /// A provider's model id, or none.
    pub model: Option<String>,
    pub steps: u32,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Questions {
    pub answered_by_lead: u32,
    pub decided_by_lead: u32,
    pub relayed: u32,
    pub overruled: u32,
    pub confirmed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopRecord {
    pub pipeline: String,
    pub verifying: bool,
    /// The pipeline's steps, so that steps switched off are recorded too.
    pub steps: Vec<StepRow>,
    pub task_repairs_cap: u32,
    pub review_rounds_cap: u32,
    pub level: String,
    pub agents: Vec<AgentRow>,
    pub phases: u32,
    pub tasks: u32,
    pub tasks_accepted: u32,
    pub repairs: u32,
    pub review_rounds: u32,
    pub questions: Questions,
    pub interventions: u32,
    pub locks: u32,
    pub lock_commit: String,
    pub final_commit: Option<String>,
    pub started: String,
    pub minutes: u64,
    pub cost_usd: f64,
    pub outcome: String,
}

/// A model id of an agent on `runtime`, kept only when it is an identifier (constitution V):
/// 017 (FR-022, research R-8): it has the form its runtime declares (`claude-` or `fake-`
/// followed by lowercase letters, digits, `.` and `-`, at most 64 characters), or one of the
/// forms kept before 017, so records written then still validate: the Claude aliases, or a
/// family, a version number and fixed words after it. Anything else could carry the engineer's
/// words.
pub fn model_id(runtime: &str, s: &str) -> Option<String> {
    if crate::agent::registry::model_naming(runtime).is_some_and(|n| n.fits(s)) {
        return Some(s.to_string());
    }
    static FORM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let form = FORM.get_or_init(|| {
        regex::Regex::new(
            r"^(?:opus|sonnet|haiku|claude-(?:(?:opus|sonnet|haiku)-)?[0-9][0-9.\-]*(?:-(?:opus|sonnet|haiku))?(?:-[0-9]{8})?(?:\[1m\])?|gpt-[0-9][0-9.]*(?:-(?:mini|nano|turbo|codex))?|o[134](?:-(?:mini|pro))?|gemini-[0-9][0-9.]*-(?:pro|flash|flash-lite))$",
        )
        .expect("a valid pattern")
    });
    (s.len() <= 64 && form.is_match(s)).then(|| s.to_string())
}

fn known(list: &[&str], s: &str) -> String {
    if list.contains(&s) {
        s.to_string()
    } else {
        "custom".to_string()
    }
}

fn hex(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

impl LoopRecord {
    /// The record of an ended loop, from its state and the runtime each definition names
    /// (`runtime_of`). `None` for a run that is not a loop or has not ended.
    pub fn of(s: &RunState, runtime_of: &dyn Fn(&str) -> Option<String>) -> Option<LoopRecord> {
        let l = s.feature_loop.as_ref()?;
        if !s.stage.is_final() {
            return None;
        }
        let raw: Option<crate::defs::pipeline::Raw> = toml::from_str(&l.pipeline).ok();
        let steps: Vec<StepRow> = raw
            .as_ref()
            .map(|r| {
                r.steps
                    .iter()
                    .map(|st| StepRow {
                        agent: known(AGENTS, &st.agent),
                        when: format!("{:?}", st.when).to_lowercase(),
                        at: if st.at() == crate::defs::pipeline::At::Start {
                            "start"
                        } else {
                            "end"
                        }
                        .into(),
                        paths: st.paths.len() as u32,
                        on_fail: format!("{:?}", st.on_fail).to_lowercase(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let has_gates = steps
            .iter()
            .any(|st| st.agent == "fallow:gates" && st.when == "task");
        let has_review = steps.iter().any(|st| {
            st.when == "checkpoint" && st.at == "end" && st.paths == 0 && st.agent != "fallow:gates"
        });
        // Agents by the definition each step ran, their runtime and model as recorded.
        let mut agents: Vec<AgentRow> = Vec::new();
        for st in s.steps.values() {
            let Some(def) = st.def.as_deref() else {
                continue;
            };
            let name = known(AGENTS, def);
            let runtime = known(RUNTIMES, &runtime_of(def).unwrap_or_default());
            let model = st.model.as_deref().and_then(|m| model_id(&runtime, m));
            match agents
                .iter_mut()
                .find(|a| a.agent == name && a.model == model && a.runtime == runtime)
            {
                Some(a) => {
                    a.steps += 1;
                    a.cost_usd = match (a.cost_usd, st.cost_usd) {
                        (Some(x), Some(y)) => Some(x + y),
                        _ => None,
                    };
                }
                None => agents.push(AgentRow {
                    agent: name,
                    runtime,
                    model,
                    steps: 1,
                    cost_usd: st.cost_usd,
                }),
            }
        }
        let mut q = Questions::default();
        for e in l.questions.values() {
            let was = |st: QuestionState| e.history.iter().any(|h| h.state == st);
            if was(QuestionState::WaitingEngineer) {
                q.relayed += 1;
            } else if was(QuestionState::Decided) {
                q.decided_by_lead += 1;
            } else if was(QuestionState::Answered) {
                q.answered_by_lead += 1;
            }
            if was(QuestionState::Overruled) {
                q.overruled += 1;
            }
            if was(QuestionState::Confirmed) {
                q.confirmed += 1;
            }
        }
        let live = |k: StepKind| {
            s.steps
                .values()
                .filter(|x| x.kind == k && !crate::run::machine::cut_short(x))
                .count() as u32
        };
        let outcome = if s.merged.is_some() {
            "merged"
        } else if s.stage == Stage::Done {
            if l.outcome.as_ref().is_some_and(|o| o.0) {
                "ready"
            } else {
                "done_unverified"
            }
        } else {
            "stopped"
        };
        let started = s
            .building_started
            .clone()
            .unwrap_or_else(|| s.created.clone());
        let end =
            crate::run::time::parse_rfc3339(&s.stage_at).unwrap_or_else(crate::run::time::unix_now);
        Some(LoopRecord {
            pipeline: known(PIPELINES, raw.as_ref().map_or("", |r| r.name.as_str())),
            verifying: has_gates && has_review,
            task_repairs_cap: raw.as_ref().map_or(0, |r| r.caps.task_repairs),
            review_rounds_cap: raw.as_ref().map_or(0, |r| r.caps.review_rounds),
            steps,
            level: match l.level {
                Level::Strict => "strict",
                Level::Normal => "normal",
                Level::Free => "free",
            }
            .into(),
            agents,
            phases: l.phases.len() as u32,
            // The tasks of the lock in force: one a new lock dropped no longer counts.
            tasks: l.phases.iter().map(|p| p.spec.tasks.len()).sum::<usize>() as u32,
            tasks_accepted: l
                .phases
                .iter()
                .flat_map(|p| &p.spec.tasks)
                .filter(|t| {
                    l.tasks
                        .get(&t.id)
                        .is_some_and(|x| x.state == crate::run::events::TaskState::Accepted)
                })
                .count() as u32,
            repairs: live(StepKind::Repair),
            review_rounds: live(StepKind::Review),
            questions: q,
            interventions: s.steering.iter().filter(|x| x.kind.intervention()).count() as u32,
            locks: l.lock,
            lock_commit: l.lock_commit.clone(),
            final_commit: s
                .merged
                .clone()
                .or_else(|| s.integrated.clone())
                .filter(|c| hex(c)),
            started,
            minutes: s.minutes_used(end),
            cost_usd: (s.cost_usd * 10_000.0).round() / 10_000.0,
            outcome: outcome.into(),
        })
    }

    /// Checks that only fixed values, identifiers and numbers sit where text could go; a record
    /// that fails is never written.
    pub fn check(&self) -> Result<(), String> {
        let fixed = |list: &[&str], v: &str, what: &str| {
            if list.contains(&v) {
                Ok(())
            } else {
                Err(format!("{what} {v:?} is not a fixed value"))
            }
        };
        fixed(PIPELINES, &self.pipeline, "pipeline")?;
        fixed(LEVELS, &self.level, "level")?;
        fixed(OUTCOMES, &self.outcome, "outcome")?;
        for st in &self.steps {
            fixed(AGENTS, &st.agent, "agent")?;
            fixed(WHEN, &st.when, "when")?;
            fixed(AT, &st.at, "at")?;
            fixed(ON_FAIL, &st.on_fail, "on_fail")?;
        }
        for a in &self.agents {
            fixed(AGENTS, &a.agent, "agent")?;
            fixed(RUNTIMES, &a.runtime, "runtime")?;
            if a.model
                .as_deref()
                .is_some_and(|m| model_id(&a.runtime, m).is_none())
            {
                return Err("a model must be a provider's id".into());
            }
        }
        if !hex(&self.lock_commit) || self.final_commit.as_deref().is_some_and(|c| !hex(c)) {
            return Err("commits must be identifiers".into());
        }
        if !self
            .started
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | 'Z'))
        {
            return Err("started must be a time".into());
        }
        Ok(())
    }
}

/// The records' folder under Fallow's data folder.
pub fn dir(data: &Path) -> PathBuf {
    data.join(DIR)
}

/// The record of loop `id`, once written.
pub fn path(data: &Path, id: u64) -> PathBuf {
    dir(data).join(format!("loop-{id}.json"))
}

/// Writes the record of loop `id` once; a loop recorded already is left as it is.
pub fn write(data: &Path, id: u64, r: &LoopRecord) -> Result<bool, String> {
    r.check()?;
    let p = path(data, id);
    if p.exists() {
        return Ok(false);
    }
    let d = dir(data);
    crate::platform::private_dir(&d)?;
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(true)
}

/// Every readable record, by loop.
pub fn list(data: &Path) -> Vec<(u64, LoopRecord)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(data)) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        let Some(id) = p
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("loop-"))
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if let Some(r) = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            out.push((id, r));
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fixed_values_pass_the_check() {
        let mut r = LoopRecord {
            pipeline: "standard".into(),
            verifying: true,
            steps: vec![StepRow {
                agent: "builder".into(),
                when: "task".into(),
                at: "end".into(),
                paths: 0,
                on_fail: "repair".into(),
            }],
            task_repairs_cap: 3,
            review_rounds_cap: 3,
            level: "normal".into(),
            agents: vec![AgentRow {
                agent: "builder".into(),
                runtime: "claude".into(),
                model: Some("sonnet".into()),
                steps: 4,
                cost_usd: Some(0.4),
            }],
            phases: 3,
            tasks: 4,
            tasks_accepted: 4,
            repairs: 0,
            review_rounds: 2,
            questions: Questions::default(),
            interventions: 0,
            locks: 1,
            lock_commit: "abc123".into(),
            final_commit: None,
            started: "2026-10-31T10:00:00Z".into(),
            minutes: 12,
            cost_usd: 0.4,
            outcome: "merged".into(),
        };
        assert!(r.check().is_ok());
        r.steps[0].agent = "my-secret-checker".into();
        assert!(r.check().is_err());
        r.steps[0].agent = "custom".into();
        r.agents[0].model = Some("my words".into());
        assert!(r.check().is_err());
        assert_eq!(
            model_id("claude", "claude-sonnet-4-5"),
            Some("claude-sonnet-4-5".into())
        );
        // 017: a name of its runtime's declared form is kept, a name of another's is not.
        for ok in ["claude-fable-5-2", "claude-opus-5-5", "claude-sonnet-5-5"] {
            assert_eq!(model_id("claude", ok).as_deref(), Some(ok), "{ok}");
        }
        assert_eq!(model_id("fake", "fake-1").as_deref(), Some("fake-1"));
        assert_eq!(model_id("claude", "fake-1"), None, "not claude's form");
        for bad in ["my words", "Claude-Opus-5-5", "claude-opus-5-5 my words"] {
            assert_eq!(model_id("claude", bad), None, "{bad}");
        }
        r.agents[0].model = Some("claude-fable-5-2".into());
        assert!(r.check().is_ok());
        // The forms kept before 017, so records written then still validate.
        for ok in [
            "sonnet",
            "claude-opus-4-1-20250805",
            "claude-sonnet-4-5",
            "claude-3-5-haiku-20241022",
            "gpt-5",
            "o3",
            "o4-mini",
            "gemini-2.5-pro",
        ] {
            assert_eq!(model_id("custom", ok).as_deref(), Some(ok), "{ok}");
        }
        for bad in [
            "o3acme-merger",
            "claude-projectfalcon",
            "sonnet-for-acme",
            "gpt-acme",
            "claude-4-acme",
        ] {
            assert_eq!(model_id("custom", bad), None, "{bad}");
        }
    }
}
