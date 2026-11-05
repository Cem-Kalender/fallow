//! Gates: the project's own build and test commands, run by Fallow itself and never reported by
//! an agent (constitution I, 006 research R-7). Each run is compared with the baseline taken on
//! the run's starting commit, so failures that were already there are told apart from
//! regressions (004 research R-5).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{Failures, GateDef};

/// How a gate run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed {
        exit: Option<i32>,
    },
    TimedOut,
    /// No tests ran (a new project): nothing to compare with.
    NoTests,
    /// The command could not be started.
    NotStarted {
        why: String,
    },
}

impl Outcome {
    pub fn passed(&self) -> bool {
        *self == Outcome::Passed
    }

    pub fn label(&self) -> String {
        match self {
            Outcome::Passed => "passed".into(),
            Outcome::Failed { exit: Some(c) } => format!("failed (exit {c})"),
            Outcome::Failed { exit: None } => "failed".into(),
            Outcome::TimedOut => "timed out".into(),
            Outcome::NoTests => "no tests ran".into(),
            Outcome::NotStarted { why } => format!("could not start: {why}"),
        }
    }
}

/// One gate run on one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: String,
    pub commit: String,
    pub outcome: Outcome,
    pub failing: BTreeSet<String>,
}

/// A run compared with the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

    /// Whether the result lets the work go on.
    pub fn ok(self) -> bool {
        matches!(self, Class::Pass | Class::BaselineOnly)
    }
}

/// The longest output kept from a gate run (its end is kept).
const KEEP: usize = 64 * 1024;

