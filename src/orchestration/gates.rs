//! Gate runs, failure names and classification against the baseline (004 US4, research R-5, R-6).
//!
//! A gate is a command session (the shell profile, no orchestration, role `gate`). Its exit
//! status is the only input for `gate-<name>-passed/-failed`, which Fallow signals itself
//! (FR-020). Failing checks are named from the session's output, in memory, so that a final run
//! can be compared with the baseline (FR-021).

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Instant;

use regex::Regex;

use super::squad::Failures;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On the starting commit, before builders begin.
    Baseline,
    /// On the lead's worktree when every builder task is done.
    Final,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Baseline => "baseline",
            Kind::Final => "final",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    Passed,
    Failed(Option<i32>),
    TimedOut,
}

impl RunState {
    pub fn label(&self) -> String {
        match self {
            RunState::Running => "running".into(),
            RunState::Passed => "passed".into(),
            RunState::Failed(Some(c)) => format!("failed (exit {c})"),
            RunState::Failed(None) => "failed".into(),
            RunState::TimedOut => "failed (timed out)".into(),
        }
    }

    pub fn passed(&self) -> bool {
        *self == RunState::Passed
    }
}

/// A final run compared with the baseline (research R-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Pass,
    /// It failed, but only where the baseline already failed.
    BaselineOnly,
    /// A check failed that passed in the baseline.
    Regression,
    /// It failed, and there are no names or no baseline to compare with.
    Fail,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Pass => "pass",
            Class::BaselineOnly => "failures already in the baseline only",
            Class::Regression => "regression",
            Class::Fail => "fail",
        }
    }

    /// Whether a merge condition `gate:<name>` holds with this result.
    pub fn opens(self) -> bool {
        matches!(self, Class::Pass | Class::BaselineOnly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRun {
    pub gate: String,
    pub kind: Kind,
    pub session: u32,
    /// What it ran against.
    pub commit: Option<String>,
    pub state: RunState,
    pub failing: BTreeSet<String>,
    /// Final runs, once finished.
    pub class: Option<Class>,
    pub started: Instant,
    pub deadline: Instant,
}

fn auto_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // cargo test
            r"^test (\S+) \.\.\. FAILED",
            // pytest short summary
            r"^FAILED (\S+)",
            // Go
            r"^\s*--- FAIL: (\S+)",
            // .NET
            r"^\s*Failed (\S+)",
            // Jest / Vitest: a failing test, then the file summary
            r"^\s*[✕×] (.+?)(?: \(\d+ ?m?s\))?$",
            r"^\s*FAIL (\S+)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid pattern"))
        .collect()
    })
}

/// The names of failing checks in a gate's output (research R-5). `ExitCode`: none.
pub fn failing_names(output: &str, how: &Failures) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let one = |re: &Regex, out: &mut BTreeSet<String>| {
        for line in output.lines() {
            if let Some(c) = re.captures(line.trim_end()).and_then(|c| c.get(1)) {
                out.insert(c.as_str().trim().to_string());
            }
        }
    };
    match how {
        Failures::ExitCode => {}
        Failures::Auto => {
            for re in auto_patterns() {
                one(re, &mut out);
            }
        }
        Failures::Regex(p) => {
            if let Ok(re) = Regex::new(p) {
                one(&re, &mut out);
            }
        }
    }
    out
}

/// Classifies a finished final run against the baseline's run (research R-5).
pub fn classify(final_run: &GateRun, baseline: Option<&GateRun>) -> Class {
    if final_run.state.passed() {
        return Class::Pass;
    }
    let Some(b) = baseline.filter(|b| b.state != RunState::Running) else {
        return Class::Fail;
    };
    if final_run.failing.is_empty() || b.state.passed() || b.failing.is_empty() {
        return if b.state.passed() && !final_run.failing.is_empty() {
            Class::Regression
        } else {
            Class::Fail
        };
    }
    if final_run.failing.is_subset(&b.failing) {
        Class::BaselineOnly
    } else {
        Class::Regression
    }
}

/// The command line a gate session runs: the shell with the gate's command for this OS
/// (research R-6).
pub fn shell_command(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let ps = if which("pwsh") { "pwsh" } else { "powershell" };
        (
            ps.into(),
            vec!["-NoProfile".into(), "-Command".into(), command.into()],
        )
    } else {
        let sh = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        (sh, vec!["-c".into(), command.into()])
    }
}

fn which(program: &str) -> bool {
    crate::session::profiles::resolve(program).is_ok()
}