/// Runs a gate in `dir` with its time limit. Output lines go to `on_line` as they come; the
/// process tree ends at the time limit.
/// Runs one gate. Failing names are collected from every line as it arrives, so a long output
/// whose tail is kept still names every failure. `stop` ends the gate's processes early; the
/// result then says it did not finish.
pub fn run(
    def: &GateDef,
    dir: &Path,
    commit: &str,
    on_line: &mut dyn FnMut(&str),
    stop: &AtomicBool,
) -> GateResult {
    let (program, args) = shell_command(&def.command);
    let mut cmd = crate::platform::command(&program);
    cmd.args(&args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let done = |outcome, failing| GateResult {
        gate: def.name.clone(),
        commit: commit.to_string(),
        outcome,
        failing,
    };
    let group = match crate::platform::job::Group::new() {
        Ok(g) => g,
        Err(e) => return done(Outcome::NotStarted { why: e.to_string() }, BTreeSet::new()),
    };
    // Its own process group, as the agents': a stop ends what the gate started too.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return done(
                Outcome::NotStarted {
                    why: format!("{program}: {e}"),
                },
                BTreeSet::new(),
            );
        }
    };
    let _ = group.adopt(child.id());
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match std::io::BufRead::read_until(&mut r, b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let _ = tx.send(String::from_utf8_lossy(&buf).trim_end().to_string());
                    }
                }
            }
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(def.timeout_min * 60);
    let mut output = String::new();
    let mut failing = BTreeSet::new();
    let mut timed_out = false;
    let mut stopped = false;
    let status = loop {
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
            failing.extend(failing_names(&line, &def.failures));
            output.push_str(&line);
            output.push('\n');
            if output.len() > 2 * KEEP {
                let mut cut = output.len() - KEEP;
                while !output.is_char_boundary(cut) {
                    cut += 1;
                }
                output = output[cut..].to_string();
            }
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if stop.load(Ordering::Relaxed) => {
                group.kill();
                stopped = true;
                let _ = child.wait();
                break None;
            }
            Ok(None) if Instant::now() >= deadline => {
                group.kill();
                timed_out = true;
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    // What is left in the pipes once the process ended.
    for line in rx.iter() {
        on_line(&line);
        failing.extend(failing_names(&line, &def.failures));
        output.push_str(&line);
        output.push('\n');
    }
    let outcome = if stopped {
        Outcome::NotStarted {
            why: "stopped before it finished".into(),
        }
    } else if timed_out {
        Outcome::TimedOut
    } else {
        match status {
            Some(s) if s.success() => Outcome::Passed,
            _ if failing.is_empty() && no_tests(&output) => Outcome::NoTests,
            Some(s) => Outcome::Failed { exit: s.code() },
            None => Outcome::Failed { exit: None },
        }
    };
    done(outcome, failing)
}

fn auto_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // cargo test
            r"^test (\S+) \.\.\. FAILED",
            // pytest short summary; never unittest's own summary, `FAILED (failures=3)` (T086)
            r"^FAILED ([^\s(]\S*)",
            // unittest
            r"^(?:FAIL|ERROR): (\S+ \([^)]+\))",
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

/// The names of failing checks in a gate's output. `ExitCode`: none.
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

/// Whether a run's output says no tests ran (unittest, pytest, cargo, Jest, Go, .NET).
pub fn no_tests(output: &str) -> bool {
    // cargo prints "running 0 tests" for every target without tests even when others ran and
    // failed: a run with any test is not "no tests".
    static RAN: OnceLock<Regex> = OnceLock::new();
    let ran = RAN.get_or_init(|| {
        Regex::new(r"(?m)running [1-9][0-9]* tests?|Ran [1-9][0-9]* tests?|collected [1-9][0-9]* items?|^(FAIL|ok)\s")
            .expect("valid pattern")
    });
    if ran.is_match(output) || !failing_names(output, &Failures::Auto).is_empty() {
        return false;
    }
    const SAYS: &[&str] = &[
        "Ran 0 tests",
        "NO TESTS RAN",
        "no tests ran",
        "collected 0 items",
        "running 0 tests",
        "No tests found",
        "no test files",
        "No test is available",
    ];
    SAYS.iter().any(|s| output.contains(s))
}

/// Classifies a run against the baseline's run of the same gate.
pub fn classify(run: &GateResult, baseline: Option<&GateResult>) -> Class {
    if run.outcome.passed() {
        return Class::Pass;
    }
    let Some(b) =
        baseline.filter(|b| !matches!(b.outcome, Outcome::NoTests | Outcome::NotStarted { .. }))
    else {
        return Class::Fail;
    };
    if run.failing.is_empty() || b.outcome.passed() || b.failing.is_empty() {
        return if b.outcome.passed() && !run.failing.is_empty() {
            Class::Regression
        } else {
            Class::Fail
        };
    }
    if run.failing.is_subset(&b.failing) {
        Class::BaselineOnly
    } else {
        Class::Regression
    }
}

/// The shell and arguments that run a gate's command.
pub fn shell_command(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let ps = if crate::platform::find_program("pwsh").is_some() {
            "pwsh"
        } else {
            "powershell"
        };
        (
            ps.into(),
            vec!["-NoProfile".into(), "-Command".into(), command.into()],
        )
    } else {
        ("/bin/sh".into(), vec!["-c".into(), command.into()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(outcome: Outcome, failing: &[&str]) -> GateResult {
        GateResult {
            gate: "tests".into(),
            commit: "c".into(),
            outcome,
            failing: failing.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn classification_against_the_baseline() {
        let failed = || Outcome::Failed { exit: Some(1) };
        assert_eq!(classify(&r(Outcome::Passed, &[]), None), Class::Pass);
        assert_eq!(classify(&r(failed(), &["a"]), None), Class::Fail);
        assert_eq!(
            classify(&r(failed(), &["a"]), Some(&r(failed(), &["a", "b"]))),
            Class::BaselineOnly
        );
        assert_eq!(
            classify(&r(failed(), &["a", "c"]), Some(&r(failed(), &["a"]))),
            Class::Regression
        );
        assert_eq!(
            classify(&r(failed(), &["a"]), Some(&r(Outcome::Passed, &[]))),
            Class::Regression
        );
        assert_eq!(
            classify(&r(failed(), &["a"]), Some(&r(Outcome::NoTests, &[]))),
            Class::Fail
        );
    }

    #[test]
    fn unittest_failures_are_named() {
        let out = "FAIL: test_x (tests.test_cart.Cart)\nERROR: test_y (tests.test_cart.Cart)\nRan 4 tests\n\nFAILED (failures=1, errors=1)\n";
        let f = failing_names(out, &Failures::Auto);
        assert!(f.contains("test_x (tests.test_cart.Cart)"));
        assert!(f.contains("test_y (tests.test_cart.Cart)"));
        // The summary names no test (T086: it differed between runs and counted as new).
        assert_eq!(f.len(), 2, "{f:?}");
        assert!(
            failing_names("FAILED tests/test_a.py::test_b - assert 1", &Failures::Auto)
                .contains("tests/test_a.py::test_b")
        );
        assert!(!no_tests(out));
        assert!(no_tests("\nRan 0 tests in 0.000s\n\nNO TESTS RAN\n"));
    }
}
